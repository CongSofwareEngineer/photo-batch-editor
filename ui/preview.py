"""Live preview before running (spec section 8.8)."""

from __future__ import annotations

import logging
import time
from collections import OrderedDict
from pathlib import Path

import cv2
import numpy as np
from PySide6.QtCore import QPointF, QRectF, Qt, QThreadPool, QTimer, Signal
from PySide6.QtGui import QColor, QImage, QMouseEvent, QPainter, QPaintEvent, QPen, QPixmap
from PySide6.QtWidgets import QDialog, QHBoxLayout, QLabel, QPushButton, QScrollArea, QVBoxLayout, QWidget

from core import adjustments as adj
from core.backend import get_backend, get_cpu_backend
from core.enhance import get_shared_resolver
from core.i18n import tr
from core.io_utils import read_image
from core.pipeline import apply_adjustments, apply_adjustments_planar
from core.settings import AdjustmentSettings
from ui.workers import Task, rgb_to_qimage, to_display_u8

log = logging.getLogger(__name__)

PREVIEW_LONG_EDGE = 1600
DEBOUNCE_MS = 80
SR_CROP = 384
CACHE_SIZE = 6


def load_preview_source(path: Path) -> np.ndarray:
    """Read and downscale to a long edge of at most 1600 px (keeps uint8/uint16)."""
    px = read_image(path).pixels
    h, w = px.shape[:2]
    k = PREVIEW_LONG_EDGE / max(h, w)
    if k < 1:
        px = cv2.resize(px, (max(1, round(w * k)), max(1, round(h * k))), interpolation=cv2.INTER_AREA)
    return np.ascontiguousarray(px)


def render_preview(src: np.ndarray, s: AdjustmentSettings, device: str) -> tuple[QImage, float, str]:
    """Adjustments 1–15 only (SR and Image Size change the size and are not previewed)."""
    t0 = time.perf_counter()
    backend = get_backend(device)
    try:
        p = apply_adjustments_planar(backend.to_device(src), s, backend)
        out = backend.to_host(adj.planar_to_uint8(p, backend))
    except Exception:  # noqa: BLE001 - e.g. GPU out of memory: fall back to the CPU
        if backend.name == 'cpu':
            raise
        log.warning('Preview on GPU failed, using CPU', exc_info=True)
        backend = get_cpu_backend()
        out = adj.planar_to_uint8(apply_adjustments_planar(src, s, backend), backend)
    return rgb_to_qimage(out), (time.perf_counter() - t0) * 1000, backend.name


class ImageView(QWidget):
    """Draws an image fitted to the widget; reports clicks in normalized image coordinates."""

    clicked = Signal(float, float)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setMinimumSize(200, 150)
        self._pixmap: QPixmap | None = None
        self._marker: tuple[float, float] | None = None
        self._message = tr('Choose a folder to see a preview')

    def set_image(self, img: QImage | None) -> None:
        self._pixmap = QPixmap.fromImage(img) if img is not None else None
        self.update()

    def set_message(self, text: str) -> None:
        self._message = text
        self.update()

    def set_marker(self, pos: tuple[float, float] | None) -> None:
        self._marker = pos
        self.update()

    def _target(self) -> QRectF | None:
        if self._pixmap is None or self._pixmap.isNull():
            return None
        w, h = self._pixmap.width(), self._pixmap.height()
        k = min(self.width() / w, self.height() / h)
        tw, th = w * k, h * k
        return QRectF((self.width() - tw) / 2, (self.height() - th) / 2, tw, th)

    def paintEvent(self, _e: QPaintEvent) -> None:  # noqa: N802
        p = QPainter(self)
        p.fillRect(self.rect(), QColor('#0a0f1d'))
        target = self._target()
        if target is None or self._pixmap is None:
            p.setPen(QColor('#6f80a3'))
            p.drawText(self.rect(), Qt.AlignmentFlag.AlignCenter, self._message)
            return
        p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform)
        p.drawPixmap(target, self._pixmap, QRectF(self._pixmap.rect()))
        if self._marker is not None:
            x = target.left() + self._marker[0] * target.width()
            y = target.top() + self._marker[1] * target.height()
            p.setPen(QPen(QColor('#22d3ee'), 2))
            p.drawEllipse(QPointF(x, y), 7, 7)

    def mousePressEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        target = self._target()
        if target is not None and target.contains(e.position()):
            nx = (e.position().x() - target.left()) / target.width()
            ny = (e.position().y() - target.top()) / target.height()
            self.clicked.emit(nx, ny)


class SrCompareDialog(QDialog):
    """Original crop (Bicubic) vs Super Resolution, both at 100%."""

    def __init__(self, bicubic: QImage, sr: QImage, factor: int, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle(tr('Super Resolution {f}x — 100% comparison', f=factor))
        self.resize(1300, 760)
        lay = QHBoxLayout(self)
        for title, img in ((f'Bicubic {factor}x', bicubic), (tr('Super Resolution') + f' {factor}x', sr)):
            col = QVBoxLayout()
            t = QLabel(title)
            t.setObjectName('Heading')
            area = QScrollArea()
            lab = QLabel()
            lab.setPixmap(QPixmap.fromImage(img))
            area.setWidget(lab)
            area.setAlignment(Qt.AlignmentFlag.AlignCenter)
            col.addWidget(t)
            col.addWidget(area, 1)
            lay.addLayout(col)
        bars = list(self.findChildren(QScrollArea))
        if len(bars) == 2:  # keep both crops aligned while scrolling
            for a, b in ((bars[0], bars[1]), (bars[1], bars[0])):
                a.horizontalScrollBar().valueChanged.connect(b.horizontalScrollBar().setValue)
                a.verticalScrollBar().valueChanged.connect(b.verticalScrollBar().setValue)


def sr_crop_job(
    path: Path, nx: float, ny: float, s: AdjustmentSettings, use_cuda: bool
) -> tuple[QImage, QImage]:
    px = read_image(path).pixels
    h, w = px.shape[:2]
    cx, cy = int(nx * w), int(ny * h)
    x0 = min(max(0, cx - SR_CROP // 2), max(0, w - SR_CROP))
    y0 = min(max(0, cy - SR_CROP // 2), max(0, h - SR_CROP))
    crop = np.ascontiguousarray(px[y0 : y0 + SR_CROP, x0 : x0 + SR_CROP])
    s = s.copy()
    s.vignette_amount = 0  # depends on the position in the full frame
    cpu = get_cpu_backend()
    img = apply_adjustments(crop, s, cpu, long_side=max(h, w))
    f = s.sr_factor
    sr = get_shared_resolver(use_cuda).upscale(img, f)
    bic = np.clip(cv2.resize(img, None, fx=f, fy=f, interpolation=cv2.INTER_CUBIC), 0, 1)
    return rgb_to_qimage(to_display_u8(bic)), rgb_to_qimage(to_display_u8(sr))


class PreviewPane(QWidget):
    """Preview area with debounced background rendering; stale renders are discarded."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('PreviewPane')
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        self.view = ImageView()
        self.title = QLabel(tr('LIVE PREVIEW'))
        self.title.setObjectName('SectionTitle')
        self.status = QLabel('')
        self.status.setProperty('secondary', True)
        self.sr_btn = QPushButton(tr('Preview Super Resolution (100%)'))
        self.sr_btn.setToolTip(tr('Runs Super Resolution on a 384×384 crop around the last clicked point'))
        self.sr_btn.hide()
        top = QHBoxLayout()
        top.addWidget(self.title)
        top.addStretch(1)
        top.addWidget(self.status)
        top.addWidget(self.sr_btn)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(10, 6, 10, 10)
        lay.addLayout(top)
        lay.addWidget(self.view, 1)

        self.pool = QThreadPool(self)
        self.pool.setMaxThreadCount(1)
        self.sr_pool = QThreadPool(self)
        self.sr_pool.setMaxThreadCount(1)
        self._cache: OrderedDict[Path, np.ndarray] = OrderedDict()
        self._path: Path | None = None
        self._settings = AdjustmentSettings()
        self._device = 'cpu'
        self._sr_cuda = False
        self._gen = 0
        self._busy = False
        self._pending = False
        self._showing_original = False
        self._original: QImage | None = None
        self._rendered: QImage | None = None
        self._click: tuple[float, float] = (0.5, 0.5)
        self._timer = QTimer(self)
        self._timer.setSingleShot(True)
        self._timer.setInterval(DEBOUNCE_MS)
        self._timer.timeout.connect(self._start_render)
        self.view.clicked.connect(self._on_click)
        self.sr_btn.clicked.connect(self._run_sr_crop)

    # Inputs
    def set_device(self, device: str, sr_cuda: bool = False) -> None:
        self._device = device
        self._sr_cuda = sr_cuda
        self.request()

    def set_settings(self, s: AdjustmentSettings) -> None:
        self._settings = s.copy()
        self.sr_btn.setVisible(s.super_resolution != 'off' and self._path is not None)
        self.request()

    def set_source(self, path: Path | None) -> None:
        self._path = path
        self._gen += 1
        self._original = self._rendered = None
        self._click = (0.5, 0.5)
        self.view.set_marker(None)
        self.sr_btn.setVisible(self._settings.super_resolution != 'off' and path is not None)
        if path is None:
            self.view.set_image(None)
            self.view.set_message(tr('Choose a folder to see a preview'))
            return
        self.view.set_message(tr('Loading…'))
        if path in self._cache:
            self._cache.move_to_end(path)
            self._set_original(self._cache[path])
            self.request(immediate=True)
            return
        self.view.set_image(None)
        gen = self._gen
        Task.submit(
            self.pool,
            lambda: load_preview_source(path),
            lambda src: self._source_loaded(gen, path, src),
            lambda err: self._failed(gen, tr('Cannot open {name}: {error}', name=path.name, error=err)),
        )

    def show_original(self, on: bool) -> None:
        self._showing_original = on
        self._show()

    # Internals
    def _source_loaded(self, gen: int, path: Path, src: np.ndarray) -> None:
        self._cache[path] = src
        while len(self._cache) > CACHE_SIZE:
            self._cache.popitem(last=False)
        if gen == self._gen:
            self._set_original(src)
            self.request(immediate=True)

    def _set_original(self, src: np.ndarray) -> None:
        self._original = rgb_to_qimage(to_display_u8(src))

    def _failed(self, gen: int, msg: str) -> None:
        if gen == self._gen:
            self.view.set_image(None)
            self.view.set_message(msg)

    def request(self, immediate: bool = False) -> None:
        if self._path is None or self._path not in self._cache:
            return
        self._gen += 1
        if immediate:
            self._timer.stop()
            self._start_render()
        else:
            self._timer.start()

    def _start_render(self) -> None:
        if self._busy:
            self._pending = True
            return
        if self._path is None or self._path not in self._cache:
            return
        self._busy = True
        gen, src, s, device = self._gen, self._cache[self._path], self._settings.copy(), self._device
        Task.submit(
            self.pool,
            lambda: render_preview(src, s, device),
            lambda res: self._rendered_ready(gen, res),
            lambda err: self._render_failed(gen, err),
        )

    def _render_done(self) -> None:
        self._busy = False
        if self._pending:
            self._pending = False
            self._start_render()

    def _rendered_ready(self, gen: int, res: tuple[QImage, float, str]) -> None:
        img, ms, dev = res
        if gen == self._gen:  # otherwise stale: a newer request is pending
            self._rendered = img
            self.status.setText(
                f'{img.width()}×{img.height()} · {ms:.0f} ms · {"⚡ GPU" if dev == "gpu" else "CPU"}'
            )
            self._show()
        self._render_done()

    def _render_failed(self, gen: int, err: str) -> None:
        if gen == self._gen:
            self.status.setText(tr('Preview failed: {error}', error=err))
        self._render_done()

    def _show(self) -> None:
        img = self._original if (self._showing_original or self._rendered is None) else self._rendered
        if img is not None:
            self.view.set_image(img)
        self.title.setText(tr('ORIGINAL') if self._showing_original else tr('LIVE PREVIEW'))

    def _on_click(self, nx: float, ny: float) -> None:
        self._click = (nx, ny)
        if self._settings.super_resolution != 'off':
            self.view.set_marker(self._click)

    def _run_sr_crop(self) -> None:
        if self._path is None or self._settings.super_resolution == 'off':
            return
        self.sr_btn.setEnabled(False)
        self.sr_btn.setText(tr('Running Super Resolution…'))
        path, (nx, ny), s, cuda = self._path, self._click, self._settings.copy(), self._sr_cuda
        Task.submit(
            self.sr_pool,
            lambda: sr_crop_job(path, nx, ny, s, cuda),
            lambda res: self._sr_done(res, s.sr_factor),
            self._sr_failed,
        )

    def _sr_reset_button(self) -> None:
        self.sr_btn.setEnabled(True)
        self.sr_btn.setText(tr('Preview Super Resolution (100%)'))

    def _sr_done(self, res: tuple[QImage, QImage], factor: int) -> None:
        self._sr_reset_button()
        SrCompareDialog(res[0], res[1], factor, self.window()).exec()

    def _sr_failed(self, err: str) -> None:
        self._sr_reset_button()
        from PySide6.QtWidgets import QMessageBox  # noqa: PLC0415

        QMessageBox.warning(
            self.window(), tr('Super Resolution'), tr('Super Resolution preview failed:\n{error}', error=err)
        )
