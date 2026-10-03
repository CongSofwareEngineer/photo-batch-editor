"""Synthetic sample images generated with NumPy (no real photos needed)."""

from __future__ import annotations

import sys
from pathlib import Path

import cv2
import numpy as np
import pytest
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from core.backend import detect_gpu, get_cpu_backend  # noqa: E402

_GPU_AVAILABLE: bool | None = None


def gpu_available() -> bool:
    global _GPU_AVAILABLE
    if _GPU_AVAILABLE is None:
        _GPU_AVAILABLE = detect_gpu().available
    return _GPU_AVAILABLE


def pytest_collection_modifyitems(config, items):  # noqa: ANN001
    gpu_items = [i for i in items if 'gpu' in i.keywords]
    if gpu_items and not gpu_available():
        skip = pytest.mark.skip(reason='no usable NVIDIA GPU')
        for item in gpu_items:
            item.add_marker(skip)


# Image factories ---------------------------------------------------------------------------


def make_gradient(h: int = 120, w: int = 160) -> np.ndarray:
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    r = xx / (w - 1)
    g = yy / (h - 1)
    b = 1.0 - (r + g) / 2
    return np.stack([r, g, b], axis=2).astype(np.float32)


def make_noise(h: int = 120, w: int = 160, sigma: float = 0.08, seed: int = 0) -> np.ndarray:
    rng = np.random.default_rng(seed)
    base = np.full((h, w, 3), 0.5, np.float32)
    return np.clip(base + rng.normal(0, sigma, base.shape), 0, 1).astype(np.float32)


def make_dark_bright(h: int = 120, w: int = 160) -> np.ndarray:
    img = np.empty((h, w, 3), np.float32)
    img[:, : w // 2] = 0.08
    img[:, w // 2 :] = 0.92
    img[..., 0] *= 0.9
    return img


def make_photo_like(h: int = 120, w: int = 160, seed: int = 3) -> np.ndarray:
    """Gradient + shapes + mild noise: exercises every adjustment."""
    img = make_gradient(h, w)
    cv2.circle(img, (w // 3, h // 2), min(h, w) // 5, (0.9, 0.2, 0.1), -1)
    cv2.rectangle(img, (w // 2, h // 4), (w - 10, h // 2), (0.1, 0.1, 0.15), -1)
    rng = np.random.default_rng(seed)
    return np.clip(img + rng.normal(0, 0.02, img.shape), 0, 1).astype(np.float32)


def to_u8(img: np.ndarray) -> np.ndarray:
    return np.rint(np.clip(img, 0, 1) * 255).astype(np.uint8)


def save_png(path: Path, rgb_u8: np.ndarray) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(rgb_u8).save(path)
    return path


def save_jpeg_with_orientation(path: Path, rgb_u8: np.ndarray, orientation: int) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    im = Image.fromarray(rgb_u8)
    exif = Image.Exif()
    exif[0x0112] = orientation
    exif[0x010F] = 'TestCam'  # Make
    exif_ifd = exif.get_ifd(0x8769)
    exif_ifd[0xA002] = rgb_u8.shape[1]
    exif_ifd[0xA003] = rgb_u8.shape[0]
    im.save(path, 'JPEG', quality=95, exif=exif.tobytes())
    return path


def save_png_alpha(path: Path, h: int = 40, w: int = 50) -> Path:
    rgba = np.zeros((h, w, 4), np.uint8)
    rgba[..., 0] = 255  # red
    rgba[:, : w // 2, 3] = 0  # left half fully transparent → white
    rgba[:, w // 2 :, 3] = 255
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(rgba, 'RGBA').save(path)
    return path


def save_tiff16(path: Path, h: int = 40, w: int = 50) -> Path:
    arr = np.zeros((h, w, 3), np.uint16)
    arr[..., 0] = 65535
    arr[..., 1] = 32768
    arr[..., 2] = 1000
    path.parent.mkdir(parents=True, exist_ok=True)
    ok, buf = cv2.imencode('.tiff', arr[..., ::-1])  # BGR for OpenCV
    assert ok
    buf.tofile(str(path))
    return path


def save_gray(path: Path, h: int = 40, w: int = 50) -> Path:
    arr = np.tile(np.linspace(0, 255, w, dtype=np.uint8), (h, 1))
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr, 'L').save(path)
    return path


@pytest.fixture
def cpu():
    return get_cpu_backend()


@pytest.fixture
def photo_folder(tmp_path: Path) -> Path:
    """A client folder with 10 photos of mixed formats + a sub-folder."""
    folder = tmp_path / 'clientA'
    for i in range(6):
        save_png(folder / f'IMG_{i:03d}.png', to_u8(make_photo_like(60 + i * 4, 80, seed=i)))
    save_jpeg_with_orientation(folder / 'rotated.jpg', to_u8(make_photo_like(60, 90)), 6)
    save_png_alpha(folder / 'alpha.png')
    save_tiff16(folder / 'outdoor' / 'ảnh cưới 01.tif')
    save_gray(folder / 'outdoor' / 'gray.bmp')
    return folder


# Qt (photo / video editor tests) -----------------------------------------------------------


@pytest.fixture(scope='session')
def qapp():
    """A QApplication on the offscreen platform (no window appears)."""
    import os  # noqa: PLC0415

    os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
    if sys.platform == 'win32':
        os.environ.setdefault('QT_QPA_FONTDIR', 'C:/Windows/Fonts')
    from PySide6.QtWidgets import QApplication  # noqa: PLC0415

    return QApplication.instance() or QApplication([])


def pump(seconds: float = 0.1) -> None:
    """Process Qt events while letting worker threads run (QTest.qWait starves them)."""
    import time  # noqa: PLC0415

    from PySide6.QtWidgets import QApplication  # noqa: PLC0415

    end = time.monotonic() + seconds
    while time.monotonic() < end:
        QApplication.processEvents()
        time.sleep(0.01)
