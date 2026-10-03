"""Enhance › Super Resolution with Real-ESRGAN + ONNX Runtime (spec section 5.4).

Only this module may import ``onnxruntime``, and only inside functions.
"""

from __future__ import annotations

import logging
import threading
from pathlib import Path
from typing import Any

import cv2
import numpy as np

from core.image_size import exceeds_limits
from core.paths import add_cuda_dll_dirs, resource_path

log = logging.getLogger(__name__)

MODEL_FILENAME = 'realesr-general-x4v3.onnx'
NATIVE_SCALE = 4
OVERLAP = 16
TILE_GPU = 512
TILE_CPU = 256
GPU_MEM_LIMIT = int(1.5 * 2**30)
SR_SKIP_WARNING = 'Super Resolution skipped: output would exceed 20,000 px / 200 MP'


class SuperResolutionError(RuntimeError):
    """Super Resolution could not run (missing model, ONNX Runtime error…)."""


def default_model_path() -> Path:
    return resource_path('models', MODEL_FILENAME)


def sr_output_size(width: int, height: int, factor: int) -> tuple[int, int]:
    return width * factor, height * factor


def sr_exceeds_limits(width: int, height: int, factor: int) -> bool:
    """True if Super Resolution output would exceed the limits of section 5.6."""
    return factor > 1 and exceeds_limits(*sr_output_size(width, height, factor))


def _preload_cuda_dlls(ort: Any) -> None:
    """Load CUDA/cuDNN DLLs from the ``nvidia-*`` pip packages when available."""
    add_cuda_dll_dirs()
    preload = getattr(ort, 'preload_dlls', None)
    if preload is None:
        return
    try:
        preload()
    except Exception:  # noqa: BLE001
        log.debug('onnxruntime.preload_dlls failed', exc_info=True)


def cuda_provider_available(model_path: Path | None = None) -> bool:
    """True if ONNX Runtime can actually run the model with the CUDA provider."""
    try:
        import onnxruntime as ort  # noqa: PLC0415
    except Exception:  # noqa: BLE001
        return False
    if 'CUDAExecutionProvider' not in ort.get_available_providers():
        return False
    from core.backend import (
        _nvidia_driver_present,  # noqa: PLC0415 - does not import CuPy
    )

    if not _nvidia_driver_present():
        return False
    path = Path(model_path or default_model_path())
    if not path.is_file():
        return True  # provider is compiled in; cannot verify further without the model
    try:
        resolver = SuperResolver(use_cuda=True, model_path=path)
        resolver._get_session()
        return resolver.device == 'gpu'
    except Exception:  # noqa: BLE001
        return False


class SuperResolver:
    """Lazily created, reusable ONNX Runtime session with tiled ×2/×4 upscaling."""

    def __init__(
        self, use_cuda: bool = False, model_path: Path | None = None, tile_size: int | None = None
    ) -> None:
        self.use_cuda = use_cuda
        self.model_path = Path(model_path or default_model_path())
        self.tile_size = tile_size or (TILE_GPU if use_cuda else TILE_CPU)
        self.device = 'gpu' if use_cuda else 'cpu'
        self._session: Any = None
        self._input_name = 'input'
        self._lock = threading.Lock()

    def _get_session(self) -> Any:
        with self._lock:
            if self._session is not None:
                return self._session
            if not self.model_path.is_file():
                raise SuperResolutionError(f'Super Resolution model not found: {self.model_path}')
            try:
                import onnxruntime as ort  # noqa: PLC0415
            except Exception as exc:  # noqa: BLE001
                raise SuperResolutionError(f'ONNX Runtime is not available: {exc}') from exc
            so = ort.SessionOptions()
            so.log_severity_level = 3
            session = None
            if self.use_cuda:
                _preload_cuda_dlls(ort)
                cuda_opts = {
                    'device_id': 0,
                    'gpu_mem_limit': GPU_MEM_LIMIT,
                    'arena_extend_strategy': 'kSameAsRequested',
                    'cudnn_conv_algo_search': 'HEURISTIC',
                }
                try:
                    session = ort.InferenceSession(
                        str(self.model_path),
                        sess_options=so,
                        providers=[('CUDAExecutionProvider', cuda_opts), 'CPUExecutionProvider'],
                    )
                    if 'CUDAExecutionProvider' not in session.get_providers():
                        log.warning('CUDA provider unavailable for Super Resolution; using CPU')
                        self.device = 'cpu'
                except Exception:  # noqa: BLE001
                    log.warning('CUDA session failed; Super Resolution uses CPU', exc_info=True)
                    session = None
                    self.device = 'cpu'
            if session is None:
                try:
                    session = ort.InferenceSession(
                        str(self.model_path), sess_options=so, providers=['CPUExecutionProvider']
                    )
                except Exception as exc:  # noqa: BLE001
                    raise SuperResolutionError(f'Could not load Super Resolution model: {exc}') from exc
                self.device = 'cpu'
                if self.use_cuda:
                    self.tile_size = min(self.tile_size, TILE_CPU)
            self._input_name = session.get_inputs()[0].name
            self._session = session
            return session

    def warmup(self) -> None:
        """Create the session and run a 64×64 image once."""
        self.upscale(np.zeros((64, 64, 3), np.float32), 4)

    def _run(self, session: Any, tile: np.ndarray) -> np.ndarray:
        inp = np.ascontiguousarray(tile.transpose(2, 0, 1)[None], dtype=np.float32)
        out = session.run(None, {self._input_name: inp})[0]
        return out[0].transpose(1, 2, 0)

    def upscale(self, img: np.ndarray, factor: int) -> np.ndarray:
        """Upscale a float32 ``(H, W, 3)`` image in ``[0, 1]`` by 2 or 4 (tiled)."""
        if factor not in (2, 4):
            return img
        session = self._get_session()
        h, w = img.shape[:2]
        pad = OVERLAP
        mode = 'reflect' if min(h, w) > pad else 'edge'
        padded = np.pad(np.asarray(img, np.float32), ((pad, pad), (pad, pad), (0, 0)), mode=mode)
        out = np.empty((h * factor, w * factor, 3), np.float32)
        t = self.tile_size
        s = NATIVE_SCALE
        for y0 in range(0, h, t):
            y1 = min(y0 + t, h)
            for x0 in range(0, w, t):
                x1 = min(x0 + t, w)
                res = self._run(session, padded[y0 : y1 + 2 * pad, x0 : x1 + 2 * pad])
                core = res[pad * s : pad * s + (y1 - y0) * s, pad * s : pad * s + (x1 - x0) * s]
                if factor != s:
                    core = cv2.resize(
                        core, ((x1 - x0) * factor, (y1 - y0) * factor), interpolation=cv2.INTER_AREA
                    )
                out[y0 * factor : y1 * factor, x0 * factor : x1 * factor] = core
        np.clip(out, 0.0, 1.0, out=out)
        return out


_shared: dict[bool, SuperResolver] = {}
_shared_lock = threading.Lock()


def get_shared_resolver(use_cuda: bool) -> SuperResolver:
    """One resolver per device, created once and reused."""
    with _shared_lock:
        if use_cuda not in _shared:
            _shared[use_cuda] = SuperResolver(use_cuda=use_cuda)
        return _shared[use_cuda]
