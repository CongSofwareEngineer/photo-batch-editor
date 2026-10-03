"""Collage maker: photos in a template (2 side by side, 2 stacked, 2×2, 1 large + 2 small).

Click a cell to choose its photo, drag inside a cell to position the photo, wheel to zoom it.
Spacing, corner radius and background colour apply to the whole collage. The result opens
in the photo editor as a new document.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from PySide6.QtCore import QPointF, QRectF, Qt, Signal
from PySide6.QtGui import QColor, QImage, QMouseEvent, QPainter, QPainterPath, QPaintEvent, QPen, QWheelEvent
from PySide6.QtWidgets import (
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QFileDialog,
    QHBoxLayout,
    QLabel,
    QMessageBox,
    QVBoxLayout,
    QWidget,
)

from core.i18n import tr
from core.photo.collage import (
    SIZE_PRESETS,
    TEMPLATE_LABELS,
    TEMPLATES,
    Cell,
    cell_rects,
    clamp_offset,
    cover_rect,
)
from core.photo.image_io import load_rgba
from ui.photo.panels import ColorButton, ValueSlider, hint, section_title
from ui.photo.render import qimage_from_rgba
from ui.widgets import button

IMAGE_FILTER = 'Images (*.jpg *.jpeg *.png *.tif *.tiff *.webp *.bmp)'


@dataclass
class CellImage:
    image: QImage
    zoom: float = 1.0
    offset: tuple[float, float] = (0.0, 0.0)


@dataclass
class CollageSpec:
    template: str = 'two_horizontal'
    size: tuple[int, int] = (2000, 2000)
    spacing: float = 20.0
    radius: float = 0.0
    background: str = '#ffffff'


def render_collage(
    spec: CollageSpec, cells: list[CellImage | None], p: QPainter | None = None
) -> QImage | None:
    """Draw the collage; into ``p`` (output coordinates) or a new full-size image."""
    out = None
    if p is None:
        out = QImage(spec.size[0], spec.size[1], QImage.Format.Format_ARGB32_Premultiplied)
        p = QPainter(out)
    p.save()
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform)
    p.fillRect(QRectF(0, 0, *spec.size), QColor(spec.background))
    for cell, ci in zip(cell_rects(spec.template, *spec.size, spec.spacing), cells, strict=False):
        r = QRectF(cell.x, cell.y, cell.w, cell.h)
        path = QPainterPath()
        rad = min(spec.radius, cell.w / 2, cell.h / 2)
        path.addRoundedRect(r, rad, rad)
        if ci is None:
            p.fillPath(path, QColor(0, 0, 0, 25))
            continue
        p.save()
        p.setClipPath(path)
        x, y, w, h = cover_rect(ci.image.width(), ci.image.height(), cell, ci.zoom, ci.offset)
        p.drawImage(QRectF(x, y, w, h), ci.image)
        p.restore()
    p.restore()
    if out is not None:
        p.end()
    return out


class CollagePreview(QWidget):
    cell_clicked = Signal(int)  # empty cell → choose a photo
    changed = Signal()

    def __init__(
        self, spec: CollageSpec, cells: list[CellImage | None], parent: QWidget | None = None
    ) -> None:
        super().__init__(parent)
        self.spec = spec
        self.cells = cells
        self.selected = -1
        self._drag: tuple[int, QPointF, tuple[float, float]] | None = None
        self.setMinimumSize(420, 420)
        self.setMouseTracking(True)

    def _geometry(self) -> tuple[float, QPointF]:
        w, h = self.spec.size
        k = min((self.width() - 24) / w, (self.height() - 24) / h)
        return k, QPointF((self.width() - w * k) / 2, (self.height() - h * k) / 2)

    def _cells(self) -> list[Cell]:
        return cell_rects(self.spec.template, *self.spec.size, self.spec.spacing)

    def cell_at(self, pos: QPointF) -> int:
        k, off = self._geometry()
        pt = (pos - off) / k
        for i, c in enumerate(self._cells()):
            if c.x <= pt.x() <= c.x + c.w and c.y <= pt.y() <= c.y + c.h:
                return i
        return -1

    def paintEvent(self, _e: QPaintEvent) -> None:  # noqa: N802
        p = QPainter(self)
        p.fillRect(self.rect(), QColor('#080c18'))
        k, off = self._geometry()
        p.translate(off)
        p.scale(k, k)
        render_collage(self.spec, self.cells, p)
        p.resetTransform()
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        for i, c in enumerate(self._cells()):
            r = QRectF(off.x() + c.x * k, off.y() + c.y * k, c.w * k, c.h * k)
            if i < len(self.cells) and self.cells[i] is None:
                p.setPen(QColor('#5d6b88'))
                p.drawText(r, Qt.AlignmentFlag.AlignCenter, tr('Click to add a photo'))
            if i == self.selected:
                p.setPen(QPen(QColor('#22d3ee'), 2))
                p.setBrush(Qt.BrushStyle.NoBrush)
                p.drawRect(r.adjusted(1, 1, -1, -1))

    def mousePressEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        i = self.cell_at(e.position())
        if i < 0 or e.button() != Qt.MouseButton.LeftButton:
            return
        self.selected = i
        if self.cells[i] is None:
            self.cell_clicked.emit(i)
        else:
            self._drag = (i, e.position(), self.cells[i].offset)
        self.update()
        self.changed.emit()

    def mouseMoveEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        i = self.cell_at(e.position())
        filled = 0 <= i < len(self.cells) and self.cells[i] is not None
        self.setCursor(
            Qt.CursorShape.SizeAllCursor
            if filled
            else Qt.CursorShape.PointingHandCursor
            if i >= 0
            else Qt.CursorShape.ArrowCursor
        )
        if self._drag is None:
            return
        idx, start, off0 = self._drag
        ci = self.cells[idx]
        k, _ = self._geometry()
        d = (e.position() - start) / k
        cell = self._cells()[idx]
        ci.offset = clamp_offset(
            ci.image.width(), ci.image.height(), cell, ci.zoom, (off0[0] + d.x(), off0[1] + d.y())
        )
        self.update()

    def mouseReleaseEvent(self, _e: QMouseEvent) -> None:  # noqa: N802
        self._drag = None

    def mouseDoubleClickEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        i = self.cell_at(e.position())
        if i >= 0:
            self.cell_clicked.emit(i)

    def wheelEvent(self, e: QWheelEvent) -> None:  # noqa: N802
        i = self.cell_at(e.position())
        if i < 0 or self.cells[i] is None:
            return
        ci = self.cells[i]
        ci.zoom = min(5.0, max(1.0, ci.zoom * (1.1 ** (e.angleDelta().y() / 120))))
        ci.offset = clamp_offset(ci.image.width(), ci.image.height(), self._cells()[i], ci.zoom, ci.offset)
        self.update()


class CollageDialog(QDialog):
    def __init__(self, parent: QWidget | None = None, start_dir: str = '') -> None:
        super().__init__(parent)
        self.setWindowTitle(tr('Collage'))
        self.resize(1100, 760)
        self.start_dir = start_dir
        self.spec = CollageSpec()
        self.cells: list[CellImage | None] = [None] * len(TEMPLATES[self.spec.template])
        self.preview = CollagePreview(self.spec, self.cells)
        self.result_image: QImage | None = None

        self.template_box = QComboBox()
        for key, label in TEMPLATE_LABELS.items():
            self.template_box.addItem(tr(label), key)
        self.size_box = QComboBox()
        for key, (w, h) in SIZE_PRESETS.items():
            self.size_box.addItem(f'{key}  ({w} × {h})', key)
        self.spacing = ValueSlider(tr('Spacing'), 0, 200, self.spec.spacing, suffix=' px')
        self.radius = ValueSlider(tr('Corner radius'), 0, 300, self.spec.radius, suffix=' px')
        self.bg_btn = ColorButton(self.spec.background)
        self.add_btn = button(
            tr('Choose photos…'), 'image-plus', 'Accent', tr('Fill the cells in order with several photos')
        )
        self.clear_btn = button(tr('Remove photo from cell'), 'trash')
        bg_row = QHBoxLayout()
        bg_row.addWidget(QLabel(tr('Background colour')))
        bg_row.addStretch(1)
        bg_row.addWidget(self.bg_btn)

        side = QVBoxLayout()
        side.setSpacing(8)
        side.addWidget(section_title(tr('LAYOUT')))
        side.addWidget(self.template_box)
        side.addWidget(QLabel(tr('Size')))
        side.addWidget(self.size_box)
        side.addWidget(self.spacing)
        side.addWidget(self.radius)
        side.addLayout(bg_row)
        side.addSpacing(8)
        side.addWidget(self.add_btn)
        side.addWidget(self.clear_btn)
        side.addWidget(
            hint(
                tr(
                    'Click a cell to choose its photo (double-click to replace it). '
                    'Drag a photo to position it in its cell, use the wheel to zoom it.'
                )
            )
        )
        side.addStretch(1)
        side_w = QWidget()
        side_w.setObjectName('Panel')
        side_w.setLayout(side)
        side_w.setFixedWidth(300)

        self.buttons = QDialogButtonBox()
        self.ok_btn = self.buttons.addButton(
            tr('Open in Photo editor'), QDialogButtonBox.ButtonRole.AcceptRole
        )
        self.buttons.addButton(tr('Cancel'), QDialogButtonBox.ButtonRole.RejectRole)
        body = QHBoxLayout()
        body.addWidget(self.preview, 1)
        body.addWidget(side_w)
        lay = QVBoxLayout(self)
        lay.addLayout(body, 1)
        lay.addWidget(self.buttons)

        self.template_box.currentIndexChanged.connect(self._template_changed)
        self.size_box.currentIndexChanged.connect(self._size_changed)
        self.spacing.value_changed.connect(lambda v: self._set('spacing', v))
        self.radius.value_changed.connect(lambda v: self._set('radius', v))
        self.bg_btn.color_changed.connect(lambda c: self._set('background', c))
        self.add_btn.clicked.connect(self._add_many)
        self.clear_btn.clicked.connect(self._clear_cell)
        self.preview.cell_clicked.connect(self._choose_for)
        self.buttons.accepted.connect(self._accept)
        self.buttons.rejected.connect(self.reject)
        self._size_changed()

    def _set(self, field: str, value: object) -> None:
        setattr(self.spec, field, value)
        self._reclamp()
        self.preview.update()

    def _reclamp(self) -> None:
        for ci, cell in zip(
            self.cells, cell_rects(self.spec.template, *self.spec.size, self.spec.spacing), strict=False
        ):
            if ci is not None:
                ci.offset = clamp_offset(ci.image.width(), ci.image.height(), cell, ci.zoom, ci.offset)

    def _template_changed(self) -> None:
        self.spec.template = self.template_box.currentData()
        n = len(TEMPLATES[self.spec.template])
        filled = [c for c in self.cells if c is not None]
        self.cells[:] = (filled + [None] * n)[:n]
        self.preview.selected = -1
        self._reclamp()
        self.preview.update()

    def _size_changed(self) -> None:
        self.spec.size = SIZE_PRESETS[self.size_box.currentData()]
        self._reclamp()
        self.preview.update()

    def _load(self, path: str) -> CellImage | None:
        try:
            img = qimage_from_rgba(load_rgba(Path(path)))
        except Exception as exc:  # noqa: BLE001
            QMessageBox.warning(
                self, tr('Collage'), tr('Cannot open the photo:\n{path}\n\n{error}', path=path, error=exc)
            )
            return None
        self.start_dir = str(Path(path).parent)
        return CellImage(img)

    def _choose_for(self, index: int) -> None:
        path, _ = QFileDialog.getOpenFileName(self, tr('Choose a photo'), self.start_dir, tr(IMAGE_FILTER))
        if path:
            ci = self._load(path)
            if ci is not None:
                self.cells[index] = ci
                self.preview.selected = index
                self.preview.update()

    def _add_many(self) -> None:
        paths, _ = QFileDialog.getOpenFileNames(self, tr('Choose photos'), self.start_dir, tr(IMAGE_FILTER))
        free = [i for i, c in enumerate(self.cells) if c is None] or list(range(len(self.cells)))
        for i, path in zip(free, paths, strict=False):
            ci = self._load(path)
            if ci is not None:
                self.cells[i] = ci
        self.preview.update()

    def _clear_cell(self) -> None:
        i = self.preview.selected
        if 0 <= i < len(self.cells):
            self.cells[i] = None
            self.preview.update()

    def _accept(self) -> None:
        if not any(self.cells):
            QMessageBox.information(self, tr('Collage'), tr('Add at least one photo first.'))
            return
        self.result_image = render_collage(self.spec, self.cells)
        self.accept()
