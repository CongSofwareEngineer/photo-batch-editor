"""Main window: sign-in page, then a sidebar shell with the batch Editor (3 screens in a
QStackedWidget, spec section 8.1), the Photo editor, the Video editor and the Settings page;
plus the large viewer."""

from __future__ import annotations

import json
import logging
import os
import sys
from pathlib import Path
from typing import cast

from PySide6.QtCore import QStandardPaths, Qt, QThreadPool, QTimer, Signal
from PySide6.QtGui import QCloseEvent, QKeySequence, QShortcut
from PySide6.QtWidgets import QApplication, QHBoxLayout, QMainWindow, QMessageBox, QStackedWidget, QWidget

from core.auth import AuthStore
from core.backend import GpuInfo, get_backend
from core.batch import BatchReport, FileResult
from core.i18n import load_language, save_language, set_language, tr, tr_msg
from core.paths import APP_NAME, local_appdata_dir, resource_path
from core.presets import Preset
from core.scanner import OutputFolderError, default_output_dir, next_free_output_dir
from core.settings import AdjustmentSettings
from ui.icons import write_stylesheet_icons
from ui.image_viewer import ImageViewer
from ui.login_view import LoginView
from ui.notifier import Notifier, app_icon
from ui.photo.photo_editor_view import PhotoEditorView
from ui.prepare_view import PrepareView
from ui.results_view import ResultsView
from ui.run_view import RunView
from ui.settings_view import SettingsView
from ui.sidebar import Sidebar
from ui.video.video_editor_view import VideoEditorView
from ui.workers import BatchWorker, GpuDetectWorker

log = logging.getLogger(__name__)

TITLE = 'Photo Batch Editor'
STATE_FILE = 'state.json'
AUTH_FILE = 'auth.json'


def app_data_dir() -> Path:
    """``%APPDATA%\\PhotoBatchEditor`` (QStandardPaths.AppDataLocation)."""
    loc = QStandardPaths.writableLocation(QStandardPaths.StandardLocation.AppDataLocation)
    return Path(loc) if loc else Path.home() / f'.{APP_NAME}'


class MainWindow(QMainWindow):
    language_change_requested = Signal(str)  # handled by run_app: the window is rebuilt

    def __init__(self, signed_in: bool | None = None) -> None:
        """``signed_in``: skip/force the sign-in page (used when the window is rebuilt)."""
        super().__init__()
        self.setWindowTitle(TITLE)
        self.setWindowIcon(app_icon())
        self.resize(1440, 900)
        self.setMinimumSize(1100, 680)
        self.data_dir = app_data_dir()
        self.worker: BatchWorker | None = None
        self.viewer: ImageViewer | None = None
        self.report: BatchReport | None = None
        self._quit_after_cancel = False
        self.auth = AuthStore(self.data_dir / AUTH_FILE)

        # Editor: the 3 screens of the batch workflow
        self.prepare = PrepareView(self.data_dir / 'presets')
        self.run_view = RunView()
        self.results = ResultsView()
        self.stack = QStackedWidget()
        for w in (self.prepare, self.run_view, self.results):
            self.stack.addWidget(w)

        # Settings page
        self.settings_view = SettingsView(
            self.data_dir / 'presets',
            self.auth,
            self.data_dir,
            editor_settings=self.prepare.settings,
            editor_photo=self.prepare.file_list.current_path,
        )

        # Single-photo and video editors
        self.photo_view = PhotoEditorView()
        self.video_view = VideoEditorView()

        # Shell = sidebar + pages; root = login | shell
        self.sidebar = Sidebar()
        self.pages = QStackedWidget()
        self.pages.setObjectName('Pages')
        self.pages.addWidget(self.stack)
        self.pages.addWidget(self.photo_view)
        self.pages.addWidget(self.video_view)
        self.pages.addWidget(self.settings_view)
        shell = QWidget()
        shell.setObjectName('Shell')
        sl = QHBoxLayout(shell)
        sl.setContentsMargins(0, 0, 0, 0)
        sl.setSpacing(0)
        sl.addWidget(self.sidebar)
        sl.addWidget(self.pages, 1)
        self.shell = shell
        self.login = LoginView(self.auth)
        self.root = QStackedWidget()
        self.root.addWidget(self.login)
        self.root.addWidget(self.shell)
        self.setCentralWidget(self.root)
        self.notifier = Notifier(self)

        self.prepare.run_requested.connect(self.start_batch)
        self.prepare.manage_settings_requested.connect(self.manage_settings)
        self.prepare.presets_edited.connect(lambda: self.settings_view.filters.reload())
        self.prepare.device.device_changed.connect(lambda _d: self._sync_settings_device())
        self.run_view.cancel_requested.connect(self.confirm_cancel)
        self.results.open_viewer.connect(self.open_viewer)
        self.results.open_in_photo_editor.connect(self.open_in_photo_editor)
        self.prepare.file_list.open_in_photo_editor.connect(self.open_in_photo_editor)
        self.results.new_folder.connect(self.new_folder)
        self.results.run_again.connect(self.adjust_and_run_again)
        self.sidebar.page_requested.connect(self.show_page)
        self.sidebar.sign_out_requested.connect(self.sign_out)
        self.settings_view.filters.apply_requested.connect(self.use_setting_in_editor)
        self.settings_view.filters.presets_changed.connect(
            lambda: self.prepare.reload_presets(follow_changes=True)
        )
        self.settings_view.filters.preset_renamed.connect(self._setting_renamed)
        self.settings_view.account.sign_out_requested.connect(self.sign_out)
        self.settings_view.account.account_changed.connect(lambda: self.sidebar.set_user(self.auth.username))
        self.login.logged_in.connect(self._logged_in)
        self.login.language_selected.connect(self.request_language)
        self.settings_view.general.language_selected.connect(self.request_language)
        self.page_shortcuts: dict[str, QShortcut] = {}
        for keys, page in (
            ('Ctrl+1', 'editor'),
            ('Ctrl+2', 'photo'),
            ('Ctrl+3', 'video'),
            ('Ctrl+4', 'settings'),
        ):
            sc = QShortcut(QKeySequence(keys), self.shell)
            sc.setContext(Qt.ShortcutContext.WidgetWithChildrenShortcut)
            sc.activated.connect(lambda p=page: self.show_page(p))
            self.page_shortcuts[page] = sc

        self.load_state()
        self.sidebar.set_user(self.auth.username)
        self.show_page('editor')
        if signed_in if signed_in is not None else self.auth.is_remembered():
            self.root.setCurrentWidget(self.shell)
        else:
            self.root.setCurrentWidget(self.login)
            self.login.reset()
        # GPU detection in the background: the window appears immediately (section 10.3)
        self.detector = GpuDetectWorker(self)
        self.detector.detected.connect(self.gpu_detected)
        QTimer.singleShot(0, self.detector.start)

    # Sign-in / navigation ---------------------------------------------------------------------
    def _logged_in(self) -> None:
        self.sidebar.set_user(self.auth.username)
        self.settings_view.account.refresh()
        self.root.setCurrentWidget(self.shell)
        self.show_page('editor')

    def is_signed_in(self) -> bool:
        return self.root.currentWidget() is self.shell

    def request_language(self, code: str) -> None:
        if self.video_view.is_busy():
            QMessageBox.information(
                self,
                tr('Language'),
                tr('A video is being exported. Change the language when it has finished.'),
            )
            self.settings_view.general.reset()
            return
        if self.worker is not None:
            QMessageBox.information(
                self, tr('Language'), tr('A batch is running. Change the language when it has finished.')
            )
            self.settings_view.general.reset()
            return
        self.language_change_requested.emit(code)

    def sign_out(self) -> None:
        if self.worker is not None:
            QMessageBox.information(
                self, tr('Sign out'), tr('A batch is running. Wait for it to finish or cancel it first.')
            )
            return
        if (
            QMessageBox.question(self, tr('Sign out'), tr('Sign out? You will need your password next time.'))
            != QMessageBox.StandardButton.Yes
        ):
            return
        self.settings_view.filters.flush()
        self.save_state()
        self.auth.logout()
        if self.viewer is not None:
            self.viewer.close()
        self.login.reset()
        self.root.setCurrentWidget(self.login)

    def show_page(self, key: str) -> None:
        if key == 'settings':
            self.settings_view.filters.refresh_sample()
            self._sync_settings_device()
            self.pages.setCurrentWidget(self.settings_view)
        else:
            self.settings_view.filters.flush()
            self.pages.setCurrentWidget(
                {'photo': self.photo_view, 'video': self.video_view}.get(key, self.stack)
            )
        if key != 'video':
            self.video_view.preview.pause()
        # Ctrl+1 is "100 %" in the photo editor (Photoshop): the page shortcut steps aside there
        self.page_shortcuts['editor'].setEnabled(key != 'photo')
        self.sidebar.set_page(key)

    def current_page(self) -> str:
        w = self.pages.currentWidget()
        return {self.settings_view: 'settings', self.photo_view: 'photo', self.video_view: 'video'}.get(
            w, 'editor'
        )

    def open_in_photo_editor(self, path: Path) -> None:
        """Right-click > Open in Photo editor (batch input list or results)."""
        if not self.photo_view.confirm_discard():
            return
        self.show_page('photo')
        self.photo_view.open_path(Path(path), confirm=False)

    def manage_settings(self, name: str) -> None:
        self.settings_view.show_tab(0)
        if name:
            self.settings_view.filters.reload(select=name)
        self.show_page('settings')

    def use_setting_in_editor(self, preset: Preset) -> None:
        if self.worker is not None:
            return
        self.prepare.apply_preset_name(preset.name)
        if self.stack.currentWidget() is self.results:
            self.stack.setCurrentWidget(self.prepare)
        self.show_page('editor')

    def _setting_renamed(self, old: str, new: str) -> None:
        cur = self.prepare.current_preset
        if cur is not None and not cur.builtin and cur.name == old:
            self.prepare.reload_presets(select=new)

    def _sync_settings_device(self) -> None:
        info = self.prepare.gpu_info
        eff = self.prepare.device.effective_device()
        self.settings_view.filters.set_device(eff, bool(eff == 'gpu' and info and info.sr_cuda_available))

    # State ----------------------------------------------------------------------------------
    def load_state(self) -> None:
        state: dict = {}
        try:
            state = json.loads((self.data_dir / STATE_FILE).read_text(encoding='utf-8'))
        except (OSError, ValueError):
            pass
        if not isinstance(state, dict):
            state = {}
        self.prepare.device.set_device('cpu' if state.get('device') == 'cpu' else 'gpu')
        preset = state.get('preset')
        if isinstance(preset, str):
            self.prepare.select_preset_name(preset)
        self.prepare.set_settings(AdjustmentSettings.from_dict(state.get('settings')))
        folder = state.get('last_folder')
        if isinstance(folder, str) and folder and Path(folder).is_dir():
            self.prepare.set_folder(Path(folder))
        sample = state.get('settings_sample')
        if isinstance(sample, str) and sample:
            self.settings_view.filters.set_sample(Path(sample))
        selected = state.get('settings_selected')
        if isinstance(selected, str) and selected:
            self.settings_view.filters.reload(select=selected)

    def save_state(self) -> None:
        filters = self.settings_view.filters
        state = {
            'version': 1,
            'device': self.prepare.device.device(),
            'settings': self.prepare.settings().to_dict(),
            'last_folder': str(self.prepare.folder) if self.prepare.folder else '',
            'preset': self.prepare.current_preset.name if self.prepare.current_preset else '',
            'settings_sample': str(filters.sample_path) if filters.sample_path else '',
            'settings_selected': filters.current.name if filters.current else '',
        }
        try:
            self.data_dir.mkdir(parents=True, exist_ok=True)
            tmp = self.data_dir / (STATE_FILE + '.tmp')
            tmp.write_text(json.dumps(state, indent=2, ensure_ascii=False), encoding='utf-8')
            tmp.replace(self.data_dir / STATE_FILE)
        except OSError:
            log.warning('Could not save state', exc_info=True)

    # GPU -------------------------------------------------------------------------------------
    def gpu_detected(self, info: GpuInfo) -> None:
        log.info('GPU detection: available=%s name=%s reason=%s', info.available, info.name, info.reason)
        self.prepare.set_gpu_info(info)
        self._sync_settings_device()

    # Run ---------------------------------------------------------------------------------------
    def _choose_output_dir(self, folder: Path) -> Path | None:
        try:
            out = default_output_dir(folder)
        except OutputFolderError as exc:
            self.prepare.show_error(tr_msg(str(exc)))
            return None
        if not out.exists():
            return out
        box = QMessageBox(self)
        box.setIcon(QMessageBox.Icon.Question)
        box.setWindowTitle(tr('Output folder exists'))
        box.setText(tr('The output folder already exists:\n{path}', path=out))
        box.setInformativeText(tr('Overwrite replaces files with the same name and keeps the others.'))
        overwrite = box.addButton(tr('Overwrite'), QMessageBox.ButtonRole.DestructiveRole)
        new = box.addButton(tr('Create New Folder'), QMessageBox.ButtonRole.AcceptRole)
        box.addButton(tr('Cancel'), QMessageBox.ButtonRole.RejectRole)
        box.setDefaultButton(new)
        box.exec()
        if box.clickedButton() is overwrite:
            return out
        if box.clickedButton() is new:
            return next_free_output_dir(folder)
        return None

    def start_batch(self) -> None:
        if self.worker is not None:
            return
        folder, files = self.prepare.folder, list(self.prepare.files)
        if folder is None or not files:
            self.prepare.show_error(tr('This folder has no supported photos.'))
            return
        out = self._choose_output_dir(folder)
        if out is None:
            return
        settings = self.prepare.settings()
        device = self.prepare.device.device()
        backend = get_backend(self.prepare.device.effective_device())
        self.save_state()
        self.prepare.set_locked(True)
        self.settings_view.filters.set_locked(True)
        self.sidebar.set_status(tr('Batch running…'))
        self.run_view.start(folder, out, len(files), backend)
        self.stack.setCurrentWidget(self.run_view)
        self.worker = BatchWorker(folder, settings, device, out, files)
        self.worker.progress.connect(self._progress)
        self.worker.device_changed.connect(lambda _d: self.run_view.gpu_fallback())
        self.worker.finished_report.connect(self.batch_finished)
        self.worker.failed.connect(self.batch_failed)
        self.worker.start()

    def _progress(self, done: int, total: int, current: str, res: FileResult | None) -> None:
        self.run_view.on_progress(done, total, current, res)
        self.setWindowTitle(f'{self.run_view.percent()}% — {TITLE}')

    def confirm_cancel(self) -> bool:
        if self.worker is None:
            return True
        if (
            QMessageBox.question(self, tr('Cancel'), tr('Stop editing? Photos already finished are kept.'))
            != QMessageBox.StandardButton.Yes
        ):
            return False
        self.worker.cancel()
        self.run_view.set_cancelling()
        return True

    def _end_batch(self) -> None:
        self.run_view.stop()
        if self.worker is not None:
            self.worker.wait(2000)
            self.worker.deleteLater()
        self.worker = None
        self.prepare.set_locked(False)
        self.settings_view.filters.set_locked(False)
        self.sidebar.set_status('')
        self.setWindowTitle(TITLE)

    def batch_finished(self, report: BatchReport) -> None:
        self._end_batch()
        self.report = report
        if self._quit_after_cancel:
            self.close()
            return
        self.results.set_report(report)
        self.stack.setCurrentWidget(self.results)
        self.notifier.notify(report)

    def batch_failed(self, message: str) -> None:
        self._end_batch()
        self.stack.setCurrentWidget(self.prepare)
        self.prepare.show_error(tr('The batch could not run: {error}', error=tr_msg(message)))
        if self._quit_after_cancel:
            self.close()

    # Results / viewer -------------------------------------------------------------------------
    def open_viewer(self, items: list[FileResult], index: int) -> None:
        if not items:
            return
        if self.viewer is None:
            self.viewer = ImageViewer()
            self.viewer.setWindowIcon(app_icon())
            self.viewer.closed.connect(self._viewer_closed)
        self.viewer.open(items, index)

    def _viewer_closed(self, r: FileResult | None) -> None:
        if r is not None:
            self.results.select_result(r)
        self.activateWindow()

    def new_folder(self) -> None:
        self.results.clear()
        self.prepare.set_folder(None)
        self.stack.setCurrentWidget(self.prepare)
        self.show_page('editor')
        QTimer.singleShot(0, self.prepare.choose_folder)

    def adjust_and_run_again(self) -> None:
        self.stack.setCurrentWidget(self.prepare)
        self.show_page('editor')

    # Close ---------------------------------------------------------------------------------------
    def closeEvent(self, e: QCloseEvent) -> None:  # noqa: N802
        if self.worker is None and not self._quit_after_cancel:
            if self.video_view.is_busy():
                if (
                    QMessageBox.question(
                        self, tr('Quit'), tr('A video is being exported. Cancel it and quit?')
                    )
                    != QMessageBox.StandardButton.Yes
                ):
                    e.ignore()
                    return
            elif not self.photo_view.confirm_discard() or not self.video_view.confirm_discard():
                e.ignore()
                return
        if self.worker is not None:
            if self._quit_after_cancel:
                e.ignore()
                return
            if (
                QMessageBox.question(self, tr('Quit'), tr('A batch is running. Cancel it and quit?'))
                != QMessageBox.StandardButton.Yes
            ):
                e.ignore()
                return
            self._quit_after_cancel = True
            self.worker.cancel()
            self.run_view.set_cancelling()
            e.ignore()  # closes once the batch has stopped
            return
        self.settings_view.filters.flush()
        self.save_state()
        self.video_view.shutdown()
        if self.viewer is not None:
            self.viewer.close()
        if self.notifier.tray is not None:
            self.notifier.tray.hide()
        if self.detector.isRunning():
            self.detector.wait(5000)
        super().closeEvent(e)


def load_stylesheet() -> str:
    qss = ''
    for path in (resource_path('ui', 'theme.qss'), Path(__file__).with_name('theme.qss')):
        try:
            qss = path.read_text(encoding='utf-8')
            break
        except OSError:
            continue
    try:
        icons = write_stylesheet_icons(local_appdata_dir() / 'qss-icons')
    except OSError:
        log.warning('Could not write the style sheet icons', exc_info=True)
        icons = {}
    for key, path in icons.items():
        qss = qss.replace(key, path)
    return qss


def run_app() -> int:
    logging.basicConfig(level=logging.INFO, format='%(asctime)s %(levelname)s %(name)s: %(message)s')
    QApplication.setApplicationName(APP_NAME)
    QApplication.setOrganizationName('')
    QApplication.setApplicationDisplayName(TITLE)
    app = cast(QApplication, QApplication.instance() or QApplication(sys.argv))
    app.setStyle('Fusion')
    app.setStyleSheet(load_stylesheet())
    app.setWindowIcon(app_icon())
    data_dir = app_data_dir()
    set_language(load_language(data_dir))
    reloader = None
    current: list[MainWindow] = []

    def open_window(old: MainWindow | None = None) -> None:
        w = MainWindow(signed_in=old.is_signed_in() if old is not None else None)
        w.language_change_requested.connect(switch_language)
        if old is not None:  # same place, same page, same photo / video
            w.restoreGeometry(old.saveGeometry())
            w.photo_view.adopt_document(old.photo_view.take_document(), old.photo_view.last_dir)
            w.video_view.adopt_state(old.video_view.take_state(), old.video_view.last_dir)
            page = old.current_page()
            if page != 'editor':
                w.show_page(page)
            if page == 'settings':
                w.settings_view.show_tab(old.settings_view.tabs.currentIndex())
        current[:] = [w]
        if reloader is not None:
            reloader.attach(w)
        w.show()

    def switch_language(code: str) -> None:
        old = current[0]
        old.settings_view.filters.flush()
        old.save_state()  # the new window loads it
        set_language(code)
        save_language(data_dir, code)
        open_window(old)
        old.close()
        dispose_when_idle(old)

    def dispose_when_idle(old: MainWindow) -> None:
        """Delete the replaced window once its background tasks (thumbnails, preview, Super
        Resolution warm-up…) have finished: their results are delivered to its widgets."""
        pools = old.findChildren(QThreadPool) + (old.viewer.findChildren(QThreadPool) if old.viewer else [])
        for p in pools:
            p.clear()

        def check() -> None:
            if any(p.activeThreadCount() for p in pools):
                QTimer.singleShot(200, check)
            else:  # let already queued results arrive first
                QTimer.singleShot(0, old.deleteLater)

        check()

    if os.environ.get('PBE_DEV') == '1':  # dev.bat: live reload, see ui/devtools.py
        from ui.devtools import DevReloader

        reloader = DevReloader()
    open_window()
    rc = app.exec()
    if reloader is not None and reloader.reload_pending:
        from ui.devtools import RELOAD_EXIT_CODE

        return RELOAD_EXIT_CODE
    return rc
