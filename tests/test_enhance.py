"""Super Resolution with CPU ONNX Runtime (spec section 5.4)."""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest

from core.backend import get_cpu_backend
from core.enhance import SR_SKIP_WARNING, SuperResolver, default_model_path
from core.pipeline import process_image
from core.settings import AdjustmentSettings
from tests.conftest import make_photo_like, to_u8

pytestmark = pytest.mark.skipif(not default_model_path().is_file(), reason='SR model missing')


@pytest.fixture(scope='module')
def resolver() -> SuperResolver:
    return SuperResolver(use_cuda=False)


@pytest.mark.parametrize('factor', [2, 4])
def test_scale_factors(resolver, factor):
    img = make_photo_like(37, 53)
    out = resolver.upscale(img, factor)
    assert out.shape == (37 * factor, 53 * factor, 3)
    assert out.dtype == np.float32 and out.min() >= 0 and out.max() <= 1


def test_off_does_nothing():
    img8 = to_u8(make_photo_like(40, 50))
    out, warnings = process_image(img8, AdjustmentSettings(super_resolution='off'), get_cpu_backend())
    assert out.shape == img8.shape and warnings == []


def test_pipeline_sr_2x(resolver):
    img8 = to_u8(make_photo_like(40, 50))
    out, warnings = process_image(
        img8, AdjustmentSettings(super_resolution='2x'), get_cpu_backend(), resolver
    )
    assert out.shape == (80, 100, 3) and out.dtype == np.uint8 and warnings == []
    # SR keeps the content: compare with a plain resize
    import cv2

    ref = cv2.resize(img8, (100, 80), interpolation=cv2.INTER_CUBIC)
    assert np.abs(out.astype(int) - ref.astype(int)).mean() < 12


@pytest.mark.parametrize('factor', [2, 4])
def test_tiled_equals_untiled(factor):
    img = make_photo_like(100, 130, seed=11)
    tiled = SuperResolver(use_cuda=False, tile_size=48).upscale(img, factor)
    whole = SuperResolver(use_cuda=False, tile_size=4096).upscale(img, factor)
    assert np.abs(tiled - whole).mean() <= 1 / 255


def test_limit_skips_sr(monkeypatch):
    import core.enhance as enhance

    monkeypatch.setattr(enhance, 'exceeds_limits', lambda w, h: w > 150)
    img8 = to_u8(make_photo_like(40, 50))
    out, warnings = process_image(img8, AdjustmentSettings(super_resolution='4x'), get_cpu_backend())
    assert out.shape == img8.shape
    assert warnings == [SR_SKIP_WARNING]


def test_missing_model_is_a_warning(tmp_path: Path):
    missing = SuperResolver(use_cuda=False, model_path=tmp_path / 'nope.onnx')
    img8 = to_u8(make_photo_like(40, 50))
    s = AdjustmentSettings(super_resolution='2x', exposure=0.5)
    s.image_size.mode, s.image_size.value = 'width', 25
    out, warnings = process_image(img8, s, get_cpu_backend(), missing)
    assert out.shape == (20, 25, 3)  # other steps still ran
    assert len(warnings) == 1 and 'model not found' in warnings[0]
