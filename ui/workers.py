"""Background work for the UI: GPU detection, batch, thumbnails, image loading, preview."""

from __future__ import annotations

import logging
import threading
from collections.abc import Callable
from pathlib import Path
from typing import Any

import cv2
import numpy as np
from PIL import Image, ImageOps
from PySide6.QtCore import QObject, QRunnable, QThread, QThreadPool, Signal
from PySide6.QtGui import QImage

from core.backend import detect_gpu
from core.batch import run_batch
from core.io_utils import read_image
from core.settings import AdjustmentSettings

log = logging.getLogger(__name__)


# Image helpers (thread-safe: they only create QImage, never QPixmap) ---------------------


def rgb_to_qimage(arr: np.ndarray) -> QImage:
    """(H, W, 3) uint8 RGB → QImage (deep copy)."""
    arr = np.ascontiguousarray(arr, dtype=np.uint8)
    h, w = arr.shape[:2]
    return QImage(arr.data, w, h, 3 * w, QImage.Format.Format_RGB888).copy()


def to_display_u8(pixels: np.ndarray) -> np.ndarray:
    if pixels.dtype == np.uint8:
        return pixels
    if pixels.dtype == np.uint16:
        return (pixels >> 8).astype(np.uint8)
    return np.rint(np.clip(pixels, 0, 1) * 255).astype(np.uint8)


def load_reduced(path: Path, max_w: int, max_h: int) -> tuple[QImage, tuple[int, int]]:
    """Fast reduced-size decode (JPEG ``draft``), EXIF-oriented.

    Returns ``(image, full_size)`` where ``full_size`` is the oriented pixel size of the file.
    """
    try:
        with Image.open(path) as im:
            full = im.size
            orientation = im.getexif().get(0x0112, 1)
            if orientation in (5, 6, 7, 8):
                full = (full[1], full[0])
                box = (max_h, max_w)
            else:
                box = (max_w, max_h)
            if im.mode.startswith(('I', 'F')):
                raise ValueError('high bit depth')  # Pillow would clip: use the core reader
            if im.format == 'JPEG':
                im.draft('RGB', box)
            im = ImageOps.exif_transpose(im)
            if 'A' in im.getbands() or (im.mode == 'P' and 'transparency' in im.info):
                rgba = im.convert('RGBA')
                bg = Image.new('RGB', im.size, (255, 255, 255))
                bg.paste(rgba, mask=rgba.getchannel('A'))
                im = bg
            im = im.convert('RGB')
            im.thumbnail((max_w, max_h), Image.Resampling.BILINEAR)
            return rgb_to_qimage(np.asarray(im)), full
    except Exception:  # noqa: BLE001 - 16-bit TIFF and other formats Pillow cannot convert
        loaded = read_image(path)
        px = to_display_u8(loaded.pixels)
        h, w = px.shape[:2]
        k = min(1.0, max_w / w, max_h / h)
        if k < 1:
            px = cv2.resize(px, (max(1, int(w * k)), max(1, int(h * k))), interpolation=cv2.INTER_AREA)
        return rgb_to_qimage(px), (w, h)


def load_full(path: Path) -> QImage:
    """Full-resolution, EXIF-oriented display image."""
    return load_reduced(path, 1 << 30, 1 << 30)[0]


# Generic thread-pool task -------------------------------------------------------------------


class _TaskSignals(QObject):
    done = Signal(object)
    failed = Signal(str)


class Task(QRunnable):
    """Run ``fn()`` on a pool thread; ``on_done(result)`` is called on the GUI thread."""

    _alive: set[Task] = set()
    _lock = threading.Lock()

    def __init__(
        self,
        fn: Callable[[], Any],
        on_done: Callable[[Any], None] | None = None,
        on_error: Callable[[str], object] | None = None,
    ) -> None:
        super().__init__()
        self.setAutoDelete(False)
        self.fn = fn
        self.signals = _TaskSignals()
        if on_done:
            self.signals.done.connect(on_done)
        if on_error:
            self.signals.failed.connect(on_error)

    def run(self) -> None:
        try:
            result = self.fn()
        except Exception as exc:  # noqa: BLE001
            log.debug('Background task failed', exc_info=True)
            self.signals.failed.emit(str(exc))
        else:
            self.signals.done.emit(result)
        finally:
            with Task._lock:
                Task._alive.discard(self)

    @classmethod
    def submit(
        cls,
        pool: QThreadPool,
        fn: Callable[[], Any],
        on_done: Callable[[Any], None] | None = None,
        on_error: Callable[[str], object] | None = None,
        priority: int = 0,
    ) -> Task:
        task = cls(fn, on_done, on_error)
        with cls._lock:
            cls._alive.add(task)  # keep Python wrapper and signals alive until finished
        pool.start(task, priority)
        return task


# Long-running workers --------------------------------------------------------------------


class GpuDetectWorker(QThread):
    """Detects the GPU at startup without blocking the window (section 10.3)."""

    detected = Signal(object)  # GpuInfo

    def run(self) -> None:
        try:
            info = detect_gpu()
        except Exception as exc:  # noqa: BLE001
            from core.backend import GpuInfo  # noqa: PLC0415

            info = GpuInfo(False, reason=f'Could not initialize CUDA: {exc}')
        self.detected.emit(info)


class BatchWorker(QThread):
    """Runs :func:`core.batch.run_batch` and turns its callbacks into Qt signals."""

    progress = Signal(int, int, str, object)  # done, total, current file, FileResult | None
    device_changed = Signal(str)
    finished_report = Signal(object)  # BatchReport
    failed = Signal(str)

    def __init__(
        self,
        input_dir: Path,
        settings: AdjustmentSettings,
        device: str,
        output_dir: Path,
        files: list[Path] | None = None,
    ) -> None:
        super().__init__()
        self.input_dir = input_dir
        self.settings = settings.copy()
        self.device = device
        self.output_dir = output_dir
        self.files = files
        self._cancel = threading.Event()

    def cancel(self) -> None:
        self._cancel.set()

    @property
    def cancelling(self) -> bool:
        return self._cancel.is_set()

    def run(self) -> None:
        try:
            report = run_batch(
                self.input_dir,
                self.settings,
                self.device,
                self.output_dir,
                on_progress=lambda d, t, c, r: self.progress.emit(d, t, c, r),
                cancel_event=self._cancel,
                on_device_change=self.device_changed.emit,
                files=self.files,
            )
        except Exception as exc:  # noqa: BLE001
            log.exception('Batch failed')
            self.failed.emit(str(exc))
            return
        self.finished_report.emit(report)
