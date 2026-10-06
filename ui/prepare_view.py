"""Screen 1 — Prepare: folder, adjustments, preview, RUN (spec sections 8.2, 8.7–8.10)."""

from __future__ import annotations

import logging
from pathlib import Path

from PySide6.QtCore import QEvent, QObject, QSize, Qt, QThreadPool, QTimer, Signal
from PySide6.QtGui import QAction, QDragEnterEvent, QDropEvent, QKeyEvent, QKeySequence, QShortcut
from PySide6.QtWidgets import (
    QAbstractSpinBox,
    QApplication,
    QComboBox,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QInputDialog,
    QLabel,
    QLineEdit,
    QMenu,
    QMessageBox,
    QSizePolicy,
    QSplitter,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from core.backend import GpuInfo
from core.enhance import SR_SKIP_WARNING, get_shared_resolver
from core.i18n import tr, tr_msg, trn
from core.image_size import CLAMP_WARNING
from core.io_utils import image_dimensions
from core.pipeline import predict_output_size
from core.presets import (
    Preset,
    delete_user_preset,
    display_name,
    list_builtin_presets,
    list_user_presets,
    save_user_preset,
    unique_name,
)
from core.scanner import OutputFolderError, default_output_dir, scan_folder
from core.settings import AdjustmentSettings
from ui.adjustment_panel import AdjustmentPanel
from ui.device_status import DeviceStatus
from ui.file_list import FileList
from ui.icons import icon
from ui.preview import PreviewPane
from ui.widgets import ElidedLabel, button
from ui.workers import Task

log = logging.getLogger(__name__)


class PrepareView(QWidget):
    run_requested = Signal()
    folder_changed = Signal(object)  # Path | None
    manage_settings_requested = Signal(str)  # name of the current setting
    presets_edited = Signal()  # a setting was saved/deleted from here

    def __init__(self, user_preset_dir: Path, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setAcceptDrops(True)
        self.user_preset_dir = user_preset_dir
        self.folder: Path | None = None
        self.files: list[Path] = []
        self.dims: dict[Path, tuple[int, int]] = {}
        self.gpu_info: GpuInfo | None = None
        self._scan_gen = 0
        self.pool = QThreadPool(self)
        self.pool.setMaxThreadCount(1)
        self.sr_pool = QThreadPool(self)
        self.sr_pool.setMaxThreadCount(1)
        self._sr_warmed: set[bool] = set()
        self.presets: list[Preset] = []
        self.current_preset: Preset | None = None

        # Top bar
        self.choose_btn = button(
            tr('Choose Folder'),
            'folder',
            'Accent',
            tr('Choose the folder with the photos to edit (Ctrl+O) — or drop a folder here'),
        )
        self.folder_label = ElidedLabel(
            tr('No folder selected — click Choose Folder or drop a folder onto the window')
        )
        self.folder_label.setProperty('secondary', True)
        self.count_label = QLabel('')
        self.count_label.setObjectName('Badge')
        self.count_label.setProperty('kind', 'accent')
        self.count_label.setSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Fixed)
        self.count_label.hide()
        self.preset_combo = QComboBox()
        self.preset_combo.setMinimumWidth(200)
        self.preset_combo.setToolTip(tr('Saved filter setting applied to the photos'))
        self.dirty_label = QLabel(tr('● Modified'))
        self.dirty_label.setObjectName('Warning')
        self.dirty_label.setToolTip(
            tr(
                'The adjustments differ from the selected setting — '
                'use the menu to update it or save a new one'
            )
        )
        self.dirty_label.hide()
        self.preset_menu_btn = QToolButton()
        self.preset_menu_btn.setIcon(icon('menu'))
        self.preset_menu_btn.setToolTip(tr('Setting menu'))
        self.preset_menu_btn.setPopupMode(QToolButton.ToolButtonPopupMode.InstantPopup)
        self.manage_btn = button(tr('Manage'), 'layers', tooltip=tr('Create and edit your saved settings'))
        top_box = QFrame()
        top_box.setObjectName('Panel')
        top = QHBoxLayout(top_box)
        top.setContentsMargins(10, 8, 10, 8)
        top.setSpacing(8)
        top.addWidget(self.choose_btn)
        top.addWidget(self.folder_label, 1)
        top.addWidget(self.count_label)
        top.addSpacing(14)
        setting_icon = QLabel()
        setting_icon.setPixmap(icon('sliders', '#6aa8ff').pixmap(16, 16))
        top.addWidget(setting_icon)
        cap = QLabel(tr('Setting'))
        cap.setProperty('secondary', True)
        top.addWidget(cap)
        top.addWidget(self.preset_combo)
        top.addWidget(self.dirty_label)
        top.addWidget(self.preset_menu_btn)
        top.addWidget(self.manage_btn)

        self.error_label = QLabel()
        self.error_label.setObjectName('Error')
        self.error_label.hide()

        # Middle
        self.file_list = FileList()
        files_box = QFrame()
        files_box.setObjectName('Panel')
        fl = QVBoxLayout(files_box)
        fl.setContentsMargins(8, 6, 8, 8)
        t = QLabel(tr('PHOTOS'))
        t.setObjectName('SectionTitle')
        fl.addWidget(t)
        fl.addWidget(self.file_list, 1)
        self.preview = PreviewPane()
        self.panel = AdjustmentPanel()
        self.hide_preview_btn = button(tr('Hide Preview'), 'eye-off', 'Ghost')
        self.hide_preview_btn.setCheckable(True)
        self.splitter = QSplitter(Qt.Orientation.Horizontal)
        self.splitter.addWidget(files_box)
        self.splitter.addWidget(self.preview)
        self.splitter.addWidget(self.panel)
        self.splitter.setStretchFactor(0, 0)
        self.splitter.setStretchFactor(1, 1)
        self.splitter.setStretchFactor(2, 0)
        self.splitter.setSizes([240, 800, 360])
        self.splitter.setChildrenCollapsible(False)

        # Bottom bar
        self.output_label = ElidedLabel(tr('Output: —'))
        self.output_label.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        self.size_label = QLabel('')
        self.size_label.setProperty('secondary', True)
        self.limit_label = QLabel()
        self.limit_label.setObjectName('Warning')
        self.limit_label.hide()
        self.sr_cpu_label = QLabel(tr('Super Resolution on CPU is very slow'))
        self.sr_cpu_label.setObjectName('Warning')
        self.sr_cpu_label.hide()
        self.device = DeviceStatus()
        self.run_btn = button(
            tr('RUN'), 'play', 'RunButton', tr('Apply the settings to every photo (Ctrl+Enter)')
        )
        self.run_btn.setIconSize(QSize(18, 18))
        self.run_btn.setEnabled(False)

        bottom = QFrame()
        bottom.setObjectName('Panel')
        bl = QVBoxLayout(bottom)
        bl.setContentsMargins(14, 8, 14, 10)
        r1 = QHBoxLayout()
        r1.addWidget(self.output_label, 1)
        r1.addWidget(self.size_label)
        r2 = QHBoxLayout()
        r2.addWidget(self.limit_label)
        r2.addWidget(self.sr_cpu_label)
        r2.addStretch(1)
        r3 = QHBoxLayout()
        r3.addWidget(self.hide_preview_btn)
        r3.addStretch(1)
        r3.addWidget(self.device)
        r3.addSpacing(12)
        r3.addWidget(self.run_btn)
        bl.addLayout(r1)
        bl.addLayout(r2)
        bl.addLayout(r3)

        lay = QVBoxLayout(self)
        lay.setContentsMargins(16, 14, 16, 14)
        lay.setSpacing(10)
        lay.addWidget(top_box)
        lay.addWidget(self.error_label)
        lay.addWidget(self.splitter, 1)
        lay.addWidget(bottom)

        # Wiring
        self.choose_btn.clicked.connect(self.choose_folder)
        self.file_list.current_path_changed.connect(self._selected_changed)
        self.panel.settings_changed.connect(self._settings_changed)
        self.panel.save_preset_requested.connect(self.save_new_preset)
        self.device.device_changed.connect(lambda _d: self._device_changed())
        self.run_btn.clicked.connect(self._run_clicked)
        self.hide_preview_btn.toggled.connect(self._toggle_preview)
        self.preset_combo.activated.connect(self._preset_chosen)
        self.manage_btn.clicked.connect(
            lambda: self.manage_settings_requested.emit(
                self.current_preset.name if self.current_preset else ''
            )
        )
        self._build_preset_menu()

        for keys, slot in (
            ('Ctrl+O', self.choose_folder),
            ('Ctrl+Return', self._run_clicked),
            ('Ctrl+Enter', self._run_clicked),
            ('Ctrl+R', self.panel.reset_all),
            ('Ctrl+S', self.save_new_preset),
        ):
            sc = QShortcut(QKeySequence(keys), self)
            sc.setContext(Qt.ShortcutContext.WidgetWithChildrenShortcut)
            sc.activated.connect(slot)
        QApplication.instance().installEventFilter(self)
        self._settings_timer = QTimer(self)
        self._settings_timer.setSingleShot(True)
        self._settings_timer.setInterval(150)
        self._settings_timer.timeout.connect(self.update_output_info)
        self.reload_presets()

    # Folder ---------------------------------------------------------------------------------
    def choose_folder(self) -> None:
        if not self.choose_btn.isEnabled():
            return
        start = str(self.folder) if self.folder else ''
        path = QFileDialog.getExistingDirectory(self, tr('Choose the folder with the photos to edit'), start)
        if path:
            self.set_folder(Path(path))

    def set_folder(self, folder: Path | None) -> None:
        self._scan_gen += 1
        self.error_label.hide()
        self.folder = None
        self.files = []
        self.dims = {}
        self.file_list.set_files(None, [])
        self.preview.set_source(None)
        self.run_btn.setEnabled(False)
        if folder is None:
            self.folder_label.setText(
                tr('No folder selected — click Choose Folder or drop a folder onto the window')
            )
            self.count_label.setText('')
            self.count_label.hide()
            self.output_label.setText(tr('Output: —'))
            self.update_output_info()
            self.folder_changed.emit(None)
            return
        folder = Path(folder)
        if not folder.is_dir():
            self.show_error(tr('Folder not found: {path}', path=folder))
            return
        self.folder = folder
        self.folder_label.setText(str(folder))
        self.folder_label.setProperty('secondary', False)
        self.folder_label.style().polish(self.folder_label)
        self.count_label.setText(tr('Scanning…'))
        self.count_label.show()
        try:
            self.output_label.setText(tr('Output: {path}', path=default_output_dir(folder)))
        except OutputFolderError as exc:
            self.output_label.setText(tr('Output: —'))
            self.show_error(tr_msg(str(exc)))
        gen = self._scan_gen
        Task.submit(
            self.pool,
            lambda: scan_folder(folder),
            lambda files: self._scanned(gen, folder, files),
            lambda err: self.show_error(tr('Cannot read folder: {error}', error=err)),
        )
        self.folder_changed.emit(folder)

    def _scanned(self, gen: int, folder: Path, files: list[Path]) -> None:
        if gen != self._scan_gen:
            return
        self.files = files
        n = len(files)
        self.count_label.setText(trn('{n} photo', '{n} photos', n))
        if not files:
            self.show_error(
                tr('This folder has no supported photos (.jpg .jpeg .png .tif .tiff .webp .bmp).')
            )
        self.file_list.set_files(folder, files)
        self._update_run_enabled()
        Task.submit(
            self.pool,
            lambda: {f: image_dimensions(f) for f in files},
            lambda dims: self._dims_ready(gen, dims),
        )

    def _dims_ready(self, gen: int, dims: dict[Path, tuple[int, int] | None]) -> None:
        if gen == self._scan_gen:
            self.dims = {k: v for k, v in dims.items() if v}
            self.update_output_info()

    def show_error(self, text: str) -> None:
        self.error_label.setText('✗ ' + text)
        self.error_label.show()

    def _update_run_enabled(self) -> None:
        ok = bool(self.folder and self.files) and self.choose_btn.isEnabled()
        try:
            if self.folder:
                default_output_dir(self.folder)
        except OutputFolderError:
            ok = False
        self.run_btn.setEnabled(ok)

    def dragEnterEvent(self, e: QDragEnterEvent) -> None:  # noqa: N802
        if e.mimeData().hasUrls() and any(u.isLocalFile() for u in e.mimeData().urls()):
            e.acceptProposedAction()

    def dropEvent(self, e: QDropEvent) -> None:  # noqa: N802
        if not self.choose_btn.isEnabled():
            return
        for u in e.mimeData().urls():
            if u.isLocalFile():
                p = Path(u.toLocalFile())
                self.set_folder(p if p.is_dir() else p.parent)
                e.acceptProposedAction()
                return

    # Settings / device ----------------------------------------------------------------------
    def settings(self) -> AdjustmentSettings:
        return self.panel.settings()

    def set_settings(self, s: AdjustmentSettings) -> None:
        self.panel.set_settings(s, emit=True)

    def _settings_changed(self) -> None:
        s = self.panel.settings()
        self.preview.set_settings(s)
        self.device.set_settings(s)
        self._update_dirty()
        self._settings_timer.start()
        self._warm_up_sr()

    def _sr_uses_cuda(self) -> bool:
        return bool(
            self.device.effective_device() == 'gpu' and self.gpu_info and self.gpu_info.sr_cuda_available
        )

    def _warm_up_sr(self) -> None:
        """Create the Super Resolution session in the background when SR is turned on (5.4)."""
        if self.panel.settings().super_resolution == 'off' or self.gpu_info is None:
            return
        cuda = self._sr_uses_cuda()
        if cuda in self._sr_warmed:
            return
        self._sr_warmed.add(cuda)
        Task.submit(
            self.sr_pool,
            lambda: get_shared_resolver(cuda).warmup(),
            on_error=lambda err: log.warning('Super Resolution warm-up failed: %s', err),
        )

    def set_gpu_info(self, info: GpuInfo) -> None:
        self.gpu_info = info
        self.device.set_gpu_info(info)
        self._device_changed()

    def _device_changed(self) -> None:
        eff = self.device.effective_device()
        self.preview.set_device(eff, self._sr_uses_cuda())
        self._warm_up_sr()
        self.update_output_info()

    def _selected_changed(self, path: Path | None) -> None:
        self.preview.set_source(path)
        self.update_output_info()

    def update_output_info(self) -> None:
        s = self.panel.settings()
        path = self.file_list.current_path()
        dims = self.dims.get(path) if path else None
        if dims:
            w, h, _ = predict_output_size(dims[0], dims[1], s)
            self.size_label.setText(
                tr('Output size: {w0}×{h0} → {w}×{h} px', w0=dims[0], h0=dims[1], w=w, h=h)
            )
        else:
            self.size_label.setText('')
        sr_over = size_over = 0
        if (s.sr_factor > 1 or not s.image_size.is_default()) and self.dims:
            for w0, h0 in self.dims.values():
                warnings = predict_output_size(w0, h0, s)[2]
                sr_over += SR_SKIP_WARNING in warnings
                size_over += CLAMP_WARNING in warnings
        parts = []
        if sr_over:
            parts.append(
                trn(
                    '⚠ {n} photo exceeds the Super Resolution limit and will be exported without it',
                    '⚠ {n} photos exceed the Super Resolution limit and will be exported without it',
                    sr_over,
                )
            )
        if size_over:
            parts.append(
                trn(
                    '⚠ {n} photo exceeds 20,000 px / 200 MP: Image Size will be clamped',
                    '⚠ {n} photos exceed 20,000 px / 200 MP: Image Size will be clamped',
                    size_over,
                )
            )
        self.limit_label.setText('    '.join(parts))
        self.limit_label.setVisible(bool(parts))
        sr_on_cpu = s.super_resolution != 'off' and not (
            self.device.effective_device() == 'gpu' and self.gpu_info and self.gpu_info.sr_cuda_available
        )
        self.sr_cpu_label.setVisible(bool(sr_on_cpu))

    def _toggle_preview(self, hidden: bool) -> None:
        self.preview.setVisible(not hidden)
        self.hide_preview_btn.setText(tr('Show Preview') if hidden else tr('Hide Preview'))
        self.hide_preview_btn.setIcon(icon('eye' if hidden else 'eye-off'))

    # Run / lock -----------------------------------------------------------------------------
    def _run_clicked(self) -> None:
        if not self.choose_btn.isEnabled():
            return
        if not self.folder:
            self.show_error(tr('Choose a folder first.'))
            return
        if not self.files:
            self.show_error(tr('This folder has no supported photos.'))
            return
        self.run_requested.emit()

    def set_locked(self, locked: bool) -> None:
        for w in (
            self.choose_btn,
            self.panel,
            self.preset_combo,
            self.preset_menu_btn,
            self.device,
            self.manage_btn,
        ):
            w.setEnabled(not locked)
        self.setAcceptDrops(not locked)
        self._update_run_enabled()

    # Presets --------------------------------------------------------------------------------
    def reload_presets(self, select: str | None = None, follow_changes: bool = False) -> None:
        """Re-read the settings. ``follow_changes``: if the selected setting was edited elsewhere
        (Settings page) and the Editor still shows its old values, show the new values."""
        old = self.current_preset
        unmodified = old is not None and self.panel.settings().to_dict() == old.settings.to_dict()
        user = list_user_presets(self.user_preset_dir)
        self.presets = user + list_builtin_presets()
        self.preset_combo.blockSignals(True)
        self.preset_combo.clear()
        for i, p in enumerate(self.presets):
            if p.builtin and i == len(user) and i > 0:
                self.preset_combo.insertSeparator(self.preset_combo.count())
            self.preset_combo.addItem(
                icon('sparkles' if p.builtin else 'sliders', '#22d3ee' if p.builtin else '#6aa8ff'),
                display_name(p),
                i,
            )
        self.preset_combo.blockSignals(False)
        name = select or (old.name if old else 'Default')
        if not self.select_preset_name(name):
            self.select_preset_name('Default')
        new = self.current_preset
        if (
            follow_changes
            and unmodified
            and new is not None
            and old is not None
            and new.name == old.name
            and new.settings.to_dict() != old.settings.to_dict()
            and self.panel.isEnabled()
        ):
            self.panel.set_settings(new.settings, emit=True)
        self._update_dirty()

    def select_preset_name(self, name: str) -> bool:
        """Select a setting by name (user settings first); returns ``False`` if not found."""
        for builtin in (False, True):
            for idx in range(self.preset_combo.count()):
                i = self.preset_combo.itemData(idx)
                if i is not None and self.presets[i].name == name and self.presets[i].builtin == builtin:
                    self.preset_combo.setCurrentIndex(idx)
                    self.current_preset = self.presets[i]
                    self._update_preset_actions()
                    self._update_dirty()
                    return True
        return False

    def apply_preset_name(self, name: str) -> None:
        """Select a setting and load its values into the adjustments."""
        self.reload_presets(select=name)
        if self.current_preset is not None and self.current_preset.name == name:
            self.panel.set_settings(self.current_preset.settings, emit=True)

    def _update_dirty(self) -> None:
        p = self.current_preset
        self.dirty_label.setVisible(p is not None and self.panel.settings().to_dict() != p.settings.to_dict())

    def _preset_chosen(self, idx: int) -> None:
        i = self.preset_combo.itemData(idx)
        if i is None:
            return
        self.current_preset = self.presets[i]
        self.panel.set_settings(self.current_preset.settings, emit=True)
        self._update_preset_actions()

    def _build_preset_menu(self) -> None:
        menu = QMenu(self)
        self.act_apply = QAction(icon('reset'), tr('Revert to Saved Values'), menu)
        self.act_save_new = QAction(icon('plus'), tr('Save as New Setting…'), menu)
        self.act_update = QAction(icon('save'), tr('Update Current Setting'), menu)
        self.act_manage = QAction(icon('layers'), tr('Manage Settings…'), menu)
        self.act_delete = QAction(icon('trash'), tr('Delete Setting'), menu)
        for a in (self.act_apply, self.act_save_new, self.act_update):
            menu.addAction(a)
        menu.addSeparator()
        menu.addAction(self.act_manage)
        menu.addAction(self.act_delete)
        self.act_manage.triggered.connect(self.manage_btn.click)
        self.preset_menu_btn.setMenu(menu)
        self.act_apply.triggered.connect(lambda: self._preset_chosen(self.preset_combo.currentIndex()))
        self.act_save_new.triggered.connect(self.save_new_preset)
        self.act_update.triggered.connect(self.update_current_preset)
        self.act_delete.triggered.connect(self.delete_current_preset)

    def _update_preset_actions(self) -> None:
        user = self.current_preset is not None and not self.current_preset.builtin
        self.act_update.setEnabled(user)
        self.act_delete.setEnabled(user)

    def save_new_preset(self) -> None:
        if not self.panel.isEnabled():
            return
        default = unique_name(tr('My Setting'), [p.name for p in self.presets])
        name, ok = QInputDialog.getText(
            self, tr('Save as New Setting'), tr('Setting name:'), QLineEdit.EchoMode.Normal, default
        )
        name = name.strip()
        if not ok or not name:
            return
        name = unique_name(name, [p.name for p in self.presets])
        try:
            save_user_preset(self.user_preset_dir, name, self.panel.settings())
        except OSError as exc:
            QMessageBox.warning(
                self, tr('Save Setting'), tr('Could not save the setting:\n{error}', error=exc)
            )
            return
        self.reload_presets(select=name)
        self.presets_edited.emit()

    def update_current_preset(self) -> None:
        p = self.current_preset
        if p is None or p.builtin:
            return
        try:
            save_user_preset(self.user_preset_dir, p.name, self.panel.settings(), path=p.path)
        except OSError as exc:
            QMessageBox.warning(
                self, tr('Update Setting'), tr('Could not save the setting:\n{error}', error=exc)
            )
            return
        self.reload_presets(select=p.name)
        self.presets_edited.emit()

    def delete_current_preset(self) -> None:
        p = self.current_preset
        if p is None or p.builtin:
            return
        if (
            QMessageBox.question(self, tr('Delete Setting'), tr('Delete the setting “{name}”?', name=p.name))
            != QMessageBox.StandardButton.Yes
        ):
            return
        delete_user_preset(p)
        self.current_preset = None
        self.reload_presets(select='Default')
        self.presets_edited.emit()

    # Keyboard: hold \ for the original, ↑/↓ for the photo list ---------------------------
    def eventFilter(self, obj: QObject, e: QEvent) -> bool:  # noqa: N802
        if e.type() not in (QEvent.Type.KeyPress, QEvent.Type.KeyRelease) or not self.isVisible():
            return False
        if not isinstance(obj, QWidget) or obj.window() is not self.window():
            return False
        ke: QKeyEvent = e  # type: ignore[assignment]
        if ke.key() == Qt.Key.Key_Backslash and not isinstance(obj, QLineEdit):
            if not ke.isAutoRepeat():
                self.preview.show_original(e.type() == QEvent.Type.KeyPress)
            return True
        if (
            e.type() == QEvent.Type.KeyPress
            and ke.key() in (Qt.Key.Key_Up, Qt.Key.Key_Down)
            and ke.modifiers() == Qt.KeyboardModifier.NoModifier
            and not isinstance(obj, (QAbstractSpinBox, QComboBox, QLineEdit))
            and obj is not self.file_list
            and self.file_list.count()
        ):
            row = self.file_list.currentRow() + (1 if ke.key() == Qt.Key.Key_Down else -1)
            self.file_list.setCurrentRow(max(0, min(self.file_list.count() - 1, row)))
            return True
        return False
