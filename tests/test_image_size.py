"""Image Size (spec section 5.5, 5.6)."""

from __future__ import annotations

import numpy as np
import pytest

from core.image_size import CLAMP_WARNING, resize_image, target_size
from core.pipeline import predict_output_size
from core.settings import MAX_OUTPUT_LONG_EDGE, RESAMPLE_OPTIONS, AdjustmentSettings, ImageSizeSettings
from tests.conftest import make_photo_like


def S(mode: str, value: float, resample: str = 'automatic', dont: bool = False) -> ImageSizeSettings:
    return ImageSizeSettings(mode=mode, value=value, resample=resample, dont_enlarge=dont)


@pytest.mark.parametrize(
    'mode,value,expected',
    [
        ('percent', 50, (300, 200)),
        ('percent', 200, (1200, 800)),
        ('long_edge', 900, (900, 600)),
        ('width', 300, (300, 200)),
        ('height', 100, (150, 100)),
    ],
)
def test_modes_give_expected_size(mode, value, expected):
    w, h, warnings = target_size(600, 400, S(mode, value))
    assert (w, h) == expected and warnings == []


@pytest.mark.parametrize('mode,value', [('percent', 37), ('long_edge', 777), ('width', 333), ('height', 251)])
def test_aspect_ratio_preserved(mode, value):
    w, h, _ = target_size(4032, 3024, S(mode, value))
    assert abs(w / h * 3024 - 4032) <= 4032 / h + 1  # within ±1 px


def test_portrait_long_edge():
    assert target_size(400, 600, S('long_edge', 300))[:2] == (200, 300)


def test_off_keeps_size():
    img = make_photo_like(50, 70)
    out, warnings = resize_image(img, S('off', 100))
    assert out.shape == img.shape and warnings == []
    assert out is img


def test_dont_enlarge():
    assert target_size(600, 400, S('long_edge', 2000, dont=True))[:2] == (600, 400)
    assert target_size(600, 400, S('percent', 150, dont=True))[:2] == (600, 400)
    assert target_size(600, 400, S('long_edge', 300, dont=True))[:2] == (300, 200)


def test_clamped_to_limits():
    w, h, warnings = target_size(6000, 4000, S('percent', 400))
    assert warnings == [CLAMP_WARNING]
    assert max(w, h) <= MAX_OUTPUT_LONG_EDGE and w * h <= 200_000_000
    assert abs(w / h - 1.5) < 0.01
    # megapixel limit on a near-square image
    w, h, warnings = target_size(10000, 9000, S('percent', 190))
    assert warnings and w * h <= 200_000_000 and max(w, h) <= MAX_OUTPUT_LONG_EDGE


@pytest.mark.parametrize('resample', list(RESAMPLE_OPTIONS))
@pytest.mark.parametrize('mode,value', [('percent', 50), ('percent', 180)])
def test_every_resample_runs(resample, mode, value):
    img = make_photo_like(60, 80)
    out, _ = resize_image(img, S(mode, value, resample))
    expected = (round(60 * value / 100), round(80 * value / 100))
    assert out.shape == (*expected, 3)
    assert out.dtype == np.float32
    assert np.isfinite(out).all() and out.min() >= 0 and out.max() <= 1


def test_predict_output_size_with_sr():
    s = AdjustmentSettings(super_resolution='2x', image_size=S('long_edge', 1000))
    assert predict_output_size(600, 400, s)[:2] == (1000, 667)
    s = AdjustmentSettings(super_resolution='4x')
    w, h, warnings = predict_output_size(6000, 4000, s)
    assert (w, h) == (6000, 4000) and warnings  # SR skipped (would exceed 20,000 px)
