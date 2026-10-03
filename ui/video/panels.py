"""Right side of the video editor: Clips, Text, Audio and Export panels."""

from __future__ import annotations

from PySide6.QtCore import Signal
from PySide6.QtGui import QFont
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDoubleSpinBox,
    QFontComboBox,
    QFormLayout,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QPlainTextEdit,
    QProgressBar,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
    QWidget,
)

from core.i18n import tr
from core.video.export import FORMATS, RESOLUTIONS
from core.video.project import TextOverlay, VideoProject, fmt_time
from ui.photo.panels import ColorButton, ValueSlider, hint, section_title
from ui.widgets import ElidedLabel, button

RESOLUTION_LABELS = {'original': 'Keep original', '1080p': '1080p (Full HD)', '720p': '720p (HD)'}


def seconds_box(maximum: float = 36000.0) -> QDoubleSpinBox:
    b = QDoubleSpinBox()
    b.setDecimals(2)
    b.setRange(0.0, maximum)
    b.setSingleStep(0.1)
    b.setSuffix(' s')
    b.setKeyboardTracking(False)
    return b


class ClipPanel(QWidget):
    clip_selected = Signal(int)
    split_requested = Signal()
    delete_requested = Signal(int)
    move_requested = Signal(int, int)
    mark_in_requested = Signal()
    mark_out_requested = Signal()
    clear_marks_requested = Signal()
    keep_range_requested = Signal()
    delete_range_requested = Signal()
    trim_requested = Signal(int, float, float)
    add_clip_requested = Signal(float, float)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._loading = False
        self.list = QListWidget()
        self.list.setObjectName('ClipList')
        self.list.setMinimumHeight(120)
        self.split_btn = button(tr('Split at playhead'), 'scissors', tooltip=tr('Cut the clip in two (S)'))
        self.delete_btn = button(tr('Delete clip'), 'trash', tooltip=tr('Remove the selected clip (Del)'))
        self.up_btn = button(tr('Move earlier'), 'chevron-up')
        self.down_btn = button(tr('Move later'), 'chevron-down')
        self.in_btn = button(tr('Mark In'), 'flag', tooltip=tr('Start of the range at the playhead (I)'))
        self.out_btn = button(tr('Mark Out'), 'flag', tooltip=tr('End of the range at the playhead (O)'))
        self.keep_btn = button(tr('Keep In–Out'), 'check', tooltip=tr('Keep only the marked range'))
        self.cut_btn = button(
            tr('Delete In–Out'), 'scissors', tooltip=tr('Remove the marked range (e.g. a part in the middle)')
        )
        self.clear_btn = button(tr('Clear marks'))
        self.range_label = QLabel()
        self.range_label.setProperty('secondary', True)
        self.start_box = seconds_box()
        self.end_box = seconds_box()
        self.src_start = seconds_box()
        self.src_end = seconds_box()
        self.add_btn = button(
            tr('Add clip'),
            'plus',
            tooltip=tr('Append this part of the original video at the end of the timeline'),
        )

        g = QGridLayout()
        g.setSpacing(6)
        g.addWidget(self.split_btn, 0, 0)
        g.addWidget(self.delete_btn, 0, 1)
        g.addWidget(self.up_btn, 1, 0)
        g.addWidget(self.down_btn, 1, 1)
        marks = QGridLayout()
        marks.setSpacing(6)
        marks.addWidget(self.in_btn, 0, 0)
        marks.addWidget(self.out_btn, 0, 1)
        marks.addWidget(self.keep_btn, 1, 0)
        marks.addWidget(self.cut_btn, 1, 1)
        marks.addWidget(self.clear_btn, 2, 0, 1, 2)
        trim = QFormLayout()
        trim.addRow(tr('Start (original)'), self.start_box)
        trim.addRow(tr('End (original)'), self.end_box)
        add = QHBoxLayout()
        add.addWidget(self.src_start)
        add.addWidget(QLabel('→'))
        add.addWidget(self.src_end)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        lay.addWidget(section_title(tr('CLIPS (PLAYED IN THIS ORDER)')))
        lay.addWidget(self.list)
        lay.addLayout(g)
        lay.addWidget(section_title(tr('SELECTED CLIP')))
        lay.addLayout(trim)
        lay.addWidget(section_title(tr('IN / OUT RANGE')))
        lay.addWidget(self.range_label)
        lay.addLayout(marks)
        lay.addWidget(section_title(tr('ADD A PART OF THE ORIGINAL')))
        lay.addLayout(add)
        lay.addWidget(self.add_btn)
        lay.addWidget(
            hint(
                tr(
                    'Tip: split twice and delete the middle clip, or mark In / Out and use '
                    'Delete In–Out. Drag the edges of a clip on the timeline to trim it.'
                )
            )
        )
        lay.addStretch(1)

        self.list.currentRowChanged.connect(lambda r: None if self._loading else self.clip_selected.emit(r))
        self.split_btn.clicked.connect(self.split_requested.emit)
        self.delete_btn.clicked.connect(lambda: self.delete_requested.emit(self.list.currentRow()))
        self.up_btn.clicked.connect(lambda: self.move_requested.emit(self.list.currentRow(), -1))
        self.down_btn.clicked.connect(lambda: self.move_requested.emit(self.list.currentRow(), 1))
        self.in_btn.clicked.connect(self.mark_in_requested.emit)
        self.out_btn.clicked.connect(self.mark_out_requested.emit)
        self.keep_btn.clicked.connect(self.keep_range_requested.emit)
        self.cut_btn.clicked.connect(self.delete_range_requested.emit)
        self.clear_btn.clicked.connect(self.clear_marks_requested.emit)
        self.start_box.valueChanged.connect(lambda _v: self._trim())
        self.end_box.valueChanged.connect(lambda _v: self._trim())
        self.add_btn.clicked.connect(
            lambda: self.add_clip_requested.emit(self.src_start.value(), self.src_end.value())
        )

    def _trim(self) -> None:
        if not self._loading and self.list.currentRow() >= 0:
            self.trim_requested.emit(self.list.currentRow(), self.start_box.value(), self.end_box.value())

    def refresh(
        self, p: VideoProject | None, selected: int, mark_in: float | None, mark_out: float | None
    ) -> None:
        self._loading = True
        self.list.clear()
        if p is not None:
            for i, c in enumerate(p.clips):
                self.list.addItem(
                    QListWidgetItem(
                        f'#{i + 1}   {fmt_time(c.start)} → {fmt_time(c.end)}   ({c.duration:.1f} s)'
                    )
                )
            self.list.setCurrentRow(selected if 0 <= selected < len(p.clips) else -1)
            for b in (self.start_box, self.end_box, self.src_start, self.src_end):
                b.setMaximum(p.info.duration)
            if self.src_end.value() == 0:
                self.src_end.setValue(min(p.info.duration, 5.0))
        sel = p is not None and 0 <= selected < len(p.clips)
        if sel:
            self.start_box.setValue(p.clips[selected].start)
            self.end_box.setValue(p.clips[selected].end)
        for w in (self.start_box, self.end_box, self.up_btn, self.down_btn):
            w.setEnabled(sel)
        self.delete_btn.setEnabled(sel and len(p.clips) > 1)
        for w in (self.split_btn, self.in_btn, self.out_btn, self.add_btn, self.src_start, self.src_end):
            w.setEnabled(p is not None)
        has_range = p is not None and (mark_in is not None or mark_out is not None)
        for w in (self.keep_btn, self.cut_btn, self.clear_btn):
            w.setEnabled(has_range)
        if has_range:
            a = mark_in if mark_in is not None else 0.0
            b = mark_out if mark_out is not None else p.duration
            lo, hi = sorted((a, b))
            self.range_label.setText(
                tr('In {a} — Out {b} ({d:.1f} s)', a=fmt_time(lo), b=fmt_time(hi), d=hi - lo)
            )
        else:
            self.range_label.setText(tr('No range marked'))
        self._loading = False


class TextPanel(QWidget):
    text_selected = Signal(int)
    add_requested = Signal()
    delete_requested = Signal(int)
    field_changed = Signal(int, str, object)  # text id, field, value
    start_at_playhead = Signal(int)
    end_at_playhead = Signal(int)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._loading = False
        self.current_id = 0
        self.list = QListWidget()
        self.list.setMinimumHeight(90)
        self.add_btn = button(tr('Add text'), 'plus', 'Accent', tr('New text at the playhead (3 s)'))
        self.del_btn = button(tr('Delete text'), 'trash')
        self.content = QPlainTextEdit()
        self.content.setPlaceholderText(tr('Your text'))
        self.content.setFixedHeight(64)
        self.font_box = QFontComboBox()
        self.size_box = QSpinBox()
        self.size_box.setRange(6, 1000)
        self.size_box.setSuffix(' px')
        self.color_btn = ColorButton('#ffffff')
        self.bold = QPushButton('B')
        self.bold.setCheckable(True)
        self.bold.setToolTip(tr('Bold'))
        self.bold.setStyleSheet('font-weight: 700;')
        self.italic = QPushButton('I')
        self.italic.setCheckable(True)
        self.italic.setToolTip(tr('Italic'))
        self.italic.setStyleSheet('font-style: italic;')
        for b in (self.bold, self.italic):
            b.setObjectName('StyleToggle')
            b.setFixedWidth(34)
        self.outline_box = QDoubleSpinBox()
        self.outline_box.setRange(0, 100)
        self.outline_box.setDecimals(1)
        self.outline_box.setSuffix(' px')
        self.outline_color = ColorButton('#000000')
        self.opacity = ValueSlider(tr('Opacity'), 0, 100, 100, suffix=' %')
        self.start_box = seconds_box()
        self.end_box = seconds_box()
        self.start_here = button(tr('= playhead'), tooltip=tr('Start at the playhead'))
        self.end_here = button(tr('= playhead'), tooltip=tr('End at the playhead'))

        top = QHBoxLayout()
        top.addWidget(self.add_btn, 1)
        top.addWidget(self.del_btn)
        style = QHBoxLayout()
        style.addWidget(self.size_box, 1)
        style.addWidget(self.color_btn)
        style.addWidget(self.bold)
        style.addWidget(self.italic)
        outline = QHBoxLayout()
        outline.addWidget(self.outline_box, 1)
        outline.addWidget(self.outline_color)
        s_row = QHBoxLayout()
        s_row.addWidget(self.start_box, 1)
        s_row.addWidget(self.start_here)
        e_row = QHBoxLayout()
        e_row.addWidget(self.end_box, 1)
        e_row.addWidget(self.end_here)
        form = QFormLayout()
        form.addRow(tr('Font'), self.font_box)
        form.addRow(tr('Size'), style)
        form.addRow(tr('Outline'), outline)
        form.addRow(tr('Appears at'), s_row)
        form.addRow(tr('Disappears at'), e_row)
        self.editor = QWidget()
        el = QVBoxLayout(self.editor)
        el.setContentsMargins(0, 0, 0, 0)
        el.addWidget(self.content)
        el.addLayout(form)
        el.addWidget(self.opacity)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        lay.addWidget(section_title(tr('TEXTS')))
        lay.addLayout(top)
        lay.addWidget(self.list)
        lay.addWidget(self.editor)
        lay.addWidget(
            hint(
                tr(
                    'Drag a text on the preview to place it; drag its block on the timeline '
                    'to change when it appears.'
                )
            )
        )
        lay.addStretch(1)

        self.list.currentRowChanged.connect(self._row_changed)
        self.add_btn.clicked.connect(self.add_requested.emit)
        self.del_btn.clicked.connect(lambda: self.current_id and self.delete_requested.emit(self.current_id))
        self.content.textChanged.connect(lambda: self._emit('text', self.content.toPlainText()))
        self.font_box.currentFontChanged.connect(lambda f: self._emit('font_family', f.family()))
        self.size_box.valueChanged.connect(lambda v: self._emit('font_size', float(v)))
        self.color_btn.color_changed.connect(lambda c: self._emit('color', c))
        self.bold.toggled.connect(lambda v: self._emit('bold', v))
        self.italic.toggled.connect(lambda v: self._emit('italic', v))
        self.outline_box.valueChanged.connect(lambda v: self._emit('outline_width', float(v)))
        self.outline_color.color_changed.connect(lambda c: self._emit('outline_color', c))
        self.opacity.value_changed.connect(lambda v: self._emit('opacity', v / 100))
        self.start_box.valueChanged.connect(lambda v: self._emit('start', float(v)))
        self.end_box.valueChanged.connect(lambda v: self._emit('end', float(v)))
        self.start_here.clicked.connect(
            lambda: self.current_id and self.start_at_playhead.emit(self.current_id)
        )
        self.end_here.clicked.connect(lambda: self.current_id and self.end_at_playhead.emit(self.current_id))
        self._texts: list[int] = []

    def _emit(self, field: str, value: object) -> None:
        if not self._loading and self.current_id:
            self.field_changed.emit(self.current_id, field, value)

    def _row_changed(self, row: int) -> None:
        if self._loading:
            return
        self.text_selected.emit(self._texts[row] if 0 <= row < len(self._texts) else 0)

    def refresh(self, p: VideoProject | None, selected: int) -> None:
        self._loading = True
        self.list.clear()
        self._texts = [t.id for t in p.texts] if p else []
        for t in p.texts if p else []:
            self.list.addItem(
                f'{fmt_time(t.start)}–{fmt_time(t.end)}   {t.text.splitlines()[0] if t.text else "…"}'
            )
        t = p.text(selected) if p else None
        self.current_id = t.id if t else 0
        self.list.setCurrentRow(self._texts.index(t.id) if t else -1)
        self.editor.setEnabled(t is not None)
        self.del_btn.setEnabled(t is not None)
        self.add_btn.setEnabled(p is not None)
        if t is not None:
            self._load(t, p.duration)
        self._loading = False

    def _load(self, t: TextOverlay, duration: float) -> None:
        if self.content.toPlainText() != t.text:
            self.content.setPlainText(t.text)
        self.font_box.setCurrentFont(QFont(t.font_family))
        self.size_box.setValue(round(t.font_size))
        self.color_btn.set_color(t.color)
        self.bold.setChecked(t.bold)
        self.italic.setChecked(t.italic)
        self.outline_box.setValue(t.outline_width)
        self.outline_color.set_color(t.outline_color)
        self.opacity.set_value(t.opacity * 100)
        for b in (self.start_box, self.end_box):
            b.setMaximum(max(duration, t.end, 0.1))
        self.start_box.setValue(t.start)
        self.end_box.setValue(t.end)


class AudioPanel(QWidget):
    field_changed = Signal(str, object)  # "mute" | "volume" | "music.offset" | "music.volume" | ...
    choose_music = Signal()
    remove_music = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._loading = False
        self.mute = QCheckBox(tr('Mute the original sound'))
        self.volume = ValueSlider(tr('Original volume'), 0, 200, 100, suffix=' %')
        self.no_audio = hint(tr('This video has no sound track.'))
        self.music_label = ElidedLabel(tr('No music'))
        self.choose_btn = button(tr('Choose music…'), 'music', 'Accent', tr('MP3, WAV or M4A file'))
        self.remove_btn = button(tr('Remove'), 'trash')
        self.offset = seconds_box()
        self.music_volume = ValueSlider(tr('Music volume'), 0, 200, 100, suffix=' %')
        self.fade = QCheckBox(tr('Fade out at the end'))
        self.fade_s = seconds_box(30)
        self.fade_s.setValue(2.0)
        music_row = QHBoxLayout()
        music_row.addWidget(self.choose_btn, 1)
        music_row.addWidget(self.remove_btn)
        off = QFormLayout()
        off.addRow(tr('Start from (music)'), self.offset)
        fade = QHBoxLayout()
        fade.addWidget(self.fade, 1)
        fade.addWidget(self.fade_s)
        self.music_box = QWidget()
        ml = QVBoxLayout(self.music_box)
        ml.setContentsMargins(0, 0, 0, 0)
        ml.addLayout(off)
        ml.addWidget(self.music_volume)
        ml.addLayout(fade)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        lay.addWidget(section_title(tr('ORIGINAL SOUND')))
        lay.addWidget(self.mute)
        lay.addWidget(self.volume)
        lay.addWidget(self.no_audio)
        lay.addSpacing(6)
        lay.addWidget(section_title(tr('MUSIC')))
        lay.addWidget(self.music_label)
        lay.addLayout(music_row)
        lay.addWidget(self.music_box)
        lay.addWidget(
            hint(
                tr(
                    'Music longer than the video is cut at the end of the video. '
                    'The preview plays at most 100 % volume; the export uses the real values.'
                )
            )
        )
        lay.addStretch(1)
        self.mute.toggled.connect(lambda v: self._emit('mute', v))
        self.volume.value_changed.connect(lambda v: self._emit('volume', v / 100))
        self.offset.valueChanged.connect(lambda v: self._emit('music.offset', float(v)))
        self.music_volume.value_changed.connect(lambda v: self._emit('music.volume', v / 100))
        self.fade.toggled.connect(lambda v: self._emit('music.fade_out', v))
        self.fade_s.valueChanged.connect(lambda v: self._emit('music.fade_seconds', float(v)))
        self.choose_btn.clicked.connect(self.choose_music.emit)
        self.remove_btn.clicked.connect(self.remove_music.emit)

    def _emit(self, field: str, value: object) -> None:
        if not self._loading:
            self.field_changed.emit(field, value)

    def refresh(self, p: VideoProject | None) -> None:
        self._loading = True
        has_audio = bool(p and p.info.has_audio)
        self.mute.setEnabled(has_audio)
        self.volume.setEnabled(has_audio and not (p and p.mute))
        self.no_audio.setVisible(p is not None and not has_audio)
        self.choose_btn.setEnabled(p is not None)
        if p is not None:
            self.mute.setChecked(p.mute)
            self.volume.set_value(p.volume * 100)
        m = p.music if p else None
        self.music_box.setEnabled(m is not None)
        self.remove_btn.setEnabled(m is not None)
        if m is not None:
            self.music_label.setText(f'♪ {m.name}  ({fmt_time(m.duration)})')
            self.offset.setMaximum(max(0.0, m.duration - 0.1))
            self.offset.setValue(m.offset)
            self.music_volume.set_value(m.volume * 100)
            self.fade.setChecked(m.fade_out)
            self.fade_s.setValue(m.fade_seconds)
        else:
            self.music_label.setText(tr('No music'))
        self._loading = False


class ExportPanel(QWidget):
    export_requested = Signal()
    cancel_requested = Signal()
    browse_requested = Signal()
    open_folder_requested = Signal()
    format_changed = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.format_box = QComboBox()
        for key in FORMATS:
            self.format_box.addItem(key.upper(), key)
        self.res_box = QComboBox()
        for key in RESOLUTIONS:
            self.res_box.addItem(tr(RESOLUTION_LABELS[key]), key)
        self.path = QLineEdit()
        self.browse_btn = button(tr('Browse…'), 'folder')
        self.export_btn = button(tr('Export video'), 'download', 'RunButton')
        self.cancel_btn = button(tr('Cancel'), 'stop', 'Danger')
        self.cancel_btn.hide()
        self.progress = QProgressBar()
        self.progress.setRange(0, 1000)
        self.progress.setTextVisible(True)
        self.progress.hide()
        self.status = QLabel()
        self.status.setWordWrap(True)
        self.status.setProperty('secondary', True)
        self.open_btn = button(tr('Show in folder'), 'folder', 'Ghost')
        self.open_btn.hide()
        self.size_label = QLabel()
        self.size_label.setProperty('secondary', True)
        form = QFormLayout()
        form.addRow(tr('Format'), self.format_box)
        form.addRow(tr('Resolution'), self.res_box)
        path_row = QHBoxLayout()
        path_row.addWidget(self.path, 1)
        path_row.addWidget(self.browse_btn)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        lay.addWidget(section_title(tr('EXPORT')))
        lay.addLayout(form)
        lay.addWidget(self.size_label)
        lay.addWidget(QLabel(tr('Save as')))
        lay.addLayout(path_row)
        lay.addWidget(self.export_btn)
        lay.addWidget(self.progress)
        lay.addWidget(self.cancel_btn)
        lay.addWidget(self.status)
        lay.addWidget(self.open_btn)
        lay.addWidget(
            hint(
                tr(
                    'The original video is never overwritten. Default folder: "editor" next to '
                    'the video. The app stays usable while exporting.'
                )
            )
        )
        lay.addStretch(1)
        self.export_btn.clicked.connect(self.export_requested.emit)
        self.cancel_btn.clicked.connect(self.cancel_requested.emit)
        self.browse_btn.clicked.connect(self.browse_requested.emit)
        self.open_btn.clicked.connect(self.open_folder_requested.emit)
        self.format_box.currentIndexChanged.connect(lambda _i: self.format_changed.emit())
        self.res_box.currentIndexChanged.connect(lambda _i: self.format_changed.emit())

    def fmt(self) -> str:
        return self.format_box.currentData()

    def resolution(self) -> str:
        return self.res_box.currentData()

    def set_running(self, running: bool) -> None:
        self.export_btn.setVisible(not running)
        self.cancel_btn.setVisible(running)
        self.cancel_btn.setEnabled(True)
        self.progress.setVisible(running)
        for w in (self.format_box, self.res_box, self.path, self.browse_btn):
            w.setEnabled(not running)
        if running:
            self.progress.setValue(0)
            self.open_btn.hide()

    def set_progress(self, fraction: float) -> None:
        self.progress.setValue(int(fraction * 1000))
        self.progress.setFormat(f'{fraction * 100:.0f} %')


TAB_KEYS = (('clips', 'Clips'), ('text', 'Text'), ('audio', 'Audio'), ('export', 'Export'))


def tab_bar(on_select) -> tuple[QHBoxLayout, dict[str, QPushButton]]:  # noqa: ANN001
    row = QHBoxLayout()
    row.setSpacing(4)
    buttons: dict[str, QPushButton] = {}
    for key, label in TAB_KEYS:
        b = QPushButton(tr(label))
        b.setObjectName('Tab')
        b.setCheckable(True)
        b.setAutoExclusive(True)
        b.clicked.connect(lambda _=False, k=key: on_select(k))
        buttons[key] = b
        row.addWidget(b)
    return row, buttons
