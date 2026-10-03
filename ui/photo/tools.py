"""Canvas tools of the photo editor: select, move, text, crop, blur.

A tool receives mouse events in image coordinates from :class:`ui.photo.canvas.PhotoCanvas`
and edits the document. Undo steps are recorded on the first real movement of a drag, so a
simple click does not create an empty step.
"""

from __future__ import annotations

import math
from typing import TYPE_CHECKING

from PySide6.QtCore import QLineF, QPointF, QRectF, Qt
from PySide6.QtGui import QColor, QKeyEvent, QMouseEvent, QPainter, QPainterPath, QPen, QPolygonF

from core.photo.geometry import crop_rect, round_rect
from ui.photo.document import BlurLayer, ImageLayer, Layer, Stroke, TextLayer
from ui.photo.render import hit_test, layer_polygon, layer_transform, paint_blur_shape

if TYPE_CHECKING:
    from ui.photo.canvas import PhotoCanvas

ACCENT = QColor('#22d3ee')
HANDLE = 5.0  # half size of a handle, screen px
ROTATE_DIST = 26.0  # screen px from the top edge to the rotation handle
DRAG_THRESHOLD = 3.0  # screen px before a press becomes a drag


def _dist(a: QPointF, b: QPointF) -> float:
    return math.hypot(a.x() - b.x(), a.y() - b.y())


class Tool:
    name = ''
    cursor = Qt.CursorShape.ArrowCursor
    shows_selection = True

    def __init__(self, canvas: PhotoCanvas) -> None:
        self.c = canvas

    @property
    def doc(self):  # noqa: ANN201
        return self.c.doc

    def activate(self) -> None:
        pass

    def deactivate(self) -> None:
        pass

    def press(self, e: QMouseEvent, pt: QPointF) -> None:
        pass

    def move(self, e: QMouseEvent, pt: QPointF) -> None:
        pass

    def release(self, e: QMouseEvent, pt: QPointF) -> None:
        pass

    def double_click(self, e: QMouseEvent, pt: QPointF) -> None:
        pass

    def key(self, e: QKeyEvent) -> bool:
        return False

    def paint_overlay(self, p: QPainter) -> None:
        pass

    def cursor_at(self, pt: QPointF) -> Qt.CursorShape:
        return self.cursor


# Select / move ---------------------------------------------------------------------------------


class SelectTool(Tool):
    """Click a layer to select it; drag to move; corner handles resize, the round handle rotates."""

    name = 'select'
    transform_handles = True

    def __init__(self, canvas: PhotoCanvas) -> None:
        super().__init__(canvas)
        self.mode: str | None = None
        self.start_screen = QPointF()
        self.start_pt = QPointF()
        self.last_pt = QPointF()
        self.start_layer: Layer | None = None
        self.handle = -1
        self.dragging = False

    # handles (screen coordinates)
    def handles(self, lay: Layer) -> tuple[list[QPointF], QPointF | None]:
        poly = layer_polygon(lay)
        corners = [self.c.to_screen(poly.at(i)) for i in range(4)]
        rot = None
        if isinstance(lay, (TextLayer, ImageLayer)):
            top_mid = (corners[0] + corners[1]) / 2
            bottom_mid = (corners[3] + corners[2]) / 2
            d = top_mid - bottom_mid
            n = math.hypot(d.x(), d.y()) or 1.0
            rot = top_mid + d / n * ROTATE_DIST
        if isinstance(lay, BlurLayer) and lay.shape == 'brush':
            corners = []
        return corners, rot

    def handle_at(self, screen: QPointF) -> tuple[str | None, int]:
        lay = self.doc.selected() if self.doc else None
        if lay is None or not lay.visible or not self.transform_handles:
            return None, -1
        corners, rot = self.handles(lay)
        if rot is not None and _dist(rot, screen) <= HANDLE + 4:
            return 'rotate', -1
        for i, c in enumerate(corners):
            if _dist(c, screen) <= HANDLE + 4:
                return 'scale', i
        return None, -1

    def layer_at(self, pt: QPointF) -> Layer | None:
        tol = 4 / self.c.scale
        for lay in reversed(self.doc.layers):
            if lay.visible and hit_test(lay, pt.x(), pt.y(), tol):
                return lay
        return None

    def press(self, e: QMouseEvent, pt: QPointF) -> None:
        if self.doc is None or e.button() != Qt.MouseButton.LeftButton:
            return
        self.start_screen = e.position()
        self.start_pt = self.last_pt = pt
        self.dragging = False
        mode, idx = self.handle_at(e.position())
        if mode is None:
            lay = self.pick(pt)
            mode = 'move' if lay is not None else None
        self.mode, self.handle = mode, idx
        sel = self.doc.selected()
        self.start_layer = sel.clone() if sel is not None else None

    def pick(self, pt: QPointF) -> Layer | None:
        lay = self.layer_at(pt)
        self.doc.select(lay.id if lay else None)
        return lay

    def move(self, e: QMouseEvent, pt: QPointF) -> None:
        if self.mode is None or self.doc is None or not (e.buttons() & Qt.MouseButton.LeftButton):
            return
        lay = self.doc.selected()
        if lay is None or self.start_layer is None:
            return
        if not self.dragging:
            if _dist(e.position(), self.start_screen) < DRAG_THRESHOLD:
                return
            self.dragging = True
            self.doc.checkpoint(
                {'move': 'Move layer', 'scale': 'Resize layer', 'rotate': 'Rotate layer'}[self.mode]
            )
        shift = bool(e.modifiers() & Qt.KeyboardModifier.ShiftModifier)
        if self.mode == 'move':
            d = pt - self.last_pt
            lay.translate(d.x(), d.y())
            self.last_pt = pt
        elif self.mode == 'rotate':
            self._rotate(lay, pt, shift)
        elif self.mode == 'scale':
            self._scale(lay, pt, shift)
        self.doc.changed()

    def _rotate(self, lay: Layer, pt: QPointF, snap: bool) -> None:
        s = self.start_layer
        c = QPointF(s.x, s.y)  # type: ignore[union-attr]
        a0 = math.degrees(math.atan2(self.start_pt.y() - c.y(), self.start_pt.x() - c.x()))
        a1 = math.degrees(math.atan2(pt.y() - c.y(), pt.x() - c.x()))
        angle = s.rotation + a1 - a0  # type: ignore[union-attr]
        if snap:
            angle = round(angle / 15) * 15
        lay.rotation = (angle + 180) % 360 - 180  # type: ignore[union-attr]

    def _scale(self, lay: Layer, pt: QPointF, keep_ratio: bool) -> None:
        s = self.start_layer
        if isinstance(lay, ImageLayer) and isinstance(s, ImageLayer):
            t = layer_transform(s)
            inv, _ = t.inverted()
            local = inv.map(pt)
            signs = ((-1, -1), (1, -1), (1, 1), (-1, 1))[self.handle]
            opp = QPointF(-signs[0] * s.width / 2, -signs[1] * s.height / 2)
            w = max(4.0, abs(local.x() - opp.x()))
            h = max(4.0, abs(local.y() - opp.y()))
            if keep_ratio:
                k = max(w / s.width, h / s.height)
                w, h = s.width * k, s.height * k
            centre = t.map(QPointF(opp.x() + signs[0] * w / 2, opp.y() + signs[1] * h / 2))
            lay.x, lay.y, lay.width, lay.height = centre.x(), centre.y(), w, h
        elif isinstance(lay, TextLayer) and isinstance(s, TextLayer):
            c = QPointF(s.x, s.y)
            d0 = max(1.0, _dist(self.start_pt, c))
            k = max(0.05, _dist(pt, c) / d0)
            lay.font_size = max(4.0, min(2000.0, s.font_size * k))
            lay.outline_width = s.outline_width * k
        elif isinstance(lay, BlurLayer) and isinstance(s, BlurLayer) and lay.shape != 'brush':
            x, y, w, h = s.rect
            corners = ((x, y), (x + w, y), (x + w, y + h), (x, y + h))
            ax, ay = corners[(self.handle + 2) % 4]
            nx, ny = min(ax, pt.x()), min(ay, pt.y())
            lay.rect = (nx, ny, max(2.0, abs(pt.x() - ax)), max(2.0, abs(pt.y() - ay)))

    def release(self, e: QMouseEvent, pt: QPointF) -> None:
        self.mode = None
        self.dragging = False
        self.start_layer = None

    def double_click(self, e: QMouseEvent, pt: QPointF) -> None:
        lay = self.layer_at(pt) if self.doc else None
        if isinstance(lay, TextLayer):
            self.doc.select(lay.id)
            self.c.edit_text_requested.emit(lay.id)

    def key(self, e: QKeyEvent) -> bool:
        lay = self.doc.selected() if self.doc else None
        if lay is None:
            return False
        if e.key() in (Qt.Key.Key_Delete, Qt.Key.Key_Backspace):
            self.doc.remove_layer(lay.id)
            return True
        step = 10 if e.modifiers() & Qt.KeyboardModifier.ShiftModifier else 1
        d = {
            Qt.Key.Key_Left: (-step, 0),
            Qt.Key.Key_Right: (step, 0),
            Qt.Key.Key_Up: (0, -step),
            Qt.Key.Key_Down: (0, step),
        }.get(Qt.Key(e.key()))
        if d is None:
            return False
        self.doc.checkpoint('Move layer', key=f'nudge:{lay.id}')
        lay.translate(*d)
        self.doc.changed()
        return True

    def cursor_at(self, pt: QPointF) -> Qt.CursorShape:
        mode, idx = self.handle_at(self.c.to_screen(pt))
        if mode == 'rotate':
            return Qt.CursorShape.CrossCursor
        if mode == 'scale':
            return Qt.CursorShape.SizeFDiagCursor if idx in (0, 2) else Qt.CursorShape.SizeBDiagCursor
        if self.doc and self.layer_at(pt) is not None:
            return Qt.CursorShape.SizeAllCursor
        return self.cursor

    def paint_overlay(self, p: QPainter) -> None:
        lay = self.doc.selected() if self.doc else None
        if lay is None or not lay.visible:
            return
        paint_selection(p, self.c, lay, self.handles(lay) if self.transform_handles else ([], None))


class MoveTool(SelectTool):
    """Drag anywhere to move the selected layer (picks the layer under the cursor if none)."""

    name = 'move'
    cursor = Qt.CursorShape.SizeAllCursor
    transform_handles = False

    def pick(self, pt: QPointF) -> Layer | None:
        sel = self.doc.selected()
        return sel if sel is not None else super().pick(pt)

    def cursor_at(self, pt: QPointF) -> Qt.CursorShape:
        return self.cursor


def paint_selection(
    p: QPainter, canvas: PhotoCanvas, lay: Layer, handles: tuple[list[QPointF], QPointF | None]
) -> None:
    p.save()
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    poly = layer_polygon(lay)
    screen = QPolygonF([canvas.to_screen(poly.at(i)) for i in range(poly.size())])
    if isinstance(lay, BlurLayer) and lay.shape == 'ellipse':
        r = QRectF(
            canvas.to_screen(QPointF(lay.rect[0], lay.rect[1])),
            canvas.to_screen(QPointF(lay.rect[0] + lay.rect[2], lay.rect[1] + lay.rect[3])),
        )
        p.setPen(QPen(QColor(0, 0, 0, 160), 3))
        p.drawEllipse(r)
        p.setPen(QPen(ACCENT, 1.4, Qt.PenStyle.DashLine))
        p.drawEllipse(r)
    p.setPen(QPen(QColor(0, 0, 0, 160), 3))
    p.drawPolygon(screen)
    p.setPen(QPen(ACCENT, 1.4, Qt.PenStyle.DashLine if isinstance(lay, BlurLayer) else Qt.PenStyle.SolidLine))
    p.drawPolygon(screen)
    corners, rot = handles
    if rot is not None:
        top_mid = (screen.at(0) + screen.at(1)) / 2
        p.setPen(QPen(ACCENT, 1.2))
        p.drawLine(QLineF(top_mid, rot))
        p.setBrush(QColor('#0b1020'))
        p.drawEllipse(rot, HANDLE, HANDLE)
    p.setPen(QPen(ACCENT, 1.2))
    p.setBrush(QColor('#ffffff'))
    for c in corners:
        p.drawRect(QRectF(c.x() - HANDLE, c.y() - HANDLE, 2 * HANDLE, 2 * HANDLE))
    p.restore()


# Text --------------------------------------------------------------------------------------------


class TextTool(SelectTool):
    """Click empty space to place a new text; click a text to select / drag it."""

    name = 'text'
    cursor = Qt.CursorShape.IBeamCursor

    def press(self, e: QMouseEvent, pt: QPointF) -> None:
        if self.doc is None or e.button() != Qt.MouseButton.LeftButton:
            return
        mode, _idx = self.handle_at(e.position())
        lay = self.layer_at(pt)
        if mode is not None or isinstance(lay, TextLayer):
            super().press(e, pt)
            return
        self.mode = None
        self.c.create_text_requested.emit(pt.x(), pt.y())

    def double_click(self, e: QMouseEvent, pt: QPointF) -> None:
        super().double_click(e, pt)

    def cursor_at(self, pt: QPointF) -> Qt.CursorShape:
        cur = super().cursor_at(pt)
        return Qt.CursorShape.IBeamCursor if cur == Qt.CursorShape.ArrowCursor else cur


# Crop -------------------------------------------------------------------------------------------


class CropTool(Tool):
    """Drag a frame; corners resize (keeping the ratio), inside moves it; Enter applies."""

    name = 'crop'
    cursor = Qt.CursorShape.CrossCursor
    shows_selection = False

    def __init__(self, canvas: PhotoCanvas) -> None:
        super().__init__(canvas)
        self.rect: QRectF | None = None
        self.ratio: float | None = None
        self.mode: str | None = None
        self.anchor = QPointF()
        self.grab = QPointF()
        self.start_rect = QRectF()

    def activate(self) -> None:
        self.rect = None
        self.c.tool_state_changed.emit()

    def deactivate(self) -> None:
        self.rect = None

    def set_ratio(self, ratio: float | None) -> None:
        self.ratio = ratio
        if self.rect is not None and ratio and self.doc is not None:
            c = self.rect.center()
            w, h = self.rect.width(), self.rect.height()
            if w / max(h, 1e-6) > ratio:
                w = h * ratio
            else:
                h = w / ratio
            x, y, w, h = crop_rect(
                (c.x() - w / 2, c.y() - h / 2), (c.x() + w / 2, c.y() + h / 2), ratio, self.doc.size
            )
            self.rect = QRectF(x, y, w, h)
            self.c.tool_state_changed.emit()
        self.c.update()

    def _corners(self) -> list[QPointF]:
        r = self.rect
        return [r.topLeft(), r.topRight(), r.bottomRight(), r.bottomLeft()] if r is not None else []

    def press(self, e: QMouseEvent, pt: QPointF) -> None:
        if self.doc is None or e.button() != Qt.MouseButton.LeftButton:
            return
        for i, c in enumerate(self._corners()):
            if _dist(self.c.to_screen(c), e.position()) <= HANDLE + 5:
                self.mode = 'corner'
                self.anchor = self._corners()[(i + 2) % 4]
                return
        if self.rect is not None and self.rect.contains(pt):
            self.mode = 'move'
            self.grab = pt
            self.start_rect = QRectF(self.rect)
            return
        self.mode = 'new'
        self.anchor = pt

    def move(self, e: QMouseEvent, pt: QPointF) -> None:
        if self.mode is None or self.doc is None:
            return
        w, h = self.doc.size
        if self.mode in ('new', 'corner'):
            x, y, rw, rh = crop_rect((self.anchor.x(), self.anchor.y()), (pt.x(), pt.y()), self.ratio, (w, h))
            self.rect = QRectF(x, y, rw, rh)
        elif self.mode == 'move':
            d = pt - self.grab
            r = self.start_rect.translated(d)
            r.moveLeft(min(max(r.left(), 0), w - r.width()))
            r.moveTop(min(max(r.top(), 0), h - r.height()))
            self.rect = r
        self.c.tool_state_changed.emit()
        self.c.update()

    def release(self, e: QMouseEvent, pt: QPointF) -> None:
        if self.rect is not None and (self.rect.width() < 2 or self.rect.height() < 2):
            self.rect = None
            self.c.tool_state_changed.emit()
        self.mode = None
        self.c.update()

    def apply(self) -> bool:
        if self.rect is None or self.doc is None:
            return False
        rect = round_rect(
            (self.rect.x(), self.rect.y(), self.rect.width(), self.rect.height()), self.doc.size
        )
        self.rect = None
        self.doc.crop(rect)
        self.c.fit()
        self.c.tool_state_changed.emit()
        return True

    def cancel(self) -> None:
        self.rect = None
        self.c.tool_state_changed.emit()
        self.c.update()

    def key(self, e: QKeyEvent) -> bool:
        if e.key() in (Qt.Key.Key_Return, Qt.Key.Key_Enter):
            return self.apply()
        if e.key() == Qt.Key.Key_Escape and self.rect is not None:
            self.cancel()
            return True
        return False

    def cursor_at(self, pt: QPointF) -> Qt.CursorShape:
        screen = self.c.to_screen(pt)
        for i, c in enumerate(self._corners()):
            if _dist(self.c.to_screen(c), screen) <= HANDLE + 5:
                return Qt.CursorShape.SizeFDiagCursor if i in (0, 2) else Qt.CursorShape.SizeBDiagCursor
        if self.rect is not None and self.rect.contains(pt):
            return Qt.CursorShape.SizeAllCursor
        return self.cursor

    def paint_overlay(self, p: QPainter) -> None:
        if self.rect is None or self.doc is None:
            return
        canvas = QRectF(self.c.to_screen(QPointF(0, 0)), self.c.to_screen(QPointF(*self.doc.size)))
        r = QRectF(self.c.to_screen(self.rect.topLeft()), self.c.to_screen(self.rect.bottomRight()))
        p.save()
        outside = QPainterPath()
        outside.addRect(canvas)
        inner = QPainterPath()
        inner.addRect(r)
        p.fillPath(outside.subtracted(inner), QColor(0, 0, 0, 150))
        p.setPen(QPen(QColor(255, 255, 255, 90), 1))
        for k in (1, 2):
            x = r.left() + r.width() * k / 3
            y = r.top() + r.height() * k / 3
            p.drawLine(QLineF(x, r.top(), x, r.bottom()))
            p.drawLine(QLineF(r.left(), y, r.right(), y))
        p.setPen(QPen(QColor('#ffffff'), 1.5))
        p.drawRect(r)
        p.setBrush(QColor('#ffffff'))
        p.setPen(QPen(ACCENT, 1.2))
        for c in (r.topLeft(), r.topRight(), r.bottomRight(), r.bottomLeft()):
            p.drawRect(QRectF(c.x() - HANDLE, c.y() - HANDLE, 2 * HANDLE, 2 * HANDLE))
        x, y, w, h = round_rect(
            (self.rect.x(), self.rect.y(), self.rect.width(), self.rect.height()), self.doc.size
        )
        label = f'{w} × {h}'
        fm = p.fontMetrics()
        box = QRectF(r.left(), r.top() - fm.height() - 8, fm.horizontalAdvance(label) + 12, fm.height() + 4)
        if box.top() < 0:
            box.moveTop(r.top() + 4)
        p.setPen(Qt.PenStyle.NoPen)
        p.setBrush(QColor(0, 0, 0, 170))
        p.drawRoundedRect(box, 4, 4)
        p.setPen(QColor('#ffffff'))
        p.drawText(box, Qt.AlignmentFlag.AlignCenter, label)
        p.restore()


# Blur -------------------------------------------------------------------------------------------


class BlurTool(Tool):
    """Rectangle / ellipse: drag an area. Brush: paint; strokes go into the selected brush
    layer, or into a new one when no brush layer is selected."""

    name = 'blur'
    cursor = Qt.CursorShape.CrossCursor

    def __init__(self, canvas: PhotoCanvas) -> None:
        super().__init__(canvas)
        self.shape = 'rect'
        self.mode = 'blur'
        self.strength = 40.0
        self.brush_size = 60.0  # diameter, image px
        self.drag_start: QPointF | None = None
        self.drag_rect: QRectF | None = None
        self.points: list[tuple[float, float]] = []
        self.hover: QPointF | None = None

    def press(self, e: QMouseEvent, pt: QPointF) -> None:
        if self.doc is None or e.button() != Qt.MouseButton.LeftButton:
            return
        if self.shape == 'brush':
            self.points = [(pt.x(), pt.y())]
        else:
            self.drag_start = pt
            self.drag_rect = QRectF(pt, pt)
        self.c.update()

    def move(self, e: QMouseEvent, pt: QPointF) -> None:
        self.hover = pt
        if self.points:
            lx, ly = self.points[-1]
            if math.hypot(pt.x() - lx, pt.y() - ly) >= max(1.0, self.brush_size / 10):
                self.points.append((pt.x(), pt.y()))
        elif self.drag_start is not None:
            r = QRectF(self.drag_start, pt).normalized()
            if e.modifiers() & Qt.KeyboardModifier.ShiftModifier:  # square / circle
                side = max(r.width(), r.height())
                r = QRectF(
                    self.drag_start.x() if pt.x() >= self.drag_start.x() else self.drag_start.x() - side,
                    self.drag_start.y() if pt.y() >= self.drag_start.y() else self.drag_start.y() - side,
                    side,
                    side,
                )
            self.drag_rect = r
        self.c.update()

    def release(self, e: QMouseEvent, pt: QPointF) -> None:
        if self.doc is None:
            return
        if self.points:
            stroke = Stroke(self.brush_size / 2, tuple(self.points))
            self.points = []
            sel = self.doc.selected()
            if isinstance(sel, BlurLayer) and sel.shape == 'brush':
                self.doc.checkpoint('Blur brush')
                sel.strokes = [*sel.strokes, stroke]
                self.doc.changed()
            else:
                self.doc.add_layer(
                    BlurLayer(shape='brush', mode=self.mode, strength=self.strength, strokes=[stroke]),
                    'Blur area',
                )
        elif self.drag_rect is not None:
            r = self.drag_rect
            self.drag_start = self.drag_rect = None
            if r.width() * self.c.scale >= 4 and r.height() * self.c.scale >= 4:
                self.doc.add_layer(
                    BlurLayer(
                        shape=self.shape,
                        mode=self.mode,
                        strength=self.strength,
                        rect=(r.x(), r.y(), r.width(), r.height()),
                    ),
                    'Blur area',
                )
        self.c.update()

    def key(self, e: QKeyEvent) -> bool:
        if e.key() == Qt.Key.Key_BracketLeft:
            self.brush_size = max(4.0, self.brush_size / 1.2)
        elif e.key() == Qt.Key.Key_BracketRight:
            self.brush_size = min(2000.0, self.brush_size * 1.2)
        else:
            return False
        self.c.tool_state_changed.emit()
        self.c.update()
        return True

    def paint_overlay(self, p: QPainter) -> None:
        p.save()
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        sel = self.doc.selected() if self.doc else None
        if isinstance(sel, BlurLayer) and sel.visible:
            paint_selection(p, self.c, sel, ([], None))
        if self.drag_rect is not None:
            r = QRectF(
                self.c.to_screen(self.drag_rect.topLeft()), self.c.to_screen(self.drag_rect.bottomRight())
            )
            p.setPen(QPen(ACCENT, 1.5, Qt.PenStyle.DashLine))
            p.setBrush(QColor(34, 211, 238, 40))
            if self.shape == 'ellipse':
                p.drawEllipse(r)
            else:
                p.drawRect(r)
        if self.points:
            p.setTransform(self.c.view_transform())
            preview = BlurLayer(shape='brush', strokes=[Stroke(self.brush_size / 2, tuple(self.points))])
            paint_blur_shape(p, preview, QColor(34, 211, 238, 90))
            p.resetTransform()
        if self.shape == 'brush' and self.hover is not None:
            c = self.c.to_screen(self.hover)
            r = self.brush_size / 2 * self.c.scale
            p.setBrush(Qt.BrushStyle.NoBrush)
            p.setPen(QPen(QColor(0, 0, 0, 160), 3))
            p.drawEllipse(c, r, r)
            p.setPen(QPen(QColor('#ffffff'), 1.2))
            p.drawEllipse(c, r, r)
        p.restore()
