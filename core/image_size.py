"""Photoshop Image › Image Size: resize + resample, always on the CPU (spec section 5.5)."""

from __future__ import annotations

import logging
import math

import cv2
import numpy as np

from core.backend import gaussian_radius
from core.settings import MAX_OUTPUT_LONG_EDGE, MAX_OUTPUT_MEGAPIXELS, ImageSizeSettings

log = logging.getLogger(__name__)

CLAMP_WARNING = 'Image Size clamped to 20,000 px / 200 MP'


def exceeds_limits(width: int, height: int) -> bool:
    return max(width, height) > MAX_OUTPUT_LONG_EDGE or width * height > MAX_OUTPUT_MEGAPIXELS * 1_000_000


def clamp_to_limits(width: int, height: int) -> tuple[int, int]:
    """Largest size with the same aspect ratio that fits the output limits (section 5.6)."""
    if not exceeds_limits(width, height):
        return width, height
    scale = min(
        MAX_OUTPUT_LONG_EDGE / max(width, height),
        math.sqrt(MAX_OUTPUT_MEGAPIXELS * 1_000_000 / (width * height)),
    )
    while True:  # loop only guards against float rounding at the boundary
        w, h = max(1, int(width * scale)), max(1, int(height * scale))
        if not exceeds_limits(w, h):
            return w, h
        scale *= 0.9999


def target_size(width: int, height: int, s: ImageSizeSettings) -> tuple[int, int, list[str]]:
    """New ``(width, height, warnings)`` for an image of ``width × height``."""
    if s.mode == 'off':
        return width, height, []
    v = float(s.value)
    if s.mode == 'percent':
        tw, th = width * v / 100, height * v / 100
    elif s.mode == 'long_edge':
        k = v / max(width, height)
        tw, th = width * k, height * k
    elif s.mode == 'width':
        tw, th = v, height * v / width
    elif s.mode == 'height':
        tw, th = width * v / height, v
    else:
        return width, height, []
    tw_i, th_i = max(1, int(round(tw))), max(1, int(round(th)))
    if s.dont_enlarge and (tw_i > width or th_i > height):
        return width, height, []
    warnings: list[str] = []
    if exceeds_limits(tw_i, th_i):
        tw_i, th_i = clamp_to_limits(tw_i, th_i)
        warnings.append(CLAMP_WARNING)
    return tw_i, th_i, warnings


def _unsharp(img: np.ndarray, amount: float, sigma: float) -> np.ndarray:
    k = 2 * gaussian_radius(sigma) + 1
    blur = cv2.GaussianBlur(img, (k, k), sigmaX=sigma, sigmaY=sigma, borderType=cv2.BORDER_REFLECT)
    return img + amount * (img - blur)


def resize_image(img: np.ndarray, s: ImageSizeSettings) -> tuple[np.ndarray, list[str]]:
    """Resize a float32 ``(H, W, 3)`` image in ``[0, 1]``; returns ``(image, warnings)``."""
    h, w = img.shape[:2]
    tw, th, warnings = target_size(w, h, s)
    if (tw, th) == (w, h):
        return img, warnings
    enlarge = tw * th > w * h
    src = np.ascontiguousarray(img, dtype=np.float32)
    method = s.resample
    if method == 'automatic':
        out = cv2.resize(src, (tw, th), interpolation=cv2.INTER_LANCZOS4 if enlarge else cv2.INTER_AREA)
    elif method == 'preserve_details':
        out = _unsharp(cv2.resize(src, (tw, th), interpolation=cv2.INTER_LANCZOS4), 0.3, 1.0)
    elif method == 'bicubic_sharper':
        out = _unsharp(cv2.resize(src, (tw, th), interpolation=cv2.INTER_AREA), 0.25, 0.6)
    elif method in ('bicubic_smoother', 'bicubic'):
        out = cv2.resize(src, (tw, th), interpolation=cv2.INTER_CUBIC)
    elif method == 'bilinear':
        out = cv2.resize(src, (tw, th), interpolation=cv2.INTER_LINEAR)
    elif method == 'nearest_neighbor':
        out = cv2.resize(src, (tw, th), interpolation=cv2.INTER_NEAREST)
    else:
        out = cv2.resize(src, (tw, th), interpolation=cv2.INTER_AREA)
    if out.ndim == 2:
        out = out[..., None]
    np.clip(out, 0.0, 1.0, out=out)
    return out, warnings
