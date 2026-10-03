"""Adjustment panel grouped like Camera Raw (spec section 8.7)."""

from __future__ import annotations

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QMouseEvent
from PySide6.QtWidgets import (
    QButtonGroup,
    QCheckBox,
    QComboBox,
    QDoubleSpinBox,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QSlider,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from core.i18n import language, tr
from core.settings import (
    IMAGE_SIZE_MODES,
    PERCENT_RANGE,
    PIXEL_RANGE,
    RESAMPLE_OPTIONS,
    SLIDERS,
    AdjustmentSettings,
    ImageSizeSettings,
    SliderSpec,
)
from ui.widgets import button

SR_TIP = (
    'Real-ESRGAN already sharpens and denoises: with Super Resolution on, '
    'a lower Sharpening › Amount is recommended.'
)


class CollapsibleSection(QWidget):
    """A titled group that can be collapsed/expanded."""

    def __init__(self, title: str, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Panel')
        self.header = QToolButton()
        self.header.setObjectName('SectionHeader')
        self.header.setText(title)
        self.header.setCheckable(True)
        self.header.setChecked(True)
        self.header.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
        self.header.setArrowType(Qt.ArrowType.DownArrow)
        self.header.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        self.body = QWidget()
        self.body.setObjectName('Panel')
        self.body_layout = QVBoxLayout(self.body)
        self.body_layout.setContentsMargins(4, 2, 4, 8)
        self.body_layout.setSpacing(2)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(0)
        lay.addWidget(self.header)
        lay.addWidget(self.body)
        self.header.toggled.connect(self._toggle)

    def _toggle(self, on: bool) -> None:
        self.body.setVisible(on)
        self.header.setArrowType(Qt.ArrowType.DownArrow if on else Qt.ArrowType.RightArrow)

    def add_title(self, text: str) -> None:
        lab = QLabel(text)
        lab.setObjectName('SectionTitle')
        self.body_layout.addWidget(lab)

    def add(self, w: QWidget) -> None:
        self.body_layout.addWidget(w)


class ResetSlider(QSlider):
    """Slider that emits ``reset`` on double-click (resets to default)."""

    reset = Signal()

    def mouseDoubleClickEvent(self, event: QMouseEvent) -> None:  # noqa: N802
        self.reset.emit()
        event.accept()


class SliderRow(QWidget):
    """Label + slider + numeric box, kept in sync both ways."""

    changed = Signal(str, float)

    def __init__(self, spec: SliderSpec, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Panel')
        self.spec = spec
        self._scale = 1.0 / spec.step
        self.label = QLabel(tr(spec.label))
        if language() != 'en':  # keep the Camera Raw name findable
            self.label.setToolTip(
                f'Camera Raw: {spec.group} › {spec.label}'
                if spec.group != spec.label
                else f'Camera Raw: {spec.label}'
            )
        self.label.setMinimumWidth(108)
        self.slider = ResetSlider(Qt.Orientation.Horizontal)
        self.slider.setRange(round(spec.minimum * self._scale), round(spec.maximum * self._scale))
        self.slider.setSingleStep(1)
        self.slider.setPageStep(10)
        self.slider.setToolTip(tr('Double-click to reset'))
        self.box = QDoubleSpinBox()
        self.box.setDecimals(spec.decimals)
        self.box.setRange(spec.minimum, spec.maximum)
        self.box.setSingleStep(spec.step)
        self.box.setAlignment(Qt.AlignmentFlag.AlignRight)
        self.box.setFixedWidth(62)
        self.box.setKeyboardTracking(False)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(6)
        lay.addWidget(self.label)
        lay.addWidget(self.slider, 1)
        lay.addWidget(self.box)
        self.slider.valueChanged.connect(self._from_slider)
        self.box.valueChanged.connect(self._from_box)
        self.slider.reset.connect(lambda: self.set_value(spec.default, emit=True))
        self.set_value(spec.default)

    def value(self) -> float:
        return (
            round(self.box.value(), self.spec.decimals)
            if self.spec.decimals
            else float(round(self.box.value()))
        )

    def set_value(self, v: float, emit: bool = False) -> None:
        for w in (self.slider, self.box):
            w.blockSignals(True)
        self.slider.setValue(round(v * self._scale))
        self.box.setValue(v)
        for w in (self.slider, self.box):
            w.blockSignals(False)
        self._mark()
        if emit:
            self.changed.emit(self.spec.key, self.value())

    def _mark(self) -> None:
        modified = abs(self.value() - self.spec.default) > 1e-9
        if self.label.property('modified') != modified:
            self.label.setProperty('modified', modified)
            self.label.style().unpolish(self.label)
            self.label.style().polish(self.label)

    def _from_slider(self, iv: int) -> None:
        self.box.blockSignals(True)
        self.box.setValue(iv / self._scale)
        self.box.blockSignals(False)
        self._mark()
        self.changed.emit(self.spec.key, self.value())

    def _from_box(self, v: float) -> None:
        self.slider.blockSignals(True)
        self.slider.setValue(round(v * self._scale))
        self.slider.blockSignals(False)
        self._mark()
        self.changed.emit(self.spec.key, self.value())


class SegmentedButtons(QWidget):
    """Off | 2x | 4x segmented control."""

    changed = Signal(str)

    def __init__(self, options: list[tuple[str, str]], parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Panel')
        self.group = QButtonGroup(self)
        self.group.setExclusive(True)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(0)
        self.buttons: dict[str, QPushButton] = {}
        for i, (key, text) in enumerate(options):
            b = QPushButton(text)
            b.setObjectName('Segment')
            b.setProperty('pos', 'first' if i == 0 else 'last' if i == len(options) - 1 else 'middle')
            b.setCheckable(True)
            self.group.addButton(b)
            self.buttons[key] = b
            lay.addWidget(b)
            b.clicked.connect(lambda _=False, k=key: self.changed.emit(k))
        lay.addStretch(1)

    def value(self) -> str:
        for k, b in self.buttons.items():
            if b.isChecked():
                return k
        return next(iter(self.buttons))

    def set_value(self, key: str) -> None:
        self.buttons.get(key, next(iter(self.buttons.values()))).setChecked(True)


class ImageSizeControls(QWidget):
    """Resize mode, value (% / px), resample method and Don't Enlarge (section 5.5)."""

    changed = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Panel')
        self.mode = QComboBox()
        for k, label in IMAGE_SIZE_MODES.items():
            self.mode.addItem(tr(label), k)
        self.value = QDoubleSpinBox()
        self.value.setDecimals(0)
        self.value.setKeyboardTracking(False)
        self.value.setFixedWidth(80)
        self.unit = QLabel('%')
        self.unit.setProperty('secondary', True)
        self.resample = QComboBox()
        for k, label in RESAMPLE_OPTIONS.items():
            self.resample.addItem(tr(label), k)
        for combo in (self.mode, self.resample):  # long item texts must not widen the panel
            combo.setSizeAdjustPolicy(QComboBox.SizeAdjustPolicy.AdjustToMinimumContentsLengthWithIcon)
            combo.setMinimumContentsLength(12)
        self.dont_enlarge = QCheckBox(tr("Don't Enlarge"))
        grid = QGridLayout(self)
        grid.setContentsMargins(0, 0, 0, 0)
        grid.setHorizontalSpacing(6)
        grid.addWidget(QLabel(tr('Resize:')), 0, 0)
        grid.addWidget(self.mode, 0, 1, 1, 2)
        grid.addWidget(QLabel(tr('Value:')), 1, 0)
        row = QHBoxLayout()
        row.addWidget(self.value)
        row.addWidget(self.unit)
        row.addStretch(1)
        grid.addLayout(row, 1, 1, 1, 2)
        grid.addWidget(QLabel(tr('Resample:')), 2, 0)
        grid.addWidget(self.resample, 2, 1, 1, 2)
        grid.addWidget(self.dont_enlarge, 3, 1, 1, 2)
        grid.setColumnStretch(2, 1)
        self._last_values = {'percent': 100.0, 'px': 2048.0}
        self.mode.currentIndexChanged.connect(self._mode_changed)
        self.value.valueChanged.connect(self._value_changed)
        self.resample.currentIndexChanged.connect(lambda _: self.changed.emit())
        self.dont_enlarge.toggled.connect(lambda _: self.changed.emit())
        self._apply_mode_ui()

    def _kind(self) -> str:
        return 'percent' if self.mode.currentData() in ('percent', 'off') else 'px'

    def _apply_mode_ui(self) -> None:
        mode = self.mode.currentData()
        lo, hi = PERCENT_RANGE if self._kind() == 'percent' else PIXEL_RANGE
        self.value.blockSignals(True)
        self.value.setRange(lo, hi)
        self.value.setValue(self._last_values[self._kind()])
        self.value.blockSignals(False)
        self.unit.setText('%' if self._kind() == 'percent' else 'px')
        on = mode != 'off'
        for w in (self.value, self.resample, self.dont_enlarge):
            w.setEnabled(on)

    def _mode_changed(self) -> None:
        self._apply_mode_ui()
        self.changed.emit()

    def _value_changed(self, v: float) -> None:
        self._last_values[self._kind()] = v
        self.changed.emit()

    def get(self) -> ImageSizeSettings:
        return ImageSizeSettings.from_dict(
            {
                'mode': self.mode.currentData(),
                'value': self.value.value(),
                'resample': self.resample.currentData(),
                'dont_enlarge': self.dont_enlarge.isChecked(),
            }
        )

    def set(self, s: ImageSizeSettings) -> None:
        for w in (self.mode, self.value, self.resample, self.dont_enlarge):
            w.blockSignals(True)
        self.mode.setCurrentIndex(max(0, self.mode.findData(s.mode)))
        kind = 'percent' if s.mode in ('percent', 'off') else 'px'
        if s.mode != 'off':
            self._last_values[kind] = float(s.value)
        self.resample.setCurrentIndex(max(0, self.resample.findData(s.resample)))
        self.dont_enlarge.setChecked(s.dont_enlarge)
        self._apply_mode_ui()
        for w in (self.mode, self.value, self.resample, self.dont_enlarge):
            w.blockSignals(False)


class AdjustmentPanel(QScrollArea):
    """All 17 adjustments. Emits ``settings_changed`` on every edit."""

    settings_changed = Signal()
    save_preset_requested = Signal()

    def __init__(self, parent: QWidget | None = None, show_save: bool = True) -> None:
        super().__init__(parent)
        self.setObjectName('Panel')
        self.setWidgetResizable(True)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.setMinimumWidth(330)
        inner = QWidget()
        inner.setObjectName('Panel')
        lay = QVBoxLayout(inner)
        lay.setContentsMargins(8, 6, 8, 8)
        lay.setSpacing(6)
        self.rows: dict[str, SliderRow] = {}

        sections: dict[str, CollapsibleSection] = {}
        groups_seen: set[tuple[str, str]] = set()
        for panel in ('Basic', 'Detail', 'Effects'):
            sec = CollapsibleSection(tr(panel).upper())
            sections[panel] = sec
            lay.addWidget(sec)
        for spec in SLIDERS:
            sec = sections[spec.panel]
            if (spec.panel, spec.group) not in groups_seen:
                groups_seen.add((spec.panel, spec.group))
                if spec.group != spec.label:  # "Noise Reduction" needs no extra title
                    sec.add_title(tr(spec.group))
            row = SliderRow(spec)
            row.changed.connect(self._changed)
            self.rows[spec.key] = row
            sec.add(row)

        enhance = CollapsibleSection(tr('ENHANCE'))
        enhance.add_title(tr('Super Resolution'))
        self.sr = SegmentedButtons([('off', tr('Off')), ('2x', '2x'), ('4x', '4x')])
        self.sr.changed.connect(lambda _: self._changed())
        enhance.add(self.sr)
        hint = QLabel(tr('AI upscaling · slower'))
        hint.setProperty('secondary', True)
        enhance.add(hint)
        self.sr_tip = QLabel(tr(SR_TIP))
        self.sr_tip.setWordWrap(True)
        self.sr_tip.setObjectName('Warning')
        self.sr_tip.hide()
        enhance.add(self.sr_tip)
        lay.addWidget(enhance)

        size = CollapsibleSection(tr('IMAGE SIZE'))
        self.image_size = ImageSizeControls()
        self.image_size.changed.connect(self._changed)
        size.add(self.image_size)
        lay.addWidget(size)

        buttons = QHBoxLayout()
        self.reset_btn = button(tr('Reset All'), 'reset', tooltip=tr('Reset all adjustments (Ctrl+R)'))
        self.save_btn = button(
            tr('Save as Setting…'),
            'save',
            tooltip=tr('Save the current adjustments as a new setting (Ctrl+S)'),
        )
        self.save_btn.setVisible(show_save)
        buttons.addWidget(self.reset_btn)
        buttons.addWidget(self.save_btn)
        lay.addLayout(buttons)
        lay.addStretch(1)
        self.setWidget(inner)
        self.reset_btn.clicked.connect(self.reset_all)
        self.save_btn.clicked.connect(self.save_preset_requested.emit)
        self.set_settings(AdjustmentSettings())

    def _changed(self, *_: object) -> None:
        s = self.settings()
        self.sr_tip.setVisible(s.super_resolution != 'off' and s.sharpening_amount > 0)
        self.settings_changed.emit()

    def settings(self) -> AdjustmentSettings:
        data: dict[str, object] = {k: row.value() for k, row in self.rows.items()}
        data['super_resolution'] = self.sr.value()
        s = AdjustmentSettings.from_dict(data)
        s.image_size = self.image_size.get()
        return s

    def set_settings(self, s: AdjustmentSettings, emit: bool = False) -> None:
        for k, row in self.rows.items():
            row.set_value(getattr(s, k))
        self.sr.set_value(s.super_resolution)
        self.image_size.set(s.image_size)
        self.sr_tip.setVisible(s.super_resolution != 'off' and s.sharpening_amount > 0)
        if emit:
            self.settings_changed.emit()

    def reset_all(self) -> None:
        self.set_settings(AdjustmentSettings(), emit=True)
