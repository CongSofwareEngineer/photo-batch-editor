"""Processing pipeline for one photo, in the fixed order of spec section 6.

The live preview and the batch both use these functions.
"""

from __future__ import annotations

import logging
from typing import Any

import numpy as np

from core import adjustments as adj
from core.backend import Backend
from core.enhance import (
    SR_SKIP_WARNING,
    SuperResolutionError,
    SuperResolver,
    get_shared_resolver,
    sr_exceeds_limits,
)
from core.image_size import resize_image, target_size
from core.settings import AdjustmentSettings

log = logging.getLogger(__name__)

SR_GPU_POOL_FRACTION = 0.5


def to_uint8(img: Any, xp: Any) -> Any:
    """Float (…, 3) image in [0, 1] → uint8, rounded."""
    out = xp.clip(img, 0.0, 1.0)
    out *= 255.0
    return xp.rint(out, out=out).astype(xp.uint8)


def apply_adjustments_planar(
    img: Any, s: AdjustmentSettings, backend: Backend, long_side: int | None = None
) -> Any:
    """Adjustments 1–15 (groups 1–5) → planar float32 ``(3, H, W)`` backend array.

    ``img`` is an ``(H, W, 3)`` backend array (uint8, uint16 or float in [0, 1]) and is
    never modified. ``long_side`` overrides the size used for spatial parameters.
    """
    ls = long_side or adj.long_side_of(img)
    # [1 White Balance] + [2 Tone: Exposure → Brightness → Contrast] (per-channel functions)
    p = adj.to_planar(img, s, backend)
    # [2 Tone, continued] Highlights + Shadows → Whites → Blacks → clip
    if any((s.exposure, s.brightness, s.contrast, s.highlights, s.shadows, s.whites, s.blacks)):
        p = adj.tone_local(p, s, backend, ls)
    # [3 Color] Vibrance → Saturation → clip
    if s.vibrance or s.saturation:
        p = adj.color(p, s, backend)
    # [4 Detail] Noise Reduction → Clarity → Sharpening › Amount → clip
    if s.noise_reduction or s.clarity or s.sharpening_amount:
        p = adj.detail(p, s, backend, ls)
    # [5 Effects] Post-Crop Vignetting › Amount → clip
    if s.vignette_amount:
        p = adj.effects(p, s, backend)
    return p


def apply_adjustments(img: Any, s: AdjustmentSettings, backend: Backend, long_side: int | None = None) -> Any:
    """Adjustments 1–15; returns a float32 ``(H, W, 3)`` backend array (section 6)."""
    return adj.planar_to_hwc(apply_adjustments_planar(img, s, backend, long_side), backend)


def predict_output_size(width: int, height: int, s: AdjustmentSettings) -> tuple[int, int, list[str]]:
    """Output dimensions after Super Resolution and Image Size, plus the warnings they raise."""
    warnings: list[str] = []
    factor = s.sr_factor
    if factor > 1:
        if sr_exceeds_limits(width, height, factor):
            warnings.append(SR_SKIP_WARNING)
        else:
            width, height = width * factor, height * factor
    w, h, more = target_size(width, height, s.image_size)
    return w, h, warnings + more


def _sr_uses_cuda(backend: Backend) -> bool:
    info = getattr(backend, 'info', None)
    return backend.name == 'gpu' and bool(info and info.sr_cuda_available)


def process_image(
    img_np: np.ndarray, s: AdjustmentSettings, backend: Backend, sr: SuperResolver | None = None
) -> tuple[np.ndarray, list[str]]:
    """Steps 1–7 for an image read from disk; returns ``(uint8 RGB, warnings)``.

    In GPU mode the image is uploaded as uint8/uint16 and converted on the GPU; it is
    downloaded as uint8 unless Super Resolution or Image Size need float data.
    """
    warnings: list[str] = []
    p = apply_adjustments_planar(backend.to_device(np.ascontiguousarray(img_np)), s, backend)

    factor = s.sr_factor
    if factor == 1 and s.image_size.is_default():
        return backend.to_host(adj.planar_to_uint8(p, backend)), warnings

    host = np.asarray(backend.to_host(adj.planar_to_hwc(p, backend)), dtype=np.float32)
    del p
    if factor > 1:
        h, w = host.shape[:2]
        if sr_exceeds_limits(w, h, factor):
            warnings.append(SR_SKIP_WARNING)
        else:
            try:
                resolver = sr or get_shared_resolver(_sr_uses_cuda(backend))
                if backend.name == 'gpu':
                    backend.set_memory_limit(SR_GPU_POOL_FRACTION)
                    backend.free_memory()
                host = resolver.upscale(host, factor)
            except SuperResolutionError as exc:
                log.warning('Super Resolution skipped: %s', exc)
                warnings.append(f'Super Resolution skipped: {exc}')

    host, size_warnings = resize_image(host, s.image_size)
    warnings.extend(size_warnings)
    return to_uint8(host, np), warnings
