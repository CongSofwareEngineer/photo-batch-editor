"""Photo editor canvas: Photoshop-like zoom / pan and the active tool.

* Ctrl + wheel: zoom at the cursor (10 %–1600 %); wheel: scroll, Shift + wheel: sideways.
* Space + drag or middle-button drag: pan.
* The view maps ``screen = offset + image * scale``.
"""

from __future__ import annotations

from PySide6.QtCore import QEvent, QPointF, QRectF, Qt, Signal
from PySide6.QtGui import (
    QBrush,
    QColor,
    QEnterEvent,
    QKeyEvent,
    QMouseEvent,
    QPainter,
    QPaintEvent,
    QPen,
    QPixmap,
    QResizeEvent,
    QTransform,
    QWheelEvent,
)
from PySide6.QtWidgets import QAbstractSpinBox, QApplication, QLineEdit, QPlainTextEdit, QTextEdit, QWidget

from core.i18n import tr
from core.photo.geometry import clamp_zoom, fit_scale, next_zoom, zoom_at
from ui.photo.document import Document
from ui.photo.render import BaseCache, Renderer
from ui.photo.tools import BlurTool, CropTool, MoveTool, SelectTool, TextTool, Tool

BACKDROP = QColor('#080c18')


def checker_brush(size: int = 8) -> QBrush:
    pm = QPixmap(size * 2, size * 2)
    pm.fill(QColor('#3a3f4b'))
    p = QPainter(pm)
    p.fillRect(0, 0, size, size, QColor('#2b2f39'))
    p.fillRect(size, size, size, size, QColor('#2b2f39'))
    p.end()
    return QBrush(pm)


class PhotoCanvas(QWidget):
    zoom_changed = Signal(float)
    cursor_moved = Signal(object)  # QPointF in image px, or None
    edit_text_requested = Signal(int)  # layer id
    create_text_requested = Signal(float, float)  # image position
    tool_state_changed = Signal()  # crop frame / brush size changed (panels refresh)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('PhotoCanvas')
        self.setMouseTracking(True)
        self.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        self.setAttribute(Qt.WidgetAttribute.WA_OpaquePaintEvent)
        self.setMinimumSize(320, 240)
        self.doc: Document | None = None
        self.base = BaseCache(self.update, self)
        self.renderer = Renderer(self.base)
        self.scale = 1.0
        self.offset = QPointF(0, 0)
        self.fit_mode = True
        self._fit_upscale = False
        self._space = False
        self._pan_last: QPointF | None = None
        self._checker = checker_brush()
        self.tools: dict[str, Tool] = {
            t.name: t
            for t in (SelectTool(self), MoveTool(self), TextTool(self), CropTool(self), BlurTool(self))
        }
        self.tool: Tool = self.tools['select']

    # document / tool -------------------------------------------------------------------------
    def set_document(self, doc: Document | None) -> None:
        if self.doc is not None and self._doc_event in self.doc.listeners:
            self.doc.listeners.remove(self._doc_event)
        self.doc = doc
        if doc is not None:
            doc.listeners.append(self._doc_event)
        self.tool.activate()
        self.fit(upscale=False)  # a small photo opens at 100 %, not enlarged

    def _doc_event(self, what: str) -> None:
        if what != 'history':
            self.update()

    def set_tool(self, key: str) -> None:
        tool = self.tools.get(key)
        if tool is None or tool is self.tool:
            return
        self.tool.deactivate()
        self.tool = tool
        tool.activate()
        self._update_cursor()
        self.update()

    # mapping ---------------------------------------------------------------------------------
    def to_image(self, pos: QPointF) -> QPointF:
        return (pos - self.offset) / self.scale

    def to_screen(self, pt: QPointF) -> QPointF:
        return self.offset + pt * self.scale

    def view_transform(self) -> QTransform:
        return QTransform(self.scale, 0, 0, self.scale, self.offset.x(), self.offset.y())

    # zoom ------------------------------------------------------------------------------------
    def fit(self, upscale: bool = True) -> None:
        """Fit on screen (Ctrl+0); like Photoshop it enlarges small photos unless ``upscale=False``."""
        self.fit_mode = True
        self._fit_upscale = upscale
        self._apply_fit()

    def _apply_fit(self) -> None:
        if self.doc is None:
            return
        w, h = self.doc.size
        self.scale = fit_scale(w, h, self.width(), self.height())
        if not self._fit_upscale:
            self.scale = min(self.scale, 1.0)
        self.offset = QPointF((self.width() - w * self.scale) / 2, (self.height() - h * self.scale) / 2)
        self.zoom_changed.emit(self.scale)
        self.update()

    def actual_size(self) -> None:
        self.zoom_to(1.0)

    def zoom_to(self, scale: float, anchor: QPointF | None = None) -> None:
        if self.doc is None:
            return
        new = clamp_zoom(scale)
        if anchor is None:
            anchor = QPointF(self.width() / 2, self.height() / 2)
        ox, oy = zoom_at(self.scale, new, (self.offset.x(), self.offset.y()), (anchor.x(), anchor.y()))
        self.scale = new
        self.offset = QPointF(ox, oy)
        self.fit_mode = False
        self._clamp_offset()
        self.zoom_changed.emit(self.scale)
        self.update()

    def zoom_step(self, direction: int, anchor: QPointF | None = None) -> None:
        self.zoom_to(next_zoom(self.scale, direction), anchor)

    def _clamp_offset(self) -> None:
        """Keep at least part of the image in view."""
        if self.doc is None:
            return
        w, h = self.doc.size[0] * self.scale, self.doc.size[1] * self.scale
        m = 60.0
        x = min(max(self.offset.x(), m - w), self.width() - m)
        y = min(max(self.offset.y(), m - h), self.height() - m)
        self.offset = QPointF(x, y)

    def pan(self, dx: float, dy: float) -> None:
        self.offset += QPointF(dx, dy)
        self.fit_mode = False
        self._clamp_offset()
        self.update()

    def resizeEvent(self, e: QResizeEvent) -> None:  # noqa: N802
        super().resizeEvent(e)
        if self.fit_mode:
            self._apply_fit()
        else:
            d = QPointF(e.size().width() - e.oldSize().width(), e.size().height() - e.oldSize().height()) / 2
            if e.oldSize().width() > 0:
                self.offset += d
            self._clamp_offset()

    # painting --------------------------------------------------------------------------------
    def paintEvent(self, _e: QPaintEvent) -> None:  # noqa: N802
        p = QPainter(self)
        p.fillRect(self.rect(), BACKDROP)
        if self.doc is None:
            p.setPen(QColor('#5d6b88'))
            p.drawText(
                self.rect(), Qt.AlignmentFlag.AlignCenter, tr('Open a photo (Ctrl+O) or drop a file here')
            )
            return
        w, h = self.doc.size
        r = QRectF(self.offset, self.offset + QPointF(w * self.scale, h * self.scale))
        p.save()
        p.setBrushOrigin(r.topLeft())
        p.fillRect(r, self._checker)
        p.setClipRect(r)
        p.setTransform(self.view_transform())
        self.renderer.paint(p, self.doc.state, display_scale=self.scale * self.devicePixelRatioF())
        p.restore()
        p.setPen(QPen(QColor(0, 0, 0, 200), 1))
        p.drawRect(r.adjusted(-0.5, -0.5, 0.5, 0.5))
        self.tool.paint_overlay(p)

    # input -----------------------------------------------------------------------------------
    def _panning_active(self) -> bool:
        return self._space or self._pan_last is not None

    def mousePressEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        self.setFocus()
        if e.button() == Qt.MouseButton.MiddleButton or (
            e.button() == Qt.MouseButton.LeftButton and self._space
        ):
            self._pan_last = e.position()
            self.setCursor(Qt.CursorShape.ClosedHandCursor)
            return
        if self.doc is not None:
            self.tool.press(e, self.to_image(e.position()))

    def mouseMoveEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        pos = e.position()
        if self._pan_last is not None:
            d = pos - self._pan_last
            self._pan_last = pos
            self.pan(d.x(), d.y())
            return
        if self.doc is None:
            return
        pt = self.to_image(pos)
        self.tool.move(e, pt)
        self.cursor_moved.emit(pt)
        if not e.buttons():
            self._update_cursor(pt)

    def mouseReleaseEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        if self._pan_last is not None:
            self._pan_last = None
            self._update_cursor()
            return
        if self.doc is not None:
            self.tool.release(e, self.to_image(e.position()))
            self._update_cursor(self.to_image(e.position()))

    def mouseDoubleClickEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        if self.doc is not None and e.button() == Qt.MouseButton.LeftButton and not self._space:
            self.tool.double_click(e, self.to_image(e.position()))

    def wheelEvent(self, e: QWheelEvent) -> None:  # noqa: N802
        if self.doc is None:
            return
        delta = e.angleDelta()
        if e.modifiers() & Qt.KeyboardModifier.ControlModifier:
            steps = delta.y() / 120.0
            if steps:
                self.zoom_to(self.scale * (1.2**steps), e.position())
            e.accept()
            return
        pd = e.pixelDelta()
        dx, dy = (pd.x(), pd.y()) if not pd.isNull() else (delta.x() / 2, delta.y() / 2)
        if e.modifiers() & Qt.KeyboardModifier.ShiftModifier:
            dx, dy = dy, dx
        self.pan(dx, dy)
        e.accept()

    def keyPressEvent(self, e: QKeyEvent) -> None:  # noqa: N802
        if e.key() == Qt.Key.Key_Space:
            if not e.isAutoRepeat():
                self._space = True
                self.setCursor(Qt.CursorShape.OpenHandCursor)
            e.accept()
            return
        if self.doc is not None and self.tool.key(e):
            e.accept()
            return
        super().keyPressEvent(e)

    def keyReleaseEvent(self, e: QKeyEvent) -> None:  # noqa: N802
        if e.key() == Qt.Key.Key_Space and not e.isAutoRepeat():
            self._space = False
            if self._pan_last is None:
                self._update_cursor()
            e.accept()
            return
        super().keyReleaseEvent(e)

    def focusOutEvent(self, e) -> None:  # noqa: N802, ANN001
        self._space = False
        super().focusOutEvent(e)

    def enterEvent(self, e: QEnterEvent) -> None:  # noqa: N802
        # take the keyboard (Space, Enter, arrows) unless the user is typing somewhere
        focus = QApplication.focusWidget()
        if not isinstance(focus, (QLineEdit, QAbstractSpinBox, QPlainTextEdit, QTextEdit)):
            self.setFocus()
        super().enterEvent(e)

    def leaveEvent(self, e: QEvent) -> None:  # noqa: N802
        self.cursor_moved.emit(None)
        super().leaveEvent(e)

    def _update_cursor(self, pt: QPointF | None = None) -> None:
        if self._space:
            self.setCursor(Qt.CursorShape.OpenHandCursor)
        elif pt is not None and self.doc is not None:
            self.setCursor(self.tool.cursor_at(pt))
        else:
            self.setCursor(self.tool.cursor)
