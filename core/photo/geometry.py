"""Geometry for the photo editor: zoom steps, zoom at a point, crop ratios, canvas rotation."""

from __future__ import annotations

import math

ZOOM_MIN, ZOOM_MAX = 0.10, 16.0  # 10 % … 1600 %
ZOOM_STEPS: tuple[float, ...] = (
    0.10,
    0.125,
    1 / 6,
    0.25,
    1 / 3,
    0.5,
    2 / 3,
    1.0,
    1.5,
    2.0,
    3.0,
    4.0,
    5.0,
    6.0,
    8.0,
    12.0,
    16.0,
)

# label → width / height (None = free); "original" is resolved by the caller
CROP_RATIOS: dict[str, float | None] = {
    'free': None,
    '1:1': 1.0,
    '4:3': 4 / 3,
    '16:9': 16 / 9,
    '9:16': 9 / 16,
}


def clamp_zoom(scale: float) -> float:
    return min(ZOOM_MAX, max(ZOOM_MIN, scale))


def next_zoom(scale: float, direction: int) -> float:
    """The next Photoshop-like zoom step above (``direction > 0``) or below the current one."""
    if direction > 0:
        for z in ZOOM_STEPS:
            if z > scale * 1.001:
                return z
        return ZOOM_MAX
    for z in reversed(ZOOM_STEPS):
        if z < scale / 1.001:
            return z
    return ZOOM_MIN


def zoom_at(
    scale: float, new_scale: float, offset: tuple[float, float], anchor: tuple[float, float]
) -> tuple[float, float]:
    """New view offset so that the image point under ``anchor`` (screen px) stays there.

    The view maps ``screen = offset + image * scale``.
    """
    ix = (anchor[0] - offset[0]) / scale
    iy = (anchor[1] - offset[1]) / scale
    return anchor[0] - ix * new_scale, anchor[1] - iy * new_scale


def fit_scale(img_w: int, img_h: int, view_w: int, view_h: int, margin: int = 24) -> float:
    w = max(1, view_w - 2 * margin)
    h = max(1, view_h - 2 * margin)
    return clamp_zoom(min(w / max(1, img_w), h / max(1, img_h)))


def crop_rect(
    anchor: tuple[float, float], pointer: tuple[float, float], ratio: float | None, bounds: tuple[int, int]
) -> tuple[float, float, float, float]:
    """Crop rectangle ``(x, y, w, h)`` dragged from ``anchor`` to ``pointer``.

    With a ``ratio`` (width / height) the rectangle keeps it; the result always stays inside
    the image ``bounds`` (width, height).
    """
    bw, bh = bounds
    ax = min(max(anchor[0], 0.0), bw)
    ay = min(max(anchor[1], 0.0), bh)
    px = min(max(pointer[0], 0.0), bw)
    py = min(max(pointer[1], 0.0), bh)
    w, h = abs(px - ax), abs(py - ay)
    sx = 1 if px >= ax else -1
    sy = 1 if py >= ay else -1
    if ratio:
        # fit the pointer box to the ratio, then shrink until it fits in the image
        if w / max(h, 1e-9) > ratio:
            h = w / ratio
        else:
            w = h * ratio
        max_w = ax if sx < 0 else bw - ax
        max_h = ay if sy < 0 else bh - ay
        k = min(1.0, max_w / max(w, 1e-9), max_h / max(h, 1e-9))
        w, h = w * k, h * k
    x = ax if sx > 0 else ax - w
    y = ay if sy > 0 else ay - h
    return x, y, w, h


def round_rect(rect: tuple[float, float, float, float], bounds: tuple[int, int]) -> tuple[int, int, int, int]:
    """Integer pixel rectangle inside the image (at least 1×1)."""
    bw, bh = bounds
    x0 = int(round(min(max(rect[0], 0), bw - 1)))
    y0 = int(round(min(max(rect[1], 0), bh - 1)))
    x1 = int(round(min(max(rect[0] + rect[2], x0 + 1), bw)))
    y1 = int(round(min(max(rect[1] + rect[3], y0 + 1), bh)))
    return x0, y0, x1 - x0, y1 - y0


def rotate90_point(x: float, y: float, size: tuple[int, int], clockwise: bool) -> tuple[float, float]:
    """Where an image point goes when the canvas of ``size`` (w, h) is turned by 90°."""
    w, h = size
    return (h - y, x) if clockwise else (y, w - x)


def flip_point(x: float, y: float, size: tuple[int, int], horizontal: bool) -> tuple[float, float]:
    w, h = size
    return (w - x, y) if horizontal else (x, h - y)


def cover_scale(w: float, h: float, angle_deg: float) -> float:
    """Scale that makes a ``w×h`` image rotated by ``angle`` still cover the ``w×h`` canvas."""
    a = math.radians(angle_deg)
    c, s = abs(math.cos(a)), abs(math.sin(a))
    return max((w * c + h * s) / w, (w * s + h * c) / h)


def normalize_angle(a: float) -> float:
    """Angle in (-180, 180]."""
    a = math.fmod(a, 360.0)
    if a <= -180:
        a += 360
    elif a > 180:
        a -= 360
    return a
