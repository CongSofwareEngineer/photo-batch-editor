"""Video editor page: preview + transport (top), timeline (bottom), panels (right).

Edits change :class:`core.video.project.VideoProject` in place after recording an undo
snapshot; the export runs FFmpeg on a worker thread (progress bar, Cancel).
"""

from __future__ import annotations

import logging
import shutil
import tempfile
import threading
from pathlib import Path

from PySide6.QtCore import QSize, Qt, QThread, QThreadPool, Signal
from PySide6.QtGui import QDragEnterEvent, QDropEvent, QKeySequence, QShortcut
from PySide6.QtWidgets import (
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QMessageBox,
    QScrollArea,
    QSplitter,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from core.i18n import tr, tr_msg
from core.io_utils import open_in_file_manager
from core.photo.history import History
from core.video.export import FORMATS, default_output_path, export_video, output_size
from core.video.ffmpeg import Cancelled, FFmpegError, find_ffmpeg, probe
from core.video.project import (
    MusicTrack,
    TextOverlay,
    VideoProject,
    add_clip,
    delete_clip,
    delete_range,
    fmt_time,
    keep_range,
    move_clip,
    split_at,
    trim_clip,
)
from ui.icons import icon
from ui.video.panels import AudioPanel, ClipPanel, ExportPanel, TextPanel, tab_bar
from ui.video.player import VideoPreview
from ui.video.text_render import export_overlays
from ui.video.timeline import TimelineWidget
from ui.widgets import ElidedLabel, button, tool_button
from ui.workers import Task

log = logging.getLogger(__name__)

VIDEO_FILTER = 'Videos (*.mp4 *.mov *.m4v *.avi *.mkv *.webm *.wmv *.mts *.3gp)'
AUDIO_FILTER = 'Audio (*.mp3 *.wav *.m4a *.aac *.ogg *.flac)'
VIDEO_EXTS = {'.mp4', '.mov', '.m4v', '.avi', '.mkv', '.webm', '.wmv', '.mts', '.3gp'}


class ExportWorker(QThread):
    progress = Signal(float)
    done = Signal(str)
    failed = Signal(str)
    cancelled = Signal()

    def __init__(
        self, project: VideoProject, overlays: list, out: Path, fmt: str, resolution: str, tmp_dir: str
    ) -> None:
        super().__init__()
        self.project, self.overlays, self.out = project, overlays, out
        self.fmt, self.resolution, self.tmp_dir = fmt, resolution, tmp_dir
        self._cancel = threading.Event()

    def cancel(self) -> None:
        self._cancel.set()

    def run(self) -> None:
        try:
            path = export_video(
                self.project,
                self.overlays,
                self.out,
                self.fmt,
                self.resolution,
                on_progress=self.progress.emit,
                cancel_event=self._cancel,
            )
        except Cancelled:
            self.cancelled.emit()
        except (FFmpegError, ValueError, OSError) as exc:
            self.failed.emit(str(exc))
        except Exception as exc:  # noqa: BLE001
            log.exception('Video export failed')
            self.failed.emit(str(exc))
        else:
            self.done.emit(str(path))
        finally:
            shutil.rmtree(self.tmp_dir, ignore_errors=True)


class VideoEditorView(QWidget):
    busy_changed = Signal(bool)  # an export is running

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Page')
        self.setAcceptDrops(True)
        self.project: VideoProject | None = None
        self.history: History[VideoProject] = History()
        self.modified = False
        self.last_dir = ''
        self.worker: ExportWorker | None = None
        self.pool = QThreadPool(self)
        self.pool.setMaxThreadCount(1)
        self.sel_clip = -1
        self.sel_text = 0
        self._last_sel = 'clip'
        self._last_export: Path | None = None
        self._out_user_set = False

        # top bar
        self.open_btn = button(tr('Open video'), 'film', 'Accent', tr('Open a video (Ctrl+O)'))
        self.undo_btn = tool_button('undo', tr('Undo (Ctrl+Z)'))
        self.redo_btn = tool_button('redo', tr('Redo (Ctrl+Y)'))
        self.title = ElidedLabel(tr('No video open — open a video or drop one here'))
        self.title.setProperty('secondary', True)
        self.info_label = QLabel()
        self.info_label.setProperty('secondary', True)
        top_box = QFrame()
        top_box.setObjectName('Panel')
        top = QHBoxLayout(top_box)
        top.setContentsMargins(10, 8, 10, 8)
        top.setSpacing(8)
        top.addWidget(self.open_btn)
        top.addSpacing(10)
        top.addWidget(self.undo_btn)
        top.addWidget(self.redo_btn)
        top.addSpacing(10)
        top.addWidget(self.title, 1)
        top.addWidget(self.info_label)

        # stage: preview + transport + timeline
        self.preview = VideoPreview()
        self.play_btn = button('', 'play', 'Accent', tr('Play / pause (Space)'))
        self.play_btn.setFixedWidth(52)
        self.time_label = QLabel('0:00.0 / 0:00.0')
        self.time_label.setMinimumWidth(150)
        self.mute_btn = tool_button('volume', tr('Mute / unmute the original sound'))
        self.mute_btn.setCheckable(True)
        self.split_btn = button(
            tr('Split'), 'scissors', tooltip=tr('Cut the clip in two at the playhead (S)')
        )
        self.in_btn = button(tr('In'), 'flag', tooltip=tr('Mark In (I)'))
        self.out_btn = button(tr('Out'), 'flag', tooltip=tr('Mark Out (O)'))
        self.fit_btn = button(tr('Fit timeline'), 'zoom-in', 'Ghost', tr('Show the whole timeline'))
        transport = QHBoxLayout()
        transport.setSpacing(8)
        transport.addWidget(self.play_btn)
        transport.addWidget(self.time_label)
        transport.addWidget(self.mute_btn)
        transport.addStretch(1)
        transport.addWidget(self.split_btn)
        transport.addWidget(self.in_btn)
        transport.addWidget(self.out_btn)
        transport.addWidget(self.fit_btn)
        self.timeline = TimelineWidget()
        preview_box = QFrame()
        preview_box.setObjectName('Panel')
        pl = QVBoxLayout(preview_box)
        pl.setContentsMargins(8, 8, 8, 8)
        pl.addWidget(self.preview, 1)
        pl.addLayout(transport)
        timeline_box = QFrame()
        timeline_box.setObjectName('Panel')
        tl = QVBoxLayout(timeline_box)
        tl.setContentsMargins(8, 8, 8, 8)
        tl.addWidget(self.timeline)
        self.stage = QSplitter(Qt.Orientation.Vertical)
        self.stage.addWidget(preview_box)
        self.stage.addWidget(timeline_box)
        self.stage.setStretchFactor(0, 1)
        self.stage.setSizes([560, 180])
        self.stage.setChildrenCollapsible(False)

        # right panels
        self.clip_panel = ClipPanel()
        self.text_panel = TextPanel()
        self.audio_panel = AudioPanel()
        self.export_panel = ExportPanel()
        self.panel_stack = QStackedWidget()
        self.panels = {
            'clips': self.clip_panel,
            'text': self.text_panel,
            'audio': self.audio_panel,
            'export': self.export_panel,
        }
        for w in self.panels.values():
            self.panel_stack.addWidget(w)
        tabs, self.tab_buttons = tab_bar(self.show_panel)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.Shape.NoFrame)
        scroll.setWidget(self.panel_stack)
        right = QFrame()
        right.setObjectName('Panel')
        right.setFixedWidth(340)
        rl = QVBoxLayout(right)
        rl.setContentsMargins(12, 10, 12, 10)
        rl.addLayout(tabs)
        rl.addWidget(scroll, 1)

        middle = QHBoxLayout()
        middle.setSpacing(10)
        middle.addWidget(self.stage, 1)
        middle.addWidget(right)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(16, 14, 16, 14)
        lay.setSpacing(10)
        lay.addWidget(top_box)
        lay.addLayout(middle, 1)

        # wiring
        self.open_btn.clicked.connect(self.open_dialog)
        self.undo_btn.clicked.connect(self.undo)
        self.redo_btn.clicked.connect(self.redo)
        self.play_btn.clicked.connect(self.preview.toggle)
        self.mute_btn.toggled.connect(lambda v: self.set_field('mute', v))
        self.split_btn.clicked.connect(self.split)
        self.in_btn.clicked.connect(self.mark_in)
        self.out_btn.clicked.connect(self.mark_out)
        self.fit_btn.clicked.connect(self.timeline.refit)
        self.preview.position_changed.connect(self._position_changed)
        self.preview.playing_changed.connect(self._playing_changed)
        self.preview.text_selected.connect(lambda tid: self.select_text(tid, from_preview=True))
        self.preview.text_drag_started.connect(lambda: self.begin('Move text'))
        self.preview.text_moved.connect(self._after_text_move)
        self.preview.media_error.connect(
            lambda msg: self.title.setText(tr('Preview error: {error}', error=msg))
        )
        self.timeline.seek_requested.connect(self.seek)
        self.timeline.clip_selected.connect(self.select_clip)
        self.timeline.text_selected.connect(self.select_text)
        self.timeline.music_selected.connect(lambda: self.show_panel('audio'))
        self.timeline.edit_started.connect(self.begin)
        self.timeline.edited.connect(lambda: self.after_edit(refit=False))
        c = self.clip_panel
        c.clip_selected.connect(self.select_clip)
        c.split_requested.connect(self.split)
        c.delete_requested.connect(self.delete_clip)
        c.move_requested.connect(self.move_clip)
        c.mark_in_requested.connect(self.mark_in)
        c.mark_out_requested.connect(self.mark_out)
        c.clear_marks_requested.connect(self.clear_marks)
        c.keep_range_requested.connect(self.keep_range)
        c.delete_range_requested.connect(self.delete_range)
        c.trim_requested.connect(self.trim)
        c.add_clip_requested.connect(self.add_clip)
        t = self.text_panel
        t.text_selected.connect(self.select_text)
        t.add_requested.connect(self.add_text)
        t.delete_requested.connect(self.delete_text)
        t.field_changed.connect(self.set_text_field)
        t.start_at_playhead.connect(lambda tid: self.set_text_field(tid, 'start', self.preview.t))
        t.end_at_playhead.connect(lambda tid: self.set_text_field(tid, 'end', self.preview.t))
        a = self.audio_panel
        a.field_changed.connect(self.set_field)
        a.choose_music.connect(self.choose_music)
        a.remove_music.connect(self.remove_music)
        e = self.export_panel
        e.export_requested.connect(self.start_export)
        e.cancel_requested.connect(self.cancel_export)
        e.browse_requested.connect(self._browse_output)
        e.open_folder_requested.connect(
            lambda: self._last_export and open_in_file_manager(self._last_export, select=True)
        )
        e.format_changed.connect(self._format_changed)
        e.path.textEdited.connect(lambda _t: setattr(self, '_out_user_set', True))

        for keys, slot in (
            ('Ctrl+O', self.open_dialog),
            ('Ctrl+Z', self.undo),
            ('Ctrl+Y', self.redo),
            ('Ctrl+Shift+Z', self.redo),
            ('Space', self.preview.toggle),
        ):
            sc = QShortcut(QKeySequence(keys), self)
            sc.setContext(Qt.ShortcutContext.WidgetWithChildrenShortcut)
            sc.activated.connect(slot)
        for keys, slot in (
            ('S', self.split),
            ('I', self.mark_in),
            ('O', self.mark_out),
            ('Delete', self.delete_selected),
            ('Left', lambda: self.step(-1)),
            ('Right', lambda: self.step(1)),
            ('Shift+Left', lambda: self.step(-1, big=True)),
            ('Shift+Right', lambda: self.step(1, big=True)),
            ('Home', lambda: self.seek(0.0)),
            ('End', lambda: self.seek(self.project.duration if self.project else 0.0)),
        ):
            sc = QShortcut(QKeySequence(keys), self.stage)
            sc.setContext(Qt.ShortcutContext.WidgetWithChildrenShortcut)
            sc.activated.connect(slot)

        self.show_panel('clips')
        self._set_project(None)

    # project ------------------------------------------------------------------------------------
    def _set_project(self, p: VideoProject | None, keep_selection: bool = False) -> None:
        self.project = p
        if not keep_selection:
            self.sel_clip, self.sel_text = (0 if p else -1), 0
            self.timeline.mark_in = self.timeline.mark_out = None
        self.preview.selected_text = self.sel_text
        self.timeline.sel_clip, self.timeline.sel_text = self.sel_clip, self.sel_text
        self.preview.set_project(p)
        self.timeline.set_project(p)
        has = p is not None
        for w in (
            self.play_btn,
            self.mute_btn,
            self.split_btn,
            self.in_btn,
            self.out_btn,
            self.fit_btn,
            self.export_panel.export_btn,
        ):
            w.setEnabled(has)
        if p is not None and not self._out_user_set:
            self.export_panel.path.setText(str(default_output_path(Path(p.source), self.export_panel.fmt())))
        self.refresh()

    def take_state(self) -> tuple[VideoProject | None, History, bool]:
        """Hand the project to a rebuilt window (language switch)."""
        state = (self.project, self.history, self.modified)
        self.preview.stop_media()
        self.project = None
        return state

    def adopt_state(self, state: tuple[VideoProject | None, History, bool], last_dir: str = '') -> None:
        project, history, modified = state
        self.last_dir = last_dir or self.last_dir
        if project is not None:
            self.history = history
            self._set_project(project)
            self.modified = modified
            self.refresh()

    def refresh(self) -> None:
        p = self.project
        tl = self.timeline
        self.clip_panel.refresh(p, self.sel_clip, tl.mark_in, tl.mark_out)
        self.text_panel.refresh(p, self.sel_text)
        self.audio_panel.refresh(p)
        self.mute_btn.blockSignals(True)
        self.mute_btn.setChecked(bool(p and p.mute))
        self.mute_btn.setIcon(icon('volume-x' if p and p.mute else 'volume'))
        self.mute_btn.setEnabled(bool(p and p.info.has_audio))
        self.mute_btn.blockSignals(False)
        self.undo_btn.setEnabled(self.history.can_undo() and p is not None)
        self.redo_btn.setEnabled(self.history.can_redo() and p is not None)
        if p is None:
            self.title.setText(tr('No video open — open a video or drop one here'))
            self.info_label.setText('')
            self.export_panel.size_label.setText('')
        else:
            self.title.setText(('● ' if self.modified else '') + Path(p.source).name)
            fps = f' · {p.info.fps:g} fps' if p.info.fps else ''
            self.info_label.setText(f'{p.info.width} × {p.info.height}{fps} · {fmt_time(p.info.duration)}')
            w, h = output_size(p.info.width, p.info.height, self.export_panel.resolution())
            self.export_panel.size_label.setText(
                tr('Output: {w} × {h} · {d}', w=w, h=h, d=fmt_time(p.duration))
            )
        self._update_time()

    def begin(self, label: str, key: str | None = None) -> None:
        """Record the state before a change (undo)."""
        if self.project is not None:
            self.history.push(label, self.project.clone(), key)

    def apply(self, label: str, fn) -> bool:  # noqa: ANN001
        """Run ``fn(project)``; record an undo step only when it changed something."""
        p = self.project
        if p is None:
            return False
        before = p.clone()
        if not fn(p):
            return False
        self.history.push(label, before)
        return True

    def after_edit(self, refit: bool = True) -> None:
        self.modified = True
        p = self.project
        if p is not None:
            self.sel_clip = min(self.sel_clip, len(p.clips) - 1)
            self.timeline.sel_clip = self.sel_clip
            self.preview.apply_audio()
            self.preview.update_music_source()
            if self.preview.t > p.duration:
                self.preview.seek(p.duration)
        if refit:
            self.timeline.refit()
        self.timeline.update()
        self.preview.update()
        self.refresh()

    def undo(self) -> None:
        if self.project is None:
            return
        state = self.history.undo(self.project.clone())
        if state is not None:
            self._restore(state)

    def redo(self) -> None:
        if self.project is None:
            return
        state = self.history.redo(self.project.clone())
        if state is not None:
            self._restore(state)

    def _restore(self, state: VideoProject) -> None:
        self.sel_clip = min(self.sel_clip, len(state.clips) - 1)
        if state.text(self.sel_text) is None:
            self.sel_text = 0
        self._set_project(state, keep_selection=True)
        self.after_edit()

    def is_modified(self) -> bool:
        return self.project is not None and self.modified

    def is_busy(self) -> bool:
        return self.worker is not None

    def confirm_discard(self) -> bool:
        if not self.is_modified():
            return True
        return (
            QMessageBox.question(
                self, tr('Video editor'), tr('The video edits have not been exported. Discard them?')
            )
            == QMessageBox.StandardButton.Yes
        )

    # open ---------------------------------------------------------------------------------------
    def open_dialog(self) -> None:
        if self.is_busy() or not self.confirm_discard():
            return
        path, _ = QFileDialog.getOpenFileName(self, tr('Open video'), self.last_dir, tr(VIDEO_FILTER))
        if path:
            self.open_path(Path(path), confirm=False)

    def open_path(self, path: Path, confirm: bool = True) -> None:
        if self.is_busy() or (confirm and not self.confirm_discard()):
            return
        try:
            find_ffmpeg()
        except FFmpegError as exc:
            QMessageBox.critical(self, tr('FFmpeg'), tr_msg(str(exc)))
            return
        self.last_dir = str(path.parent)
        self.title.setText(tr('Opening {name}…', name=path.name))
        Task.submit(
            self.pool,
            lambda: probe(path),
            lambda info: self._opened(path, info),
            lambda err: self._open_failed(path, err),
        )

    def _opened(self, path: Path, info) -> None:  # noqa: ANN001
        if not info.has_video:
            self._open_failed(path, tr('This file has no video track.'))
            return
        self.preview.pause()
        self.history.clear()
        self.modified = False
        self._out_user_set = False
        self.preview.t = 0.0
        self._set_project(VideoProject.new(path, info))

    def _open_failed(self, path: Path, err: str) -> None:
        self.refresh()
        QMessageBox.warning(
            self,
            tr('Open video'),
            tr('Cannot open the video:\n{path}\n\n{error}', path=path, error=tr_msg(err)),
        )

    # playback -----------------------------------------------------------------------------------
    def seek(self, t: float) -> None:
        self.preview.seek(t)

    def step(self, direction: int, big: bool = False) -> None:
        p = self.project
        if p is None:
            return
        self.preview.pause()
        dt = 1.0 if big else 1.0 / (p.info.fps or 30.0)
        self.seek(self.preview.t + direction * dt)

    def _position_changed(self, t: float) -> None:
        self.timeline.set_position(t)
        self._update_time()

    def _update_time(self) -> None:
        p = self.project
        self.time_label.setText(f'{fmt_time(self.preview.t)} / {fmt_time(p.duration if p else 0.0)}')

    def _playing_changed(self, playing: bool) -> None:
        self.play_btn.setIcon(icon('pause' if playing else 'play', '#ffffff'))

    # selection ----------------------------------------------------------------------------------
    def show_panel(self, key: str) -> None:
        self.panel_stack.setCurrentWidget(self.panels[key])
        self.tab_buttons[key].setChecked(True)

    def select_clip(self, index: int) -> None:
        if self.project is None or not 0 <= index < len(self.project.clips):
            return
        self.sel_clip = index
        self._last_sel = 'clip'
        self.timeline.sel_clip = index
        self.timeline.update()
        self.clip_panel.refresh(self.project, index, self.timeline.mark_in, self.timeline.mark_out)
        self.show_panel('clips')

    def select_text(self, text_id: int, from_preview: bool = False) -> None:
        self.sel_text = text_id
        self.preview.selected_text = text_id
        self.timeline.sel_text = text_id
        if text_id:
            self._last_sel = 'text'
            self.show_panel('text')
        self.text_panel.refresh(self.project, text_id)
        self.timeline.update()
        self.preview.update()

    # clip edits --------------------------------------------------------------------------------
    def split(self) -> None:
        t = self.preview.t
        if self.apply('Split clip', lambda p: split_at(p, t)):
            self.sel_clip, _ = self.project.locate(t)
            self.after_edit()

    def delete_clip(self, index: int) -> None:
        if self.apply('Delete clip', lambda p: delete_clip(p, index)):
            self.sel_clip = min(index, len(self.project.clips) - 1)
            self.after_edit()

    def delete_selected(self) -> None:
        if self._last_sel == 'text' and self.sel_text:
            self.delete_text(self.sel_text)
        else:
            self.delete_clip(self.sel_clip)

    def move_clip(self, index: int, delta: int) -> None:
        if self.apply('Reorder clips', lambda p: move_clip(p, index, delta)):
            self.sel_clip = index + delta
            self.after_edit()

    def trim(self, index: int, start: float, end: float) -> None:
        p = self.project
        if p is None or not 0 <= index < len(p.clips):
            return
        self.begin('Trim clip', key=f'trim:{index}')
        trim_clip(p, index, start, end)
        self.after_edit()

    def add_clip(self, start: float, end: float) -> None:
        if self.apply('Add clip', lambda p: add_clip(p, start, end)):
            self.sel_clip = len(self.project.clips) - 1
            self.after_edit()

    def mark_in(self) -> None:
        if self.project is not None:
            self.timeline.mark_in = self.preview.t
            self.after_marks()

    def mark_out(self) -> None:
        if self.project is not None:
            self.timeline.mark_out = self.preview.t
            self.after_marks()

    def clear_marks(self) -> None:
        self.timeline.mark_in = self.timeline.mark_out = None
        self.after_marks()

    def after_marks(self) -> None:
        self.timeline.update()
        self.clip_panel.refresh(self.project, self.sel_clip, self.timeline.mark_in, self.timeline.mark_out)
        self.show_panel('clips')

    def _range(self) -> tuple[float, float] | None:
        p = self.project
        tl = self.timeline
        if p is None or (tl.mark_in is None and tl.mark_out is None):
            return None
        return (
            tl.mark_in if tl.mark_in is not None else 0.0,
            tl.mark_out if tl.mark_out is not None else p.duration,
        )

    def keep_range(self) -> None:
        r = self._range()
        if r is None:
            return
        if self.apply('Keep In–Out', lambda p: keep_range(p, *r)):
            self.clear_marks()
            self.sel_clip = 0
            self.preview.seek(0.0)
            self.after_edit()

    def delete_range(self) -> None:
        r = self._range()
        if r is None:
            return
        if self.apply('Delete In–Out', lambda p: delete_range(p, *r)):
            self.clear_marks()
            self.preview.seek(min(r))
            self.after_edit()
        else:
            QMessageBox.information(self, tr('Video editor'), tr('The whole video would be removed.'))

    # texts --------------------------------------------------------------------------------------
    def add_text(self) -> None:
        p = self.project
        if p is None:
            return
        self.begin('Add text')
        start = min(self.preview.t, max(0.0, p.duration - 0.5))
        t = TextOverlay(
            text=tr('Your text'),
            font_size=max(16.0, round(p.info.height / 12)),
            outline_width=max(1.0, round(p.info.height / 300)),
            start=start,
            end=min(p.duration, start + 3.0),
        )
        p.texts.append(t)
        self.after_edit(refit=False)
        self.select_text(t.id)

    def delete_text(self, text_id: int) -> None:
        p = self.project
        t = p.text(text_id) if p else None
        if t is None:
            return
        self.begin('Delete text')
        p.texts.remove(t)
        self.select_text(0)
        self.after_edit(refit=False)

    def set_text_field(self, text_id: int, field: str, value: object) -> None:
        p = self.project
        t = p.text(text_id) if p else None
        if t is None or getattr(t, field) == value:
            return
        self.begin('Edit text', key=f'text:{text_id}:{field}')
        setattr(t, field, value)
        if field == 'start' and t.end <= t.start:
            t.end = t.start + 0.5
        if field == 'end' and t.end <= t.start:
            t.start = max(0.0, t.end - 0.5)
        self.modified = True
        self.timeline.update()
        self.preview.update()
        if field in ('start', 'end', 'text'):
            self.text_panel.refresh(p, text_id)
        self.refresh_history()

    def _after_text_move(self) -> None:
        self.modified = True
        self.refresh_history()

    def refresh_history(self) -> None:
        self.undo_btn.setEnabled(self.history.can_undo())
        self.redo_btn.setEnabled(self.history.can_redo())
        p = self.project
        if p is not None:
            self.title.setText(('● ' if self.modified else '') + Path(p.source).name)

    # audio --------------------------------------------------------------------------------------
    def set_field(self, field: str, value: object) -> None:
        p = self.project
        if p is None:
            return
        target, name = (p.music, field.split('.', 1)[1]) if field.startswith('music.') else (p, field)
        if target is None or getattr(target, name) == value:
            return
        self.begin({'mute': 'Mute'}.get(field, 'Audio'), key=f'audio:{field}')
        setattr(target, name, value)
        self.after_edit(refit=False)

    def choose_music(self) -> None:
        if self.project is None:
            return
        path, _ = QFileDialog.getOpenFileName(self, tr('Choose music'), self.last_dir, tr(AUDIO_FILTER))
        if not path:
            return
        Task.submit(
            self.pool,
            lambda: probe(Path(path)),
            lambda info: self._music_probed(path, info),
            lambda err: QMessageBox.warning(
                self,
                tr('Choose music'),
                tr('Cannot open the music file:\n{path}\n\n{error}', path=path, error=tr_msg(err)),
            ),
        )

    def _music_probed(self, path: str, info) -> None:  # noqa: ANN001
        if self.project is None:
            return
        if not info.has_audio:
            QMessageBox.warning(self, tr('Choose music'), tr('This file has no sound.'))
            return
        self.begin('Add music')
        self.project.music = MusicTrack(path, info.duration)
        self.after_edit(refit=False)
        self.show_panel('audio')

    def remove_music(self) -> None:
        if self.project is None or self.project.music is None:
            return
        self.begin('Remove music')
        self.project.music = None
        self.after_edit(refit=False)

    # export -------------------------------------------------------------------------------------
    def _format_changed(self) -> None:
        if self.project is None:
            return
        cur = self.export_panel.path.text().strip()
        ext = FORMATS[self.export_panel.fmt()]
        if cur:
            self.export_panel.path.setText(str(Path(cur).with_suffix(ext)))
        self.refresh()

    def _browse_output(self) -> None:
        flt = f'{self.export_panel.fmt().upper()} (*{FORMATS[self.export_panel.fmt()]})'
        path, _ = QFileDialog.getSaveFileName(self, tr('Export video'), self.export_panel.path.text(), flt)
        if path:
            self.export_panel.path.setText(path)
            self._out_user_set = True

    def start_export(self) -> None:
        p = self.project
        if p is None or self.worker is not None:
            return
        out = Path(self.export_panel.path.text().strip())
        if not self.export_panel.path.text().strip():
            return
        fmt = self.export_panel.fmt()
        if out.suffix.lower() != FORMATS[fmt]:
            out = out.with_suffix(FORMATS[fmt])
            self.export_panel.path.setText(str(out))
        if out.resolve() == Path(p.source).resolve():
            QMessageBox.warning(
                self,
                tr('Export video'),
                tr('This would overwrite the original video. Choose another name or folder.'),
            )
            return
        if (
            out.exists()
            and QMessageBox.question(
                self, tr('Export video'), tr('{name} already exists. Replace it?', name=out.name)
            )
            != QMessageBox.StandardButton.Yes
        ):
            return
        try:
            find_ffmpeg()
        except FFmpegError as exc:
            QMessageBox.critical(self, tr('FFmpeg'), tr_msg(str(exc)))
            return
        self.preview.pause()
        res = self.export_panel.resolution()
        w, h = output_size(p.info.width, p.info.height, res)
        tmp_dir = tempfile.mkdtemp(prefix='pbe_video_')
        snapshot = p.clone()
        overlays = export_overlays(snapshot, w, h, Path(tmp_dir))
        self.worker = ExportWorker(snapshot, overlays, out, fmt, res, tmp_dir)
        self.worker.progress.connect(self.export_panel.set_progress)
        self.worker.done.connect(self._export_done)
        self.worker.failed.connect(self._export_failed)
        self.worker.cancelled.connect(self._export_cancelled)
        self.export_panel.set_running(True)
        self.export_panel.status.setText(tr('Exporting {name}…', name=out.name))
        self.open_btn.setEnabled(False)
        self.busy_changed.emit(True)
        self.worker.start()

    def cancel_export(self) -> None:
        if self.worker is not None:
            self.worker.cancel()
            self.export_panel.cancel_btn.setEnabled(False)
            self.export_panel.status.setText(tr('Cancelling…'))

    def _end_export(self) -> None:
        if self.worker is not None:
            self.worker.wait(5000)
            self.worker.deleteLater()
        self.worker = None
        self.export_panel.set_running(False)
        self.open_btn.setEnabled(True)
        self.busy_changed.emit(False)

    def _export_done(self, path: str) -> None:
        self._end_export()
        self._last_export = Path(path)
        self.modified = False
        self.refresh()
        self.export_panel.status.setText(tr('Exported: {path}', path=path))
        self.export_panel.open_btn.show()

    def _export_failed(self, message: str) -> None:
        self._end_export()
        self.export_panel.status.setText(tr('Export failed.'))
        QMessageBox.warning(
            self, tr('Export video'), tr('The export failed:\n\n{error}', error=tr_msg(message))
        )

    def _export_cancelled(self) -> None:
        self._end_export()
        self.export_panel.status.setText(tr('Export cancelled. No file was written.'))

    def shutdown(self) -> None:
        """Window closing: stop the export and the players."""
        if self.worker is not None:
            self.worker.cancel()
            self.worker.wait(10000)
        self.preview.stop_media()

    # drag & drop ---------------------------------------------------------------------------------
    def dragEnterEvent(self, e: QDragEnterEvent) -> None:  # noqa: N802
        if e.mimeData().hasUrls():
            e.acceptProposedAction()

    def dropEvent(self, e: QDropEvent) -> None:  # noqa: N802
        paths = [Path(u.toLocalFile()) for u in e.mimeData().urls() if u.isLocalFile()]
        if paths and paths[0].suffix.lower() in VIDEO_EXTS:
            self.open_path(paths[0])
            e.acceptProposedAction()

    def minimumSizeHint(self) -> QSize:  # noqa: N802
        return QSize(900, 560)
