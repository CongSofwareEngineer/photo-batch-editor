"""Adjustments 1–15 on the CPU backend (spec section 12)."""

from __future__ import annotations

from typing import Any

import cv2
import numpy as np
import pytest

from core.pipeline import apply_adjustments, process_image
from core.settings import SLIDERS, AdjustmentSettings
from tests.conftest import make_dark_bright, make_gradient, make_noise, make_photo_like, to_u8


def run(img: np.ndarray, cpu, **kw) -> np.ndarray:
    return apply_adjustments(img.copy(), AdjustmentSettings(**kw), cpu)


def lum(img: np.ndarray) -> np.ndarray:
    return img[..., 0] * 0.2126 + img[..., 1] * 0.7152 + img[..., 2] * 0.0722


@pytest.mark.parametrize('spec', SLIDERS, ids=lambda s: s.key)
def test_default_is_unchanged(spec, cpu):
    img = make_photo_like()
    out = run(img, cpu, **{spec.key: spec.default})
    assert np.abs(out - img).max() <= 1 / 255


def test_all_default_full_pipeline_is_identity(cpu):
    img8 = to_u8(make_photo_like())
    out, warnings = process_image(img8, AdjustmentSettings(), cpu)
    assert warnings == []
    assert out.dtype == np.uint8
    assert np.array_equal(out, img8)


@pytest.mark.parametrize('spec', SLIDERS, ids=lambda s: s.key)
@pytest.mark.parametrize('which', ['min', 'max'])
def test_extremes_are_finite_and_in_range(spec, which, cpu):
    value = spec.minimum if which == 'min' else spec.maximum
    for img in (
        make_photo_like(),
        make_noise(),
        make_dark_bright(),
        np.zeros((20, 30, 3), np.float32),
        np.ones((20, 30, 3), np.float32),
    ):
        out = run(img, cpu, **{spec.key: value})
        assert np.isfinite(out).all()
        assert out.min() >= 0.0 and out.max() <= 1.0
        assert out.dtype == np.float32


def test_exposure_and_brightness_direction(cpu):
    img = make_gradient()
    assert run(img, cpu, exposure=1.0).mean() > img.mean()
    assert run(img, cpu, exposure=-1.0).mean() < img.mean()
    assert run(img, cpu, brightness=50).mean() > img.mean()
    assert run(img, cpu, brightness=-50).mean() < img.mean()


def test_saturation_minus_100_is_gray(cpu):
    out = run(make_photo_like(), cpu, saturation=-100)
    assert np.abs(out[..., 0] - out[..., 1]).max() < 1e-5
    assert np.abs(out[..., 1] - out[..., 2]).max() < 1e-5


def test_temperature_direction(cpu):
    img = make_photo_like()
    ratio = lambda a: a[..., 0].mean() / a[..., 2].mean()  # noqa: E731
    assert ratio(run(img, cpu, temperature=50)) > ratio(img)
    assert ratio(run(img, cpu, temperature=-50)) < ratio(img)


def test_tint_direction(cpu):
    img = make_photo_like()
    assert run(img, cpu, tint=50)[..., 1].mean() < img[..., 1].mean()


def test_contrast_direction(cpu):
    img = make_gradient()
    assert run(img, cpu, contrast=50).std() > img.std()
    assert run(img, cpu, contrast=-50).std() < img.std()


def test_highlights_shadows_direction(cpu):
    img = make_dark_bright()
    w = img.shape[1]
    dark, bright = slice(0, w // 2 - 20), slice(w // 2 + 20, w)
    s = run(img, cpu, shadows=80)
    assert lum(s)[:, dark].mean() > lum(img)[:, dark].mean()
    h = run(img, cpu, highlights=-80)
    assert lum(h)[:, bright].mean() < lum(img)[:, bright].mean()
    assert abs(lum(h)[:, dark].mean() - lum(img)[:, dark].mean()) < 0.01


def test_whites_blacks_direction(cpu):
    img = make_gradient()
    assert run(img, cpu, whites=50).mean() > img.mean()
    deeper = run(img, cpu, blacks=-50)
    assert deeper.mean() < img.mean()


def test_vibrance_boosts_muted_more(cpu):
    img = np.zeros((10, 20, 3), np.float32)
    img[:, :10] = (0.55, 0.5, 0.45)  # muted
    img[:, 10:] = (0.9, 0.1, 0.1)  # saturated
    out = run(img, cpu, vibrance=60)
    chroma = lambda a: a.max(axis=2) - a.min(axis=2)  # noqa: E731
    gain_muted = chroma(out)[:, :10].mean() / chroma(img)[:, :10].mean()
    gain_sat = chroma(out)[:, 10:].mean() / chroma(img)[:, 10:].mean()
    assert gain_muted > gain_sat >= 1.0


def test_clarity_and_sharpening_increase_local_contrast(cpu):
    img = cv2.GaussianBlur(make_photo_like(200, 260), (0, 0), 1.5)
    edge = lambda a: np.abs(np.diff(lum(a), axis=1)).mean()  # noqa: E731
    assert edge(run(img, cpu, clarity=80)) > edge(img)
    assert edge(run(img, cpu, sharpening_amount=120)) > edge(img)


def test_vignette_negative_darkens_corners(cpu):
    img = np.full((100, 150, 3), 0.6, np.float32)
    out = run(img, cpu, vignette_amount=-80)
    assert out[:5, :5].mean() < out[45:55, 70:80].mean()
    assert abs(out[45:55, 70:80].mean() - 0.6) < 1e-3
    lighter = run(img, cpu, vignette_amount=80)
    assert lighter[:5, :5].mean() > 0.6


def test_noise_reduction_lowers_std(cpu):
    img = make_noise(200, 260)
    out = run(img, cpu, noise_reduction=100)
    assert out.std() < img.std() * 0.8
    mid = run(img, cpu, noise_reduction=40)
    assert out.std() < mid.std() < img.std()


def test_noise_reduction_keeps_edges(cpu):
    img = make_dark_bright(200, 260)
    out = run(img, cpu, noise_reduction=100)
    # the edge between the dark and bright halves stays sharp
    row = lum(out)[100]
    assert row[125] < 0.2 and row[134] > 0.8


@pytest.mark.parametrize(
    'kw',
    [
        dict(highlights=-60, shadows=60),
        dict(clarity=70),
        dict(noise_reduction=80),
        dict(sharpening_amount=100, exposure=0.5, vibrance=30, vignette_amount=-40),
    ],
)
def test_preview_matches_full_size(kw, cpu):
    """Downscaled preview and full-size export look alike after scaling to the same size."""
    full = make_photo_like(1200, 1600, seed=7)
    full = cv2.GaussianBlur(full, (0, 0), 2)  # photo-like smoothness
    small = cv2.resize(full, (400, 300), interpolation=cv2.INTER_AREA)
    s = AdjustmentSettings(**kw)
    out_full = apply_adjustments(full, s, cpu)
    out_small = apply_adjustments(small, s, cpu)
    down = cv2.resize(out_full, (400, 300), interpolation=cv2.INTER_AREA)
    assert np.abs(down - out_small).mean() <= 2 / 255


def reference_pipeline(img: np.ndarray, s: AdjustmentSettings, b) -> np.ndarray:
    """Section 6 order written with the one-adjustment reference functions."""
    from core import adjustments as A

    c = lambda x: np.clip(x, 0, 1)  # noqa: E731
    img = c(A.tint(A.temperature(img, s.temperature, b), s.tint, b))
    img = A.contrast(A.brightness(A.exposure(img, s.exposure, b), s.brightness, b), s.contrast, b)
    img = A.highlights_shadows(img, s.highlights, s.shadows, b)
    img = c(A.blacks(A.whites(img, s.whites, b), s.blacks, b))
    img = c(A.saturation(A.vibrance(img, s.vibrance, b), s.saturation, b))
    img = A.noise_reduction(img, s.noise_reduction, b)
    img = c(A.sharpening(A.clarity(img, s.clarity, b), s.sharpening_amount, b))
    return c(A.vignette(img, s.vignette_amount, b))


@pytest.mark.parametrize('seed', range(8))
def test_fused_pipeline_matches_reference(seed, cpu):
    rng = np.random.default_rng(seed)
    kw: dict[str, Any] = {sp.key: float(rng.uniform(sp.minimum, sp.maximum)) for sp in SLIDERS}
    if seed % 2:  # also cover partially-enabled groups
        for sp in SLIDERS:
            if rng.random() < 0.5:
                kw[sp.key] = 0.0
    s = AdjustmentSettings(**kw)
    img8 = to_u8(make_photo_like(150, 200, seed=seed))
    fused = apply_adjustments(img8, s, cpu)
    ref = reference_pipeline(img8.astype(np.float32) / 255, s, cpu)
    assert np.abs(fused - ref).max() < 2e-3
    # float input takes the non-LUT path and must agree too
    fused_f = apply_adjustments(img8.astype(np.float32) / 255, s, cpu)
    assert np.abs(fused_f - ref).max() < 2e-3


def test_input_not_modified(cpu):
    img = make_photo_like()
    before = img.copy()
    apply_adjustments(img, AdjustmentSettings(clarity=50, vignette_amount=30, saturation=20), cpu)
    assert np.array_equal(img, before)


def test_fast_guided_filter_close_to_exact(cpu):
    """Noise Reduction at large radius (subsampled coefficients) stays close to the exact filter."""
    from core import adjustments as A

    img = make_noise(600, 800, sigma=0.05)
    lum = A.luminance(img)
    exact_backend_ratio = A.NR_SUBSAMPLE_RADIUS
    try:
        A.NR_SUBSAMPLE_RADIUS = 10_000  # disables subsampling
        exact = A.guided_filter(cpu, lum, [lum], 12, 0.0005)[0]
    finally:
        A.NR_SUBSAMPLE_RADIUS = exact_backend_ratio
    fast = A.guided_filter(cpu, lum, [lum], 12, 0.0005)[0]
    assert np.abs(fast - exact).mean() < 1 / 255


def test_large_sigma_blur_close_to_exact(cpu):
    """The downscale → blur → upscale shortcut stays close to a full-resolution blur."""
    rng = np.random.default_rng(0)
    a = cv2.GaussianBlur(rng.random((300, 400)).astype(np.float32), (0, 0), 6)
    approx = cpu.gaussian_blur(a, 20.0)
    exact = cv2.GaussianBlur(a, (161, 161), 20.0, borderType=cv2.BORDER_REFLECT)
    assert np.abs(approx - exact).mean() < 0.01
