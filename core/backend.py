"""GPU/CPU backend selection, NVIDIA GPU detection and shared blur helpers (spec section 10).

Only this module may import CuPy, and only inside functions, so the app runs on
machines without an NVIDIA GPU.
"""

from __future__ import annotations

import logging
import math
import os
import subprocess
import sys
import threading
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType
from typing import Any

import cv2
import numpy as np

from core.paths import add_cuda_dll_dirs, local_appdata_dir

log = logging.getLogger(__name__)

# Above this sigma (px) blurs run on a downscaled copy, identically on both backends.
LARGE_SIGMA = 8.0
SMALL_SIGMA_TARGET = 4.0
MIN_DRIVER_VERSION = 12000  # CUDA 12


def gaussian_radius(sigma: float) -> int:
    """Kernel radius shared by OpenCV and SciPy/CuPy (``truncate=4``)."""
    return int(4.0 * sigma + 0.5)


def _downscale_factor(sigma: float) -> int:
    return max(2, int(math.ceil(sigma / SMALL_SIGMA_TARGET)))


def _resize_linear_xp(xp: ModuleType, a: Any, out_h: int, out_w: int) -> Any:
    """Bilinear resize with OpenCV ``INTER_LINEAR`` semantics (half-pixel centers, clamped)."""
    h, w = a.shape

    def coords(n_out: int, n_in: int) -> tuple[Any, Any, Any]:
        s = (xp.arange(n_out, dtype=xp.float32) + 0.5) * (n_in / n_out) - 0.5
        s = xp.clip(s, 0, n_in - 1)
        i0 = xp.floor(s).astype(xp.int32)
        i1 = xp.minimum(i0 + 1, n_in - 1)
        return i0, i1, (s - i0).astype(xp.float32)

    y0, y1, wy = coords(out_h, h)
    x0, x1, wx = coords(out_w, w)
    rows0 = a[y0]
    rows1 = a[y1]
    top = rows0[:, x0] * (1 - wx) + rows0[:, x1] * wx
    bot = rows1[:, x0] * (1 - wx) + rows1[:, x1] * wx
    return (top * (1 - wy)[:, None] + bot * wy[:, None]).astype(xp.float32)


class Backend:
    """Common interface of the CPU (NumPy) and GPU (CuPy) backends (section 10.4)."""

    name: str = 'cpu'
    xp: ModuleType = np
    device_name: str = 'CPU'

    def to_device(self, img: np.ndarray) -> Any:
        return img

    def to_host(self, arr: Any) -> np.ndarray:
        return arr

    def gaussian_blur(self, arr2d: Any, sigma: float) -> Any:
        """Gaussian blur with reflect borders; large sigmas run on a downscaled copy."""
        if sigma <= 0:
            return arr2d
        if sigma <= LARGE_SIGMA:
            return self._gaussian_native(arr2d, sigma)
        f = _downscale_factor(sigma)
        h, w = arr2d.shape
        small = self._gaussian_native(self.downscale(arr2d, f), sigma / f)
        return self.upscale(small, h, w, f)

    def blur_map(self, arr2d: Any, sigma: float, fn: Any) -> Any:
        """``fn(gaussian_blur(arr2d, sigma))`` for a pointwise ``fn``.

        When the blur runs on a downscaled copy, ``fn`` is evaluated there too and only
        its (smooth) result is upscaled: one full-resolution pass instead of many.
        """
        if sigma <= LARGE_SIGMA:
            return fn(self.gaussian_blur(arr2d, sigma))
        f = _downscale_factor(sigma)
        h, w = arr2d.shape
        return self.upscale(fn(self._gaussian_native(self.downscale(arr2d, f), sigma / f)), h, w, f)

    def downscale(self, arr2d: Any, f: int) -> Any:
        """Average ``f × f`` blocks (bottom/right padded by reflection to a multiple of ``f``)."""
        h, w = arr2d.shape
        return self._block_mean(arr2d, f, (-h) % f, (-w) % f)

    def upscale(self, small: Any, h: int, w: int, f: int) -> Any:
        """Inverse of :meth:`downscale`: bilinear ×``f``, cropped to ``h × w``."""
        sh, sw = small.shape
        return self._resize_linear(small, sh * f, sw * f)[:h, :w]

    def box_filter(self, arr2d: Any, radius: int) -> Any:
        raise NotImplementedError

    def free_memory(self) -> None:
        """Release cached device memory (no-op on CPU)."""

    def set_memory_limit(self, fraction: float | None) -> None:
        """Limit the device memory pool to a fraction of VRAM (no-op on CPU)."""

    def mem_info(self) -> tuple[int, int] | None:
        """``(free, total)`` device memory in bytes, ``None`` on CPU."""
        return None

    # Backend-specific primitives
    def _gaussian_native(self, arr2d: Any, sigma: float) -> Any:
        raise NotImplementedError

    def _block_mean(self, arr2d: Any, f: int, ph: int, pw: int) -> Any:
        raise NotImplementedError

    def _resize_linear(self, arr2d: Any, out_h: int, out_w: int) -> Any:
        raise NotImplementedError


class CpuBackend(Backend):
    """NumPy + OpenCV implementation."""

    name = 'cpu'
    xp = np

    def __init__(self) -> None:
        self.device_name = f'CPU ({os.cpu_count() or 1} threads)'

    def box_filter(self, arr2d: np.ndarray, radius: int) -> np.ndarray:
        k = 2 * int(radius) + 1
        return cv2.boxFilter(
            np.ascontiguousarray(arr2d, dtype=np.float32),
            -1,
            (k, k),
            normalize=True,
            borderType=cv2.BORDER_REFLECT,
        )

    def _gaussian_native(self, arr2d: np.ndarray, sigma: float) -> np.ndarray:
        k = 2 * gaussian_radius(sigma) + 1
        return cv2.GaussianBlur(
            np.ascontiguousarray(arr2d, dtype=np.float32),
            (k, k),
            sigmaX=sigma,
            sigmaY=sigma,
            borderType=cv2.BORDER_REFLECT,
        )

    def _block_mean(self, arr2d: np.ndarray, f: int, ph: int, pw: int) -> np.ndarray:
        a = np.ascontiguousarray(arr2d, dtype=np.float32)
        if ph or pw:
            a = cv2.copyMakeBorder(a, 0, ph, 0, pw, cv2.BORDER_REFLECT)
        h, w = a.shape
        # INTER_AREA with an exact integer factor is a block average.
        return cv2.resize(a, (w // f, h // f), interpolation=cv2.INTER_AREA)

    def _resize_linear(self, arr2d: np.ndarray, out_h: int, out_w: int) -> np.ndarray:
        return cv2.resize(arr2d, (out_w, out_h), interpolation=cv2.INTER_LINEAR)


class GpuBackend(Backend):
    """CuPy implementation (NVIDIA CUDA)."""

    name = 'gpu'

    def __init__(self, info: GpuInfo | None = None) -> None:
        import cupy  # noqa: PLC0415 - must stay inside the function (spec rule 8)
        import cupyx.scipy.ndimage as ndi  # noqa: PLC0415

        self._cp = cupy
        self._ndi = ndi
        self.xp = cupy
        self.info = info
        self.device_name = info.name if info and info.name else 'NVIDIA GPU'

    def to_device(self, img: np.ndarray) -> Any:
        return self._cp.asarray(img)

    def to_host(self, arr: Any) -> np.ndarray:
        return self._cp.asnumpy(arr)

    def box_filter(self, arr2d: Any, radius: int) -> Any:
        return self._ndi.uniform_filter(
            arr2d.astype(self._cp.float32, copy=False), size=2 * int(radius) + 1, mode='reflect'
        )

    def _gaussian_native(self, arr2d: Any, sigma: float) -> Any:
        return self._ndi.gaussian_filter(
            arr2d.astype(self._cp.float32, copy=False), sigma, mode='reflect', truncate=4.0
        )

    def _block_mean(self, arr2d: Any, f: int, ph: int, pw: int) -> Any:
        a = arr2d.astype(self._cp.float32, copy=False)
        if ph or pw:
            a = self._cp.pad(a, ((0, ph), (0, pw)), mode='symmetric')
        h, w = a.shape
        return a.reshape(h // f, f, w // f, f).mean(axis=(1, 3), dtype=self._cp.float32)

    def _resize_linear(self, arr2d: Any, out_h: int, out_w: int) -> Any:
        return _resize_linear_xp(self._cp, arr2d, out_h, out_w)

    def free_memory(self) -> None:
        try:
            self._cp.get_default_memory_pool().free_all_blocks()
            self._cp.get_default_pinned_memory_pool().free_all_blocks()
        except Exception:  # noqa: BLE001
            log.debug('free_memory failed', exc_info=True)

    def set_memory_limit(self, fraction: float | None) -> None:
        try:
            pool = self._cp.get_default_memory_pool()
            if fraction is None:
                pool.set_limit(size=0)
            else:
                pool.set_limit(fraction=fraction)
        except Exception:  # noqa: BLE001
            log.debug('set_memory_limit failed', exc_info=True)

    def mem_info(self) -> tuple[int, int] | None:
        try:
            free, total = self._cp.cuda.Device(0).mem_info
            return int(free), int(total)
        except Exception:  # noqa: BLE001
            return None

    def pool_free_bytes(self) -> int:
        """Memory held by the CuPy pool but not in use (reusable for the next photo)."""
        try:
            return int(self._cp.get_default_memory_pool().free_bytes())
        except Exception:  # noqa: BLE001
            return 0


@dataclass
class GpuInfo:
    """Result of NVIDIA GPU detection (section 10.3)."""

    available: bool
    name: str | None = None
    vram_total: int | None = None
    vram_free: int | None = None
    driver_version: str | None = None
    cuda_version: str | None = None
    sr_cuda_available: bool = False
    reason: str | None = None


_gpu_info: GpuInfo | None = None
_gpu_backend: GpuBackend | None = None
_cpu_backend: CpuBackend | None = None
_lock = threading.RLock()


def _cuda_version_str(v: int) -> str:
    return f'{v // 1000}.{(v % 1000) // 10}'


def _nvidia_driver_present() -> bool:
    """True if an NVIDIA display driver (the CUDA driver library) is installed."""
    if sys.platform == 'win32':
        system32 = Path(os.environ.get('SystemRoot', r'C:\Windows')) / 'System32'
        return (system32 / 'nvcuda.dll').exists()
    import ctypes.util  # noqa: PLC0415

    return Path('/proc/driver/nvidia').exists() or ctypes.util.find_library('cuda') is not None


NO_GPU = 'No NVIDIA GPU found on this computer.'
OLD_DRIVER = (
    'NVIDIA driver is older than CUDA 12 needs. Optional: updating the driver (NVIDIA App or nvidia.com) '
    'enables the GPU speed-up.'
)


def _nvidia_smi_driver() -> str | None:
    """Driver version string from ``nvidia-smi`` (best effort, hidden console)."""
    try:
        flags = getattr(subprocess, 'CREATE_NO_WINDOW', 0) if sys.platform == 'win32' else 0
        out = subprocess.run(
            ['nvidia-smi', '--query-gpu=driver_version', '--format=csv,noheader'],
            capture_output=True,
            text=True,
            timeout=5,
            creationflags=flags,
        )
        line = out.stdout.strip().splitlines()
        return line[0].strip() if out.returncode == 0 and line else None
    except Exception:  # noqa: BLE001
        return None


def _self_test(gpu: GpuBackend) -> str | None:
    """Run the adjustments on a 256 px image on GPU and CPU; return an error text or None."""
    from core.pipeline import apply_adjustments  # noqa: PLC0415 - avoid import cycle
    from core.settings import AdjustmentSettings  # noqa: PLC0415

    rng = np.random.default_rng(1)
    yy, xx = np.mgrid[0:256, 0:256].astype(np.float32) / 255.0
    img = np.stack([xx, yy, 1 - xx * yy], axis=2)
    img = np.clip(img + rng.normal(0, 0.03, img.shape), 0, 1).astype(np.float32)
    s = AdjustmentSettings(
        temperature=20,
        tint=-10,
        exposure=0.4,
        brightness=10,
        contrast=20,
        highlights=-30,
        shadows=30,
        whites=10,
        blacks=-10,
        clarity=30,
        vibrance=20,
        saturation=10,
        sharpening_amount=40,
        noise_reduction=40,
        vignette_amount=-30,
    )
    cpu = get_cpu_backend()
    ref = apply_adjustments(img.copy(), s, cpu)
    out = gpu.to_host(apply_adjustments(gpu.to_device(img), s, gpu))
    diff = np.abs(out.astype(np.float32) - ref)
    if diff.mean() > 1 / 255 or diff.max() > 3 / 255:
        return f'GPU self-test mismatch (mean {diff.mean() * 255:.2f}/255, max {diff.max() * 255:.2f}/255)'
    return None


def detect_gpu(force: bool = False) -> GpuInfo:
    """Detect a usable NVIDIA GPU (cached). Safe to call from a background thread."""
    global _gpu_info, _gpu_backend
    with _lock:
        if _gpu_info is not None and not force:
            return _gpu_info
        _gpu_backend = None
        info = _detect_gpu_uncached()
        _gpu_info = info
        return info


def _detect_gpu_uncached() -> GpuInfo:
    global _gpu_backend
    cache_dir = local_appdata_dir() / 'cupy_cache'
    try:
        cache_dir.mkdir(parents=True, exist_ok=True)
        os.environ.setdefault('CUPY_CACHE_DIR', str(cache_dir))
    except OSError:
        pass

    if not _nvidia_driver_present():  # also avoids the 1–3 s CuPy import on such machines
        return GpuInfo(False, reason=NO_GPU)
    add_cuda_dll_dirs()  # CUDA runtime DLLs from the nvidia-*-cu12 packages (no CUDA Toolkit)
    try:
        import cupy  # noqa: PLC0415
    except Exception as exc:  # noqa: BLE001 - ImportError or DLL load error
        log.info('CuPy import failed: %s', exc)
        return GpuInfo(False, reason=f'Could not initialize CUDA: CuPy is not available ({exc})')

    try:
        count = cupy.cuda.runtime.getDeviceCount()
    except Exception as exc:  # noqa: BLE001
        msg = str(exc)
        if 'insufficient' in msg.lower():
            return GpuInfo(False, reason=OLD_DRIVER)
        if 'no cuda-capable' in msg.lower() or 'NoDevice' in msg:
            return GpuInfo(False, reason=NO_GPU)
        return GpuInfo(False, reason=f'Could not initialize CUDA: {msg}')
    if count < 1:
        return GpuInfo(False, reason=NO_GPU)

    try:
        drv = int(cupy.cuda.runtime.driverGetVersion())
    except Exception as exc:  # noqa: BLE001
        return GpuInfo(False, reason=f'Could not initialize CUDA: {exc}')
    if drv < MIN_DRIVER_VERSION:
        return GpuInfo(False, driver_version=_cuda_version_str(drv), reason=OLD_DRIVER)

    info = GpuInfo(False)
    try:
        props = cupy.cuda.runtime.getDeviceProperties(0)
        name = props.get('name', b'NVIDIA GPU')
        info.name = name.decode(errors='replace') if isinstance(name, bytes) else str(name)
        free, total = cupy.cuda.Device(0).mem_info
        info.vram_free, info.vram_total = int(free), int(total)
        info.driver_version = _nvidia_smi_driver() or f'CUDA {_cuda_version_str(drv)}'
        try:
            info.cuda_version = _cuda_version_str(int(cupy.cuda.runtime.runtimeGetVersion()))
        except Exception:  # noqa: BLE001
            info.cuda_version = None
        gpu = GpuBackend(info)
        err = _self_test(gpu)  # also warms up / compiles the CUDA kernels
    except Exception as exc:  # noqa: BLE001
        log.warning('GPU initialisation failed', exc_info=True)
        info.reason = f'Could not initialize CUDA: {exc}'
        return info
    if err:
        info.reason = f'Could not initialize CUDA: {err}'
        return info

    try:
        from core.enhance import cuda_provider_available  # noqa: PLC0415

        info.sr_cuda_available = cuda_provider_available()
    except Exception:  # noqa: BLE001
        info.sr_cuda_available = False
    info.available = True
    _gpu_backend = gpu
    log.info('GPU ready: %s (%.1f GB)', info.name, (info.vram_total or 0) / 2**30)
    return info


def get_cached_gpu_info() -> GpuInfo | None:
    """GPU info if detection already ran, otherwise ``None``."""
    return _gpu_info


def reset_gpu_cache() -> None:
    """Forget the detection result (used by tests)."""
    global _gpu_info, _gpu_backend
    with _lock:
        _gpu_info = None
        _gpu_backend = None


def get_cpu_backend() -> CpuBackend:
    global _cpu_backend
    if _cpu_backend is None:
        _cpu_backend = CpuBackend()
    return _cpu_backend


def get_backend(preference: str) -> Backend:
    """``"gpu"`` | ``"cpu"``. If the GPU is unusable the CPU backend is returned."""
    if preference == 'gpu':
        info = detect_gpu()
        if info.available and _gpu_backend is not None:
            return _gpu_backend
    return get_cpu_backend()


def is_gpu_backend(backend: Backend) -> bool:
    return backend.name == 'gpu'
