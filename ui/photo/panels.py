"""Right side of the photo editor: the Layers panel and the properties of the active tool."""

from __future__ import annotations

from collections.abc import Callable

from PySide6.QtCore import QSize, Qt, Signal
from PySide6.QtGui import QColor, QFont, QIcon, QPixmap
from PySide6.QtWidgets import (
    QButtonGroup,
    QCheckBox,
    QColorDialog,
    QComboBox,
    QDoubleSpinBox,
    QFontComboBox,
    QFormLayout,
    QFrame,
    QHBoxLayout,
    QInputDialog,
    QLabel,
    QListWidget,
    QListWidgetItem,
    QPlainTextEdit,
    QPushButton,
    QSlider,
    QSpinBox,
    QVBoxLayout,
    QWidget,
)

from core.i18n import tr
from core.photo.effects import ADJUST_SLIDERS, PhotoAdjust
from core.photo.geometry import CROP_RATIOS
from core.system import DEFAULT_FONT_FAMILY
from ui.icons import icon
from ui.photo.document import BlurLayer, Document, ImageLayer, Layer, TextLayer
from ui.widgets import button, tool_button

ID_ROLE = Qt.ItemDataRole.UserRole + 1
KIND_ICON = {'text': 'pencil', 'image': 'image', 'blur': 'sparkles'}


def section_title(text: str) -> QLabel:
    lab = QLabel(text)
    lab.setObjectName('SectionTitle')
    return lab


def hint(text: str) -> QLabel:
    lab = QLabel(text)
    lab.setWordWrap(True)
    lab.setProperty('secondary', True)
    return lab


class ValueSlider(QWidget):
    """Label + slider + number box (integer steps, optional decimals)."""

    value_changed = Signal(float)

    def __init__(
        self,
        label: str,
        minimum: float,
        maximum: float,
        value: float = 0.0,
        decimals: int = 0,
        suffix: str = '',
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self._k = 10**decimals
        self.default = value
        self.label = QLabel(label)
        self.label.setMinimumWidth(92)
        self.slider = QSlider(Qt.Orientation.Horizontal)
        self.slider.setRange(round(minimum * self._k), round(maximum * self._k))
        self.slider.setToolTip(tr('Double-click to reset'))
        self.box = QDoubleSpinBox()
        self.box.setDecimals(decimals)
        self.box.setRange(minimum, maximum)
        self.box.setSuffix(suffix)
        self.box.setFixedWidth(70)
        self.box.setAlignment(Qt.AlignmentFlag.AlignRight)
        self.box.setKeyboardTracking(False)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(6)
        lay.addWidget(self.label)
        lay.addWidget(self.slider, 1)
        lay.addWidget(self.box)
        self.slider.valueChanged.connect(lambda v: self._set(v / self._k, from_slider=True))
        self.box.valueChanged.connect(lambda v: self._set(v, from_slider=False))
        self.slider.mouseDoubleClickEvent = lambda _e: self._set(self.default, from_slider=False)  # type: ignore
        self.set_value(value)

    def _set(self, v: float, from_slider: bool) -> None:
        self.set_value(v, keep_slider=from_slider)
        self.value_changed.emit(self.value())

    def value(self) -> float:
        return round(self.box.value(), 6)

    def set_value(self, v: float, keep_slider: bool = False) -> None:
        for w in (self.slider, self.box):
            w.blockSignals(True)
        self.box.setValue(v)
        if not keep_slider:
            self.slider.setValue(round(v * self._k))
        for w in (self.slider, self.box):
            w.blockSignals(False)


class ColorButton(QPushButton):
    color_changed = Signal(str)

    def __init__(self, color: str = '#ffffff', parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setFixedSize(46, 26)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self._color = color
        self.set_color(color)
        self.clicked.connect(self._pick)

    def color(self) -> str:
        return self._color

    def set_color(self, color: str) -> None:
        self._color = color
        pm = QPixmap(28, 14)
        pm.fill(QColor(color))
        self.setIcon(QIcon(pm))
        self.setIconSize(QSize(28, 14))
        self.setToolTip(color)

    def _pick(self) -> None:
        c = QColorDialog.getColor(
            QColor(self._color), self, tr('Choose a colour'), QColorDialog.ColorDialogOption.ShowAlphaChannel
        )
        if c.isValid():
            name = c.name(QColor.NameFormat.HexArgb if c.alpha() < 255 else QColor.NameFormat.HexRgb)
            self.set_color(name)
            self.color_changed.emit(name)


class PropsPanel(QWidget):
    """Base: knows the document and refreshes from the selected layer."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Panel')
        self.doc: Document | None = None
        self._loading = False

    def set_document(self, doc: Document | None) -> None:
        self.doc = doc
        self.refresh()

    def refresh(self) -> None:
        pass

    def _edit(self, layer_type: type, label: str, field: str, value: object) -> None:
        if self._loading or self.doc is None:
            return
        lay = self.doc.selected()
        if isinstance(lay, layer_type):
            self.doc.edit_layer(lay.id, label, key=f'{field}:{lay.id}', **{field: value})


# Layers ------------------------------------------------------------------------------------------


class LayerPanel(QFrame):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Panel')
        self.doc: Document | None = None
        self._loading = False
        self.list = QListWidget()
        self.list.setIconSize(QSize(18, 18))
        self.list.setToolTip(tr('Tick = visible · double-click to rename'))
        self.up_btn = tool_button('chevron-up', tr('Move layer up'))
        self.down_btn = tool_button('chevron-down', tr('Move layer down'))
        self.dup_btn = tool_button('copy', tr('Duplicate layer'))
        self.del_btn = tool_button('trash', tr('Delete layer (Del)'))
        row = QHBoxLayout()
        row.setSpacing(4)
        for b in (self.up_btn, self.down_btn, self.dup_btn, self.del_btn):
            row.addWidget(b)
        row.addStretch(1)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(10, 8, 10, 8)
        lay.setSpacing(6)
        lay.addWidget(section_title(tr('LAYERS')))
        lay.addWidget(self.list, 1)
        lay.addLayout(row)
        self.list.currentItemChanged.connect(self._current_changed)
        self.list.itemChanged.connect(self._item_changed)
        self.list.itemDoubleClicked.connect(self._rename)
        self.up_btn.clicked.connect(lambda: self._move(1))
        self.down_btn.clicked.connect(lambda: self._move(-1))
        self.dup_btn.clicked.connect(self._duplicate)
        self.del_btn.clicked.connect(self._delete)
        self._update_buttons()

    def set_document(self, doc: Document | None) -> None:
        self.doc = doc
        self.refresh()

    def refresh(self) -> None:
        self._loading = True
        self.list.clear()
        if self.doc is not None:
            for lay in reversed(self.doc.layers):
                item = QListWidgetItem(icon(KIND_ICON.get(lay.kind, 'layers')), lay.name)
                item.setData(ID_ROLE, lay.id)
                item.setFlags(item.flags() | Qt.ItemFlag.ItemIsUserCheckable)
                item.setCheckState(Qt.CheckState.Checked if lay.visible else Qt.CheckState.Unchecked)
                self.list.addItem(item)
            bg = QListWidgetItem(icon('lock'), tr('Background'))
            bg.setData(ID_ROLE, None)
            bg.setFlags(bg.flags() | Qt.ItemFlag.ItemIsUserCheckable)
            bg.setCheckState(
                Qt.CheckState.Checked if self.doc.background.visible else Qt.CheckState.Unchecked
            )
            f = QFont(bg.font())
            f.setItalic(True)
            bg.setFont(f)
            self.list.addItem(bg)
        self._loading = False
        self.sync_selection()

    def sync_selection(self) -> None:
        self._loading = True
        sel = self.doc.selected_id if self.doc else None
        for i in range(self.list.count()):
            if self.list.item(i).data(ID_ROLE) == sel:
                self.list.setCurrentRow(i)
                break
        else:
            self.list.setCurrentRow(-1)
        self._loading = False
        self._update_buttons()

    def _update_buttons(self) -> None:
        lay = self.doc.selected() if self.doc else None
        idx = self.doc.index_of(lay.id) if lay is not None else -1
        n = len(self.doc.layers) if self.doc else 0
        self.up_btn.setEnabled(lay is not None and idx < n - 1)
        self.down_btn.setEnabled(lay is not None and idx > 0)
        self.dup_btn.setEnabled(lay is not None)
        self.del_btn.setEnabled(lay is not None)

    def _current_changed(self, cur: QListWidgetItem | None, _prev: QListWidgetItem | None) -> None:
        if self._loading or self.doc is None:
            return
        self.doc.select(cur.data(ID_ROLE) if cur is not None else None)
        self._update_buttons()

    def _item_changed(self, item: QListWidgetItem) -> None:
        if self._loading or self.doc is None:
            return
        self.doc.set_visible(item.data(ID_ROLE), item.checkState() == Qt.CheckState.Checked)

    def _rename(self, item: QListWidgetItem) -> None:
        layer_id = item.data(ID_ROLE)
        lay = self.doc.layer(layer_id) if self.doc else None
        if lay is None:
            return
        name, ok = QInputDialog.getText(self, tr('Rename layer'), tr('Name:'), text=lay.name)
        if ok:
            self.doc.rename_layer(layer_id, name)

    def _move(self, delta: int) -> None:
        if self.doc and self.doc.selected_id is not None:
            self.doc.move_layer(self.doc.selected_id, delta)

    def _duplicate(self) -> None:
        if self.doc and self.doc.selected_id is not None:
            self.doc.duplicate_layer(self.doc.selected_id)

    def _delete(self) -> None:
        if self.doc and self.doc.selected_id is not None:
            self.doc.remove_layer(self.doc.selected_id)


# Properties -----------------------------------------------------------------------------------


class SelectProps(PropsPanel):
    """Shown for Select / Move: the properties of the selected layer."""

    def __init__(
        self,
        text_props: TextProps,
        image_props: ImageProps,
        blur_props: BlurProps,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.text_props, self.image_props, self.blur_props = text_props, image_props, blur_props
        self.empty = hint(
            tr(
                'Click a layer on the photo or in the Layers list to select it. '
                'Drag to move, corner handles resize, the round handle rotates '
                '(Shift: keep proportions / 15° steps). Arrow keys nudge, Del deletes.'
            )
        )
        self.lay = QVBoxLayout(self)
        self.lay.setContentsMargins(0, 0, 0, 0)
        self.lay.addWidget(self.empty)
        self.lay.addStretch(1)


class TextProps(PropsPanel):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.defaults = TextLayer(text='', font_family=DEFAULT_FONT_FAMILY, font_size=64, color='#ffffff')
        self.content = QPlainTextEdit()
        self.content.setPlaceholderText(tr('Your text'))
        self.content.setFixedHeight(70)
        self.font_box = QFontComboBox()
        self.font_box.setCurrentFont(QFont(self.defaults.font_family))
        self.size_box = QSpinBox()
        self.size_box.setRange(4, 2000)
        self.size_box.setSuffix(' px')
        self.size_box.setValue(round(self.defaults.font_size))
        self.color_btn = ColorButton(self.defaults.color)
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
        self.outline_box.setRange(0, 200)
        self.outline_box.setDecimals(1)
        self.outline_box.setSuffix(' px')
        self.outline_color = ColorButton(self.defaults.outline_color)
        self.opacity = ValueSlider(tr('Opacity'), 0, 100, 100, suffix=' %')
        self.rotation = ValueSlider(tr('Rotation'), -180, 180, 0, suffix='°')

        style = QHBoxLayout()
        style.addWidget(self.size_box, 1)
        style.addWidget(self.color_btn)
        style.addWidget(self.bold)
        style.addWidget(self.italic)
        outline = QHBoxLayout()
        outline.addWidget(self.outline_box, 1)
        outline.addWidget(self.outline_color)
        form = QFormLayout()
        form.setContentsMargins(0, 0, 0, 0)
        form.addRow(tr('Font'), self.font_box)
        form.addRow(tr('Size'), style)
        form.addRow(tr('Outline'), outline)
        self.status = hint('')
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        lay.addWidget(section_title(tr('TEXT')))
        lay.addWidget(self.status)
        lay.addWidget(self.content)
        lay.addLayout(form)
        lay.addWidget(self.opacity)
        lay.addWidget(self.rotation)
        lay.addStretch(1)

        self.content.textChanged.connect(lambda: self._set('text', self.content.toPlainText()))
        self.font_box.currentFontChanged.connect(lambda f: self._set('font_family', f.family()))
        self.size_box.valueChanged.connect(lambda v: self._set('font_size', float(v)))
        self.color_btn.color_changed.connect(lambda c: self._set('color', c))
        self.bold.toggled.connect(lambda v: self._set('bold', v))
        self.italic.toggled.connect(lambda v: self._set('italic', v))
        self.outline_box.valueChanged.connect(lambda v: self._set('outline_width', float(v)))
        self.outline_color.color_changed.connect(lambda c: self._set('outline_color', c))
        self.opacity.value_changed.connect(lambda v: self._set('opacity', v / 100))
        self.rotation.value_changed.connect(lambda v: self._set('rotation', v))

    def _set(self, field: str, value: object) -> None:
        if self._loading:
            return
        if field != 'text':
            setattr(self.defaults, field, value)  # the next new text uses the same style
        lay = self.doc.selected() if self.doc else None
        if isinstance(lay, TextLayer):
            values = {field: value}
            if field == 'text':
                first = str(value).strip().splitlines()[0][:32] if str(value).strip() else lay.name
                values['name'] = first
            self.doc.edit_layer(lay.id, 'Edit text', key=f'{field}:{lay.id}', **values)

    def new_layer(self, x: float, y: float, text: str) -> TextLayer:
        d = self.defaults
        return TextLayer(
            text=text,
            font_family=d.font_family,
            font_size=d.font_size,
            color=d.color,
            bold=d.bold,
            italic=d.italic,
            outline_width=d.outline_width,
            outline_color=d.outline_color,
            opacity=d.opacity,
            rotation=d.rotation,
            x=x,
            y=y,
        )

    def refresh(self) -> None:
        lay = self.doc.selected() if self.doc else None
        src = lay if isinstance(lay, TextLayer) else self.defaults
        self._loading = True
        if self.content.toPlainText() != src.text:
            self.content.setPlainText(src.text)
        self.content.setEnabled(isinstance(lay, TextLayer))
        self.font_box.setCurrentFont(QFont(src.font_family))
        self.size_box.setValue(round(src.font_size))
        self.color_btn.set_color(src.color)
        self.bold.setChecked(src.bold)
        self.italic.setChecked(src.italic)
        self.outline_box.setValue(src.outline_width)
        self.outline_color.set_color(src.outline_color)
        self.opacity.set_value(src.opacity * 100)
        self.rotation.set_value(src.rotation)
        self.status.setText(
            tr('Editing the selected text. Double-click a text on the photo to edit it.')
            if isinstance(lay, TextLayer)
            else tr('Click on the photo to add a text. These settings are used for new texts.')
        )
        self._loading = False


class ImageProps(PropsPanel):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.opacity = ValueSlider(tr('Opacity'), 0, 100, 100, suffix=' %')
        self.rotation = ValueSlider(tr('Rotation'), -180, 180, 0, suffix='°')
        self.w_box = QSpinBox()
        self.h_box = QSpinBox()
        for b in (self.w_box, self.h_box):
            b.setRange(1, 40000)
            b.setSuffix(' px')
        self.keep = QCheckBox(tr('Keep proportions'))
        self.keep.setChecked(True)
        self.flip_h = button(tr('Flip horizontal'), 'flip-h')
        self.flip_v = button(tr('Flip vertical'), 'flip-v')
        self.reset_btn = button(tr('Original size'), 'reset')
        size = QHBoxLayout()
        size.addWidget(self.w_box)
        size.addWidget(QLabel('×'))
        size.addWidget(self.h_box)
        flips = QHBoxLayout()
        flips.addWidget(self.flip_h)
        flips.addWidget(self.flip_v)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        lay.addWidget(section_title(tr('IMAGE LAYER')))
        lay.addLayout(size)
        lay.addWidget(self.keep)
        lay.addWidget(self.opacity)
        lay.addWidget(self.rotation)
        lay.addLayout(flips)
        lay.addWidget(self.reset_btn)
        lay.addStretch(1)
        self.opacity.value_changed.connect(lambda v: self._edit(ImageLayer, 'Opacity', 'opacity', v / 100))
        self.rotation.value_changed.connect(lambda v: self._edit(ImageLayer, 'Rotate layer', 'rotation', v))
        self.w_box.valueChanged.connect(lambda v: self._size(w=v))
        self.h_box.valueChanged.connect(lambda v: self._size(h=v))
        self.flip_h.clicked.connect(lambda: self._flip('flip_h'))
        self.flip_v.clicked.connect(lambda: self._flip('flip_v'))
        self.reset_btn.clicked.connect(self._reset_size)

    def _layer(self) -> ImageLayer | None:
        lay = self.doc.selected() if self.doc else None
        return lay if isinstance(lay, ImageLayer) else None

    def _size(self, w: int | None = None, h: int | None = None) -> None:
        lay = self._layer()
        if self._loading or lay is None:
            return
        nw, nh = float(w if w is not None else lay.width), float(h if h is not None else lay.height)
        if self.keep.isChecked():
            if w is not None:
                nh = nw * lay.height / max(lay.width, 1e-6)
            else:
                nw = nh * lay.width / max(lay.height, 1e-6)
        self.doc.edit_layer(lay.id, 'Resize layer', key=f'size:{lay.id}', width=nw, height=nh)
        self.refresh()

    def _flip(self, field: str) -> None:
        lay = self._layer()
        if lay is not None:
            self.doc.edit_layer(lay.id, 'Flip', None, **{field: not getattr(lay, field)})

    def _reset_size(self) -> None:
        lay = self._layer()
        if lay is not None:
            self.doc.edit_layer(
                lay.id, 'Resize layer', width=float(lay.image.width()), height=float(lay.image.height())
            )
            self.refresh()

    def refresh(self) -> None:
        lay = self._layer()
        self.setEnabled(lay is not None)
        if lay is None:
            return
        self._loading = True
        self.w_box.setValue(round(lay.width))
        self.h_box.setValue(round(lay.height))
        self.opacity.set_value(lay.opacity * 100)
        self.rotation.set_value(lay.rotation)
        self._loading = False


BLUR_SHAPES = (('rect', 'Rectangle'), ('ellipse', 'Ellipse'), ('brush', 'Brush'))
BLUR_MODE_LABELS = {'blur': 'Blur', 'pixelate': 'Pixelate'}


class BlurProps(PropsPanel):
    options_changed = Signal()  # tool options (shape, mode, strength, brush size)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.shape_group = QButtonGroup(self)
        shapes = QHBoxLayout()
        shapes.setSpacing(0)
        self.shape_buttons: dict[str, QPushButton] = {}
        for i, (key, label) in enumerate(BLUR_SHAPES):
            b = QPushButton(tr(label))
            b.setObjectName('Segment')
            b.setProperty('pos', 'first' if i == 0 else 'last' if i == len(BLUR_SHAPES) - 1 else 'mid')
            b.setCheckable(True)
            self.shape_group.addButton(b)
            self.shape_buttons[key] = b
            shapes.addWidget(b)
        self.shape_buttons['rect'].setChecked(True)
        self.mode_box = QComboBox()
        for key, label in BLUR_MODE_LABELS.items():
            self.mode_box.addItem(tr(label), key)
        self.strength = ValueSlider(tr('Strength'), 1, 100, 40)
        self.brush = ValueSlider(tr('Brush size'), 4, 1000, 60, suffix=' px')
        self.opacity = ValueSlider(tr('Opacity'), 0, 100, 100, suffix=' %')
        self.info = hint('')
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        lay.addWidget(section_title(tr('BLUR AREA')))
        lay.addLayout(shapes)
        row = QHBoxLayout()
        row.addWidget(QLabel(tr('Effect')))
        row.addWidget(self.mode_box, 1)
        lay.addLayout(row)
        lay.addWidget(self.strength)
        lay.addWidget(self.brush)
        lay.addWidget(self.opacity)
        lay.addWidget(self.info)
        lay.addStretch(1)
        self.shape_group.buttonClicked.connect(lambda _b: self._options())
        self.mode_box.currentIndexChanged.connect(lambda _i: self._options('mode'))
        self.strength.value_changed.connect(lambda _v: self._options('strength'))
        self.brush.value_changed.connect(lambda _v: self._options())
        self.opacity.value_changed.connect(lambda v: self._edit(BlurLayer, 'Opacity', 'opacity', v / 100))
        self._update_info()

    def shape(self) -> str:
        return next(k for k, b in self.shape_buttons.items() if b.isChecked())

    def mode(self) -> str:
        return self.mode_box.currentData()

    def _options(self, layer_field: str | None = None) -> None:
        if self._loading:
            return
        self._update_info()
        self.options_changed.emit()
        if layer_field == 'mode':
            self._edit(BlurLayer, 'Blur effect', 'mode', self.mode())
        elif layer_field == 'strength':
            self._edit(BlurLayer, 'Blur strength', 'strength', self.strength.value())

    def _update_info(self) -> None:
        self.brush.setVisible(self.shape() == 'brush')
        self.info.setText(
            tr(
                'Paint over the area. Strokes are added to the selected brush layer; '
                'deselect it (Esc) to start a new one. [ and ] change the brush size.'
            )
            if self.shape() == 'brush'
            else tr(
                'Drag on the photo to draw the area (Shift: square / circle). Each area is a separate layer.'
            )
        )

    def set_brush_size(self, size: float) -> None:
        self._loading = True
        self.brush.set_value(round(size))
        self._loading = False

    def refresh(self) -> None:
        lay = self.doc.selected() if self.doc else None
        if not isinstance(lay, BlurLayer):
            return
        self._loading = True
        self.mode_box.setCurrentIndex(max(0, self.mode_box.findData(lay.mode)))
        self.strength.set_value(lay.strength)
        self.opacity.set_value(lay.opacity * 100)
        self._loading = False


class CropProps(PropsPanel):
    ratio_changed = Signal(object)  # float | None
    apply_requested = Signal()
    cancel_requested = Signal()

    RATIO_LABELS = {
        'free': 'Free',
        'original': 'Original',
        '1:1': '1:1 (square)',
        '4:3': '4:3',
        '16:9': '16:9',
        '9:16': '9:16 (story)',
    }

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.ratio_box = QComboBox()
        for key in ('free', 'original', '1:1', '4:3', '16:9', '9:16'):
            self.ratio_box.addItem(tr(self.RATIO_LABELS[key]), key)
        self.size_label = QLabel('—')
        self.apply_btn = button(tr('Apply crop'), 'check', 'Accent', tr('Enter'))
        self.cancel_btn = button(tr('Cancel'), tooltip=tr('Esc'))
        row = QHBoxLayout()
        row.addWidget(self.cancel_btn)
        row.addWidget(self.apply_btn, 1)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        lay.addWidget(section_title(tr('CROP')))
        r = QHBoxLayout()
        r.addWidget(QLabel(tr('Ratio')))
        r.addWidget(self.ratio_box, 1)
        lay.addLayout(r)
        lay.addWidget(self.size_label)
        lay.addLayout(row)
        lay.addWidget(
            hint(
                tr(
                    'Drag on the photo to draw the crop frame; drag a corner to resize it, '
                    'inside to move it. Enter = apply, Esc = cancel.'
                )
            )
        )
        lay.addStretch(1)
        self.ratio_box.currentIndexChanged.connect(lambda _i: self.ratio_changed.emit(self.ratio()))
        self.apply_btn.clicked.connect(self.apply_requested.emit)
        self.cancel_btn.clicked.connect(self.cancel_requested.emit)

    def ratio(self) -> float | None:
        key = self.ratio_box.currentData()
        if key == 'original':
            return self.doc.width / self.doc.height if self.doc else None
        return CROP_RATIOS.get(key)

    def set_rect(self, size: tuple[int, int] | None) -> None:
        self.size_label.setText(
            tr('Selection: {w} × {h} px', w=size[0], h=size[1]) if size else tr('No crop frame yet')
        )
        self.apply_btn.setEnabled(size is not None)
        self.cancel_btn.setEnabled(size is not None)


class AdjustProps(PropsPanel):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.sliders: dict[str, ValueSlider] = {}
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        lay.addWidget(section_title(tr('ADJUST PHOTO')))
        for spec in ADJUST_SLIDERS:
            s = ValueSlider(tr(spec.label), spec.minimum, spec.maximum, spec.default)
            s.value_changed.connect(lambda _v: self._changed())
            self.sliders[spec.key] = s
            lay.addWidget(s)
        self.reset_btn = button(tr('Reset'), 'reset')
        lay.addWidget(self.reset_btn)
        lay.addWidget(
            hint(
                tr(
                    'Same algorithms as the batch editor. Applies to the background photo; '
                    'the preview updates at once and the full resolution follows.'
                )
            )
        )
        lay.addStretch(1)
        self.reset_btn.clicked.connect(self._reset)

    def adjust(self) -> PhotoAdjust:
        return PhotoAdjust(**{k: s.value() for k, s in self.sliders.items()})

    def _changed(self) -> None:
        if not self._loading and self.doc is not None:
            self.doc.set_adjust(self.adjust())

    def _reset(self) -> None:
        if self.doc is not None:
            self.doc.set_adjust(PhotoAdjust(), key=None)
            self.refresh()

    def refresh(self) -> None:
        adj = self.doc.background.adjust if self.doc else PhotoAdjust()
        self._loading = True
        for k, s in self.sliders.items():
            s.set_value(getattr(adj, k))
        self._loading = False


class RotateProps(PropsPanel):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.left_btn = button(tr('Rotate left 90°'), 'reset')
        self.right_btn = button(tr('Rotate right 90°'), 'rotate-cw')
        self.flip_h_btn = button(tr('Flip horizontal'), 'flip-h')
        self.flip_v_btn = button(tr('Flip vertical'), 'flip-v')
        self.angle = ValueSlider(tr('Straighten'), -45, 45, 0, decimals=1, suffix='°')
        grid = QVBoxLayout()
        r1 = QHBoxLayout()
        r1.addWidget(self.left_btn)
        r1.addWidget(self.right_btn)
        r2 = QHBoxLayout()
        r2.addWidget(self.flip_h_btn)
        r2.addWidget(self.flip_v_btn)
        grid.addLayout(r1)
        grid.addLayout(r2)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        lay.addWidget(section_title(tr('ROTATE / FLIP')))
        lay.addLayout(grid)
        lay.addWidget(self.angle)
        lay.addWidget(
            hint(
                tr(
                    '90° turns and flips move the whole canvas with its layers. '
                    'Straighten turns only the photo and enlarges it to avoid empty corners.'
                )
            )
        )
        lay.addStretch(1)
        self.left_btn.clicked.connect(lambda: self._do(lambda d: d.rotate90(False)))
        self.right_btn.clicked.connect(lambda: self._do(lambda d: d.rotate90(True)))
        self.flip_h_btn.clicked.connect(lambda: self._do(lambda d: d.flip(True)))
        self.flip_v_btn.clicked.connect(lambda: self._do(lambda d: d.flip(False)))
        self.angle.value_changed.connect(self._angle)
        self.after_canvas_change: Callable[[], None] | None = None

    def _do(self, fn: Callable[[Document], None]) -> None:
        if self.doc is not None:
            fn(self.doc)
            if self.after_canvas_change:
                self.after_canvas_change()
            self.refresh()

    def _angle(self, v: float) -> None:
        if not self._loading and self.doc is not None:
            self.doc.set_angle(v)

    def refresh(self) -> None:
        self._loading = True
        self.angle.set_value(self.doc.background.angle if self.doc else 0.0)
        self._loading = False


def layer_label(lay: Layer | None) -> str:
    return lay.name if lay is not None else ''
