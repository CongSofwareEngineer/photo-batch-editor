"""Pixel effects of the photo editor (NumPy / OpenCV, RGBA uint8).

* Blur / pixelate of a region (the "Blur" layers).
* "Adjust" sliders mapped onto the batch editor's Camera Raw adjustments, so both editors
  produce the same look (``core.pipeline.apply_adjustments``).
"""

from __future__ import annotations

from dataclasses import asdict, dataclass

import cv2
import numpy as np

from core.backend import get_cpu_backend
from core.pipeline import apply_adjustments, to_uint8
from core.settings import AdjustmentSettings, SliderSpec

BLUR_MODES = ('blur', 'pixelate')


def blur_sigma(strength: float, long_side: int) -> float:
    """Gaussian sigma for a strength 1–100, proportional to the image size."""
    return max(0.5, strength / 100.0 * max(long_side, 1) / 60.0)


def pixel_block(strength: float, long_side: int) -> int:
    """Pixelate block size (px) for a strength 1–100."""
    return max(2, int(round(strength / 100.0 * max(long_side, 1) / 25.0)))


def gaussian_blur(rgba: np.ndarray, sigma: float) -> np.ndarray:
    """Blur that does not darken transparent edges (blurs premultiplied colour)."""
    if sigma <= 0:
        return rgba.copy()
    f = rgba.astype(np.float32)
    a = f[..., 3:4] / 255.0
    pre = np.concatenate([f[..., :3] * a, f[..., 3:4]], axis=2)
    pre = cv2.GaussianBlur(pre, (0, 0), sigmaX=sigma, sigmaY=sigma, borderType=cv2.BORDER_REFLECT)
    alpha = pre[..., 3:4]
    rgb = np.where(alpha > 0.5, pre[..., :3] / np.maximum(alpha / 255.0, 1e-6), 0)
    out = np.concatenate([rgb, alpha], axis=2)
    return np.clip(np.rint(out), 0, 255).astype(np.uint8)


def pixelate(rgba: np.ndarray, block: int, origin: tuple[int, int] = (0, 0)) -> np.ndarray:
    """Mosaic of ``block``-px cells aligned on the image grid (``origin`` = region offset)."""
    h, w = rgba.shape[:2]
    block = max(1, int(block))
    if block == 1 or h == 0 or w == 0:
        return rgba.copy()
    ox, oy = origin[0] % block, origin[1] % block
    # pad so the cells line up with the full image, average each cell, expand back
    pad_l, pad_t = ox, oy
    pw = -(-(w + pad_l) // block) * block
    ph = -(-(h + pad_t) // block) * block
    padded = cv2.copyMakeBorder(rgba, pad_t, ph - h - pad_t, pad_l, pw - w - pad_l, cv2.BORDER_REPLICATE)
    small = cv2.resize(padded, (pw // block, ph // block), interpolation=cv2.INTER_AREA)
    big = cv2.resize(small, (pw, ph), interpolation=cv2.INTER_NEAREST)
    return np.ascontiguousarray(big[pad_t : pad_t + h, pad_l : pad_l + w])


def apply_blur_effect(
    rgba: np.ndarray, mode: str, strength: float, long_side: int, origin: tuple[int, int] = (0, 0)
) -> np.ndarray:
    if mode == 'pixelate':
        return pixelate(rgba, pixel_block(strength, long_side), origin)
    return gaussian_blur(rgba, blur_sigma(strength, long_side))


# Adjust panel ------------------------------------------------------------------------------

ADJUST_SLIDERS: tuple[SliderSpec, ...] = (
    SliderSpec('brightness', 'Brightness', 'Adjust', 'Adjust', -100, 100, 1),
    SliderSpec('contrast', 'Contrast', 'Adjust', 'Adjust', -100, 100, 1),
    SliderSpec('saturation', 'Saturation', 'Adjust', 'Adjust', -100, 100, 1),
    SliderSpec('temperature', 'Temperature', 'Adjust', 'Adjust', -100, 100, 1),
    SliderSpec('sharpness', 'Sharpness', 'Adjust', 'Adjust', 0, 150, 1),
)


@dataclass(frozen=True)
class PhotoAdjust:
    brightness: float = 0.0
    contrast: float = 0.0
    saturation: float = 0.0
    temperature: float = 0.0
    sharpness: float = 0.0

    def is_default(self) -> bool:
        return self == PhotoAdjust()

    def to_settings(self) -> AdjustmentSettings:
        """Same algorithms as the batch editor (Camera Raw–style adjustments)."""
        return AdjustmentSettings(
            brightness=self.brightness,
            contrast=self.contrast,
            saturation=self.saturation,
            temperature=self.temperature,
            sharpening_amount=self.sharpness,
        )

    def to_dict(self) -> dict[str, float]:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: object) -> PhotoAdjust:
        if not isinstance(data, dict):
            return cls()
        out = {}
        for spec in ADJUST_SLIDERS:
            try:
                v = float(data.get(spec.key, 0.0))
            except (TypeError, ValueError):
                v = 0.0
            out[spec.key] = min(spec.maximum, max(spec.minimum, v))
        return cls(**out)


def apply_photo_adjust(rgba: np.ndarray, adjust: PhotoAdjust, long_side: int | None = None) -> np.ndarray:
    """RGBA uint8 → adjusted RGBA uint8 (alpha unchanged).

    ``long_side`` is the size of the full image when ``rgba`` is a reduced preview, so the
    sharpening radius looks the same at every preview size.
    """
    if adjust.is_default():
        return rgba
    backend = get_cpu_backend()
    rgb = np.ascontiguousarray(rgba[..., :3])
    out = apply_adjustments(rgb, adjust.to_settings(), backend, long_side=long_side)
    res = np.empty_like(rgba)
    res[..., :3] = to_uint8(out, np)
    res[..., 3] = rgba[..., 3]
    return res
