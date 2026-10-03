"""Large viewer window: zoom, navigation, before/after (spec section 8.6).

Displays the exported JPEG files and the originals; photos are never reprocessed.
"""

from __future__ import annotations

import logging
from collections import OrderedDict
from pathlib import Path

from PySide6.QtCore import QObject, QPointF, QRectF, QSize, Qt, QThreadPool, Signal
from PySide6.QtGui import (
    QCloseEvent,
    QColor,
    QGuiApplication,
    QImage,
    QKeyEvent,
    QMouseEvent,
    QPainter,
    QPaintEvent,
    QPixmap,
    QWheelEvent,
)
from PySide6.QtWidgets import QHBoxLayout, QLabel, QPushButton, QSizePolicy, QVBoxLayout, QWidget

from core.batch import FileResult
from core.i18n import tr
from core.io_utils import open_in_file_manager
from core.system import IS_MAC
from ui.results_view import human_size
from ui.workers import Task, load_full, load_reduced

log = logging.getLogger(__name__)

MIN_SCALE, MAX_SCALE = 0.02, 32.0
ZOOM_STEP = 1.25


class ViewState(QObject):
    """Zoom/pan shared by the canvases (keeps side-by-side views synchronized)."""

    changed = Signal()

    def __init__(self) -> None:
        super().__init__()
        self.size = (1, 1)  # image size in output pixels
        self.fit = True
        self.scale = 1.0
        self.cx = self.cy = 0.5

    def reset(self, size: tuple[int, int]) -> None:
        self.size = (max(1, size[0]), max(1, size[1]))
        self.fit = True
        self.cx, self.cy = self.size[0] / 2, self.size[1] / 2
        self.changed.emit()

    def fit_scale(self, view: QSize) -> float:
        w, h = self.size
        return min(view.width() / w, view.height() / h, 1.0)

    def effective(self, view: QSize) -> tuple[float, float, float]:
        if self.fit:
            return self.fit_scale(view), self.size[0] / 2, self.size[1] / 2
        return self.scale, self.cx, self.cy

    def _clamp(self) -> None:
        self.cx = min(max(self.cx, 0.0), float(self.size[0]))
        self.cy = min(max(self.cy, 0.0), float(self.size[1]))

    def set_fit(self) -> None:
        self.fit = True
        self.changed.emit()

    def set_scale(self, scale: float, view: QSize, anchor: QPointF | None = None) -> None:
        k, cx, cy = self.effective(view)
        new = min(max(scale, MIN_SCALE), MAX_SCALE)
        if anchor is None:
            anchor = QPointF(view.width() / 2, view.height() / 2)
        # keep the image point under the anchor fixed
        px = cx + (anchor.x() - view.width() / 2) / k
        py = cy + (anchor.y() - view.height() / 2) / k
        self.cx = px - (anchor.x() - view.width() / 2) / new
        self.cy = py - (anchor.y() - view.height() / 2) / new
        self.scale = new
        self.fit = False
        self._clamp()
        self.changed.emit()

    def zoom(self, factor: float, view: QSize, anchor: QPointF | None = None) -> None:
        self.set_scale(self.effective(view)[0] * factor, view, anchor)

    def pan(self, dx: float, dy: float, view: QSize) -> None:
        k, cx, cy = self.effective(view)
        if self.fit:
            self.scale, self.fit = k, False
        self.cx = cx - dx / k
        self.cy = cy - dy / k
        self._clamp()
        self.changed.emit()


class Canvas(QWidget):
    """Draws a (possibly reduced) image into the output-size coordinate system."""

    def __init__(self, state: ViewState, caption: str = '', parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Canvas')
        self.state = state
        self.caption = caption
        self.pixmap: QPixmap | None = None
        self.message = ''
        self.setMouseTracking(True)
        self.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Expanding)
        self.setMinimumSize(200, 150)
        self._drag: QPointF | None = None
        state.changed.connect(self.update)

    def set_pixmap(self, pm: QPixmap | None, message: str = '') -> None:
        self.pixmap = pm
        self.message = message
        self.update()

    def paintEvent(self, _e: QPaintEvent) -> None:  # noqa: N802
        p = QPainter(self)
        p.fillRect(self.rect(), QColor('#070b16'))
        if self.pixmap is None or self.pixmap.isNull():
            p.setPen(QColor('#8d9bb8'))
            p.drawText(self.rect(), Qt.AlignmentFlag.AlignCenter, self.message or tr('Loading…'))
            return
        k, cx, cy = self.state.effective(self.size())
        w, h = self.state.size
        x0 = self.width() / 2 - cx * k
        y0 = self.height() / 2 - cy * k
        target = QRectF(x0, y0, w * k, h * k)
        visible = target.intersected(QRectF(self.rect()))
        if not visible.isEmpty():
            sx = self.pixmap.width() / w
            sy = self.pixmap.height() / h
            src = QRectF(
                (visible.left() - x0) / k * sx,
                (visible.top() - y0) / k * sy,
                visible.width() / k * sx,
                visible.height() / k * sy,
            )
            p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, k * sx < 2.0)
            p.drawPixmap(visible, self.pixmap, src)
        if self.caption:
            p.setPen(QColor('#e7ecf6'))
            box = QRectF(8, 8, max(90, p.fontMetrics().horizontalAdvance(self.caption) + 20), 24)
            p.fillRect(box, QColor(0, 0, 0, 150))
            p.drawText(box, Qt.AlignmentFlag.AlignCenter, self.caption)

    def wheelEvent(self, e: QWheelEvent) -> None:  # noqa: N802
        steps = e.angleDelta().y() / 120
        if steps:
            self.state.zoom(ZOOM_STEP**steps, self.size(), e.position())

    def mouseDoubleClickEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        if self.state.fit:
            self.state.set_scale(1.0, self.size(), e.position())
        else:
            self.state.set_fit()

    def mousePressEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        if e.button() == Qt.MouseButton.LeftButton:
            self._drag = e.position()
            self.setCursor(Qt.CursorShape.ClosedHandCursor)

    def mouseMoveEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        if self._drag is not None:
            d = e.position() - self._drag
            self._drag = e.position()
            self.state.pan(d.x(), d.y(), self.size())

    def mouseReleaseEvent(self, _e: QMouseEvent) -> None:  # noqa: N802
        self._drag = None
        self.unsetCursor()


def _button(text: str, tip: str = '') -> QPushButton:
    b = QPushButton(text)
    b.setFocusPolicy(Qt.FocusPolicy.NoFocus)
    if tip:
        b.setToolTip(tip)
    return b


class ImageViewer(QWidget):
    """Separate, maximized window. ``closed`` carries the last viewed FileResult."""

    closed = Signal(object)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent, Qt.WindowType.Window)
        self.setObjectName('ViewerRoot')
        self.setWindowTitle('Photo Batch Editor — ' + tr('Viewer'))
        self.setStyleSheet('QWidget#ViewerRoot { background-color: #070b16; }')
        self.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        self.items: list[FileResult] = []
        self.index = 0
        self.state = ViewState()
        self.after = Canvas(self.state)
        self.before = Canvas(self.state, tr('Before'))
        self.after_side = Canvas(self.state, tr('After'))
        self.before.hide()
        self.after_side.hide()
        self.pool = QThreadPool(self)
        self.pool.setMaxThreadCount(3)
        self._screen_cache: OrderedDict[Path, tuple[QPixmap, tuple[int, int]]] = OrderedDict()
        self._full_cache: OrderedDict[Path, QPixmap] = OrderedDict()
        self._pending: set[tuple[str, Path]] = set()
        self._holding = False
        self._side = False

        self.title = QLabel()
        self.title.setObjectName('Heading')
        self.counter = QLabel()
        self.counter.setProperty('secondary', True)
        close = _button('✕', tr('Close (Esc)'))
        close.setObjectName('Flat')
        top = QHBoxLayout()
        top.addWidget(self.title)
        top.addStretch(1)
        top.addWidget(self.counter)
        top.addSpacing(16)
        top.addWidget(close)

        self.prev_btn = _button('◀', tr('Previous (←)'))
        self.next_btn = _button('▶', tr('Next (→)'))
        for b in (self.prev_btn, self.next_btn):
            b.setFixedWidth(44)
            b.setMinimumHeight(120)
        mid = QHBoxLayout()
        mid.addWidget(self.prev_btn)
        mid.addWidget(self.before, 1)
        mid.addWidget(self.after, 1)
        mid.addWidget(self.after_side, 1)
        mid.addWidget(self.next_btn)

        self.info = QLabel()
        self.info.setProperty('secondary', True)
        self.info.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        self.info.setSizePolicy(QSizePolicy.Policy.Ignored, QSizePolicy.Policy.Fixed)
        self.fit_btn = _button(tr('Fit'), tr('Fit to window (0)'))
        self.one_btn = _button('100%', tr('Actual pixels (1)'))
        self.hold_btn = _button(tr('Hold: Before'), tr('Hold to show the original (or hold \\)'))
        self.side_btn = _button(tr('Side by Side'), tr('Original left, edited right (C)'))
        self.side_btn.setCheckable(True)
        self.explorer_btn = _button(tr('Show in Finder') if IS_MAC else tr('Show in Explorer'))
        bottom = QHBoxLayout()
        for b in (self.fit_btn, self.one_btn, self.hold_btn, self.side_btn, self.explorer_btn):
            bottom.addWidget(b)
        bottom.addStretch(1)

        lay = QVBoxLayout(self)
        lay.setContentsMargins(10, 6, 10, 8)
        lay.addLayout(top)
        lay.addLayout(mid, 1)
        lay.addWidget(self.info)
        lay.addLayout(bottom)

        close.clicked.connect(self.close)
        self.prev_btn.clicked.connect(lambda: self.go(-1))
        self.next_btn.clicked.connect(lambda: self.go(1))
        self.fit_btn.clicked.connect(self.state.set_fit)
        self.one_btn.clicked.connect(lambda: self.state.set_scale(1.0, self.after.size()))
        self.hold_btn.pressed.connect(lambda: self.set_holding(True))
        self.hold_btn.released.connect(lambda: self.set_holding(False))
        self.side_btn.toggled.connect(self.set_side_by_side)
        self.explorer_btn.clicked.connect(self._show_in_explorer)

    # Navigation ---------------------------------------------------------------------------
    def open(self, items: list[FileResult], index: int) -> None:
        self.items = items
        self.showMaximized()
        self.raise_()
        self.activateWindow()
        self.setFocus()
        self.show_index(index)

    def current(self) -> FileResult | None:
        return self.items[self.index] if self.items else None

    def go(self, delta: int) -> None:
        if self.items:
            self.show_index(max(0, min(len(self.items) - 1, self.index + delta)))

    def show_index(self, index: int) -> None:
        if not self.items:
            return
        self.index = index
        r = self.items[index]
        self.title.setText(r.output_path.name)
        self.counter.setText(f'{index + 1} / {len(self.items)}')
        self.prev_btn.setEnabled(index > 0)
        self.next_btn.setEnabled(index < len(self.items) - 1)
        self.info.setText(f'{r.width}×{r.height} · {human_size(r.size_bytes)} · {r.output_path}')
        self.state.reset((r.width or 1, r.height or 1))
        self._refresh()
        for d in (1, -1):  # preload neighbours
            j = index + d
            out = self.items[j].output_path if 0 <= j < len(self.items) else None
            if out is not None:
                self._load_screen(out)

    # Loading ------------------------------------------------------------------------------
    def _screen_box(self) -> tuple[int, int]:
        screen = self.screen() or QGuiApplication.primaryScreen()
        g = screen.availableGeometry()
        ratio = screen.devicePixelRatio()
        return int(g.width() * ratio), int(g.height() * ratio)

    def _load_screen(self, path: Path) -> None:
        key = ('screen', path)
        if path in self._screen_cache or key in self._pending:
            return
        self._pending.add(key)
        w, h = self._screen_box()
        Task.submit(
            self.pool,
            lambda: load_reduced(path, w, h),
            lambda res: self._screen_ready(path, res),
            lambda err: self._load_failed(key, err),
            priority=2,
        )

    def _load_full(self, path: Path) -> None:
        key = ('full', path)
        if path in self._full_cache or key in self._pending:
            return
        self._pending.add(key)
        Task.submit(
            self.pool,
            lambda: load_full(path),
            lambda img: self._full_ready(path, img),
            lambda err: self._load_failed(key, err),
            priority=1,
        )

    def _screen_ready(self, path: Path, res: tuple[QImage, tuple[int, int]]) -> None:
        self._pending.discard(('screen', path))
        self._screen_cache[path] = (QPixmap.fromImage(res[0]), res[1])
        while len(self._screen_cache) > 10:
            self._screen_cache.popitem(last=False)
        self._refresh()

    def _full_ready(self, path: Path, img: QImage) -> None:
        self._pending.discard(('full', path))
        self._full_cache[path] = QPixmap.fromImage(img)
        while len(self._full_cache) > 4:
            self._full_cache.popitem(last=False)
        self._refresh()

    def _load_failed(self, key: tuple[str, Path], err: str) -> None:
        self._pending.discard(key)
        log.warning('Viewer could not load %s: %s', key[1], err)
        for c in (self.after, self.after_side, self.before):
            if c.pixmap is None:
                c.set_pixmap(None, tr('Cannot open {name}: {error}', name=key[1].name, error=err))

    def _pixmap_for(self, path: Path, full: bool = True) -> QPixmap | None:
        if path in self._full_cache:
            return self._full_cache[path]
        if path in self._screen_cache:
            if full:
                self._load_full(path)  # full size in the background for 100% zoom
            return self._screen_cache[path][0]
        self._load_screen(path)
        return None

    def _refresh(self) -> None:
        r = self.current()
        if r is None:
            return
        after = self._pixmap_for(r.output_path) if r.output_path else None
        need_before = self._side or self._holding
        # the original is preloaded at screen size; full size only once it is shown
        before = (
            self._pixmap_for(r.input_path, full=need_before) if (need_before or after is not None) else None
        )
        if self._side:
            self.before.set_pixmap(before)
            self.after_side.set_pixmap(after)
        else:
            self.after.set_pixmap(before if (self._holding and before is not None) else after)
            self.after.caption = tr('Before') if self._holding else ''
            self.after.update()

    # Before / after -------------------------------------------------------------------------
    def set_holding(self, on: bool) -> None:
        self._holding = on
        self._refresh()

    def set_side_by_side(self, on: bool) -> None:
        self._side = on
        if self.side_btn.isChecked() != on:
            self.side_btn.setChecked(on)
        self.after.setVisible(not on)
        self.before.setVisible(on)
        self.after_side.setVisible(on)
        self._refresh()

    def _show_in_explorer(self) -> None:
        r = self.current()
        if r and r.output_path:
            open_in_file_manager(r.output_path, select=True)

    # Keyboard -----------------------------------------------------------------------------
    def _view_size(self) -> QSize:
        return (self.after_side if self._side else self.after).size()

    def keyPressEvent(self, e: QKeyEvent) -> None:  # noqa: N802
        key = e.key()
        if key == Qt.Key.Key_Backslash:
            if not e.isAutoRepeat():
                self.set_holding(True)
        elif key == Qt.Key.Key_Left:
            self.go(-1)
        elif key == Qt.Key.Key_Right:
            self.go(1)
        elif key == Qt.Key.Key_0:
            self.state.set_fit()
        elif key == Qt.Key.Key_1:
            self.state.set_scale(1.0, self._view_size())
        elif key in (Qt.Key.Key_Plus, Qt.Key.Key_Equal):
            self.state.zoom(ZOOM_STEP, self._view_size())
        elif key in (Qt.Key.Key_Minus, Qt.Key.Key_Underscore):
            self.state.zoom(1 / ZOOM_STEP, self._view_size())
        elif key == Qt.Key.Key_C:
            self.set_side_by_side(not self._side)
        elif key == Qt.Key.Key_Escape:
            self.close()
        else:
            super().keyPressEvent(e)

    def keyReleaseEvent(self, e: QKeyEvent) -> None:  # noqa: N802
        if e.key() == Qt.Key.Key_Backslash and not e.isAutoRepeat():
            self.set_holding(False)
        else:
            super().keyReleaseEvent(e)

    def closeEvent(self, e: QCloseEvent) -> None:  # noqa: N802
        self._holding = False
        self.closed.emit(self.current())
        self._full_cache.clear()
        super().closeEvent(e)
