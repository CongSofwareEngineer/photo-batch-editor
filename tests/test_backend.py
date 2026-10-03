"""GPU vs CPU (spec section 10). GPU tests are skipped automatically without an NVIDIA GPU."""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import pytest
from PIL import Image

import core.backend as backend_mod
from core.backend import CpuBackend, GpuInfo, _resize_linear_xp, get_backend, reset_gpu_cache
from core.pipeline import apply_adjustments, process_image
from core.presets import list_builtin_presets
from core.settings import SLIDERS, AdjustmentSettings
from tests.conftest import make_photo_like, to_u8


def test_cupy_import_failure_falls_back(monkeypatch):
    reset_gpu_cache()
    monkeypatch.setattr(backend_mod, '_nvidia_driver_present', lambda: True)
    monkeypatch.setitem(sys.modules, 'cupy', None)  # makes ``import cupy`` raise ImportError
    try:
        b = get_backend('gpu')
        assert isinstance(b, CpuBackend) and b.name == 'cpu'
        info = backend_mod.detect_gpu()
        assert isinstance(info, GpuInfo) and not info.available
        assert info.reason and 'CuPy' in info.reason
    finally:
        reset_gpu_cache()


def test_no_driver_reason(monkeypatch):
    reset_gpu_cache()
    monkeypatch.setattr(backend_mod, '_nvidia_driver_present', lambda: False)
    try:
        info = backend_mod.detect_gpu()
        assert not info.available and info.reason == backend_mod.NO_GPU
        assert get_backend('gpu').name == 'cpu'
    finally:
        reset_gpu_cache()


def test_cpu_preference_never_touches_gpu(monkeypatch):
    monkeypatch.setattr(backend_mod, 'detect_gpu', lambda force=False: pytest.fail('detected'))
    assert get_backend('cpu').name == 'cpu'


def test_numpy_bilinear_matches_opencv():
    """The xp bilinear resize used on the GPU equals OpenCV INTER_LINEAR on the CPU."""
    import cv2

    rng = np.random.default_rng(0)
    a = rng.random((23, 31)).astype(np.float32)
    ref = cv2.resize(a, (217, 161), interpolation=cv2.INTER_LINEAR)
    out = _resize_linear_xp(np, a, 161, 217)
    assert np.abs(out - ref).max() < 1e-4


def test_numpy_block_mean_matches_opencv_area():
    import cv2

    rng = np.random.default_rng(1)
    a = rng.random((40, 60)).astype(np.float32)
    cpu = CpuBackend()
    ref = cpu._block_mean(a, 4, 0, 0)  # noqa: SLF001
    out = a.reshape(10, 4, 15, 4).mean(axis=(1, 3))
    assert np.abs(out - ref).max() < 1e-5
    padded = cpu._block_mean(a[:38, :57], 4, 2, 3)  # noqa: SLF001
    ref_pad = cv2.copyMakeBorder(a[:38, :57], 0, 2, 0, 3, cv2.BORDER_REFLECT)
    assert np.allclose(padded, ref_pad.reshape(10, 4, 15, 4).mean(axis=(1, 3)), atol=1e-5)
    # NumPy "symmetric" padding == OpenCV BORDER_REFLECT (used by the GPU backend)
    assert np.array_equal(np.pad(a[:38, :57], ((0, 2), (0, 3)), mode='symmetric'), ref_pad)


# Real GPU ----------------------------------------------------------------------------------


def _close(a: np.ndarray, b: np.ndarray, mean: float = 1 / 255, mx: float = 3 / 255) -> None:
    d = np.abs(a.astype(np.float32) - b.astype(np.float32))
    assert d.mean() <= mean and d.max() <= mx, (d.mean() * 255, d.max() * 255)


@pytest.mark.gpu
@pytest.mark.parametrize('spec', SLIDERS, ids=lambda s: s.key)
def test_gpu_matches_cpu_per_adjustment(spec):
    gpu, cpu = get_backend('gpu'), backend_mod.get_cpu_backend()
    assert gpu.name == 'gpu'
    img = make_photo_like(300, 400)
    for value in (spec.minimum, (spec.minimum + spec.maximum) / 2, spec.maximum):
        s = AdjustmentSettings(**{spec.key: value})
        ref = apply_adjustments(img, s, cpu)
        out = gpu.to_host(apply_adjustments(gpu.to_device(img), s, gpu))
        _close(out, ref)


@pytest.mark.gpu
@pytest.mark.parametrize('preset', list_builtin_presets(), ids=lambda p: p.name)
def test_gpu_matches_cpu_presets(preset):
    gpu, cpu = get_backend('gpu'), backend_mod.get_cpu_backend()
    img8 = to_u8(make_photo_like(600, 900))
    ref, _ = process_image(img8, preset.settings, cpu)
    out, _ = process_image(img8, preset.settings, gpu)
    _close(out / 255.0, ref / 255.0)


@pytest.mark.gpu
def test_gpu_sr_matches_cpu():
    from core.enhance import SuperResolver, cuda_provider_available

    if not cuda_provider_available():
        pytest.skip('ONNX Runtime CUDA provider unavailable')
    img = make_photo_like(64, 80)
    a = SuperResolver(use_cuda=True).upscale(img, 2)
    b = SuperResolver(use_cuda=False).upscale(img, 2)
    assert np.abs(a - b).mean() <= 2 / 255


@pytest.mark.gpu
def test_gpu_oom_real_backend(photo_folder: Path, monkeypatch):
    import cupy

    import core.batch as batch

    real = batch.process_image
    calls = {'n': 0}

    def flaky(img, s, backend, sr=None):
        if backend.name == 'gpu':
            calls['n'] += 1
            if calls['n'] == 3:
                raise cupy.cuda.memory.OutOfMemoryError(1 << 30, 0)
        return real(img, s, backend, sr)

    monkeypatch.setattr(batch, 'process_image', flaky)
    report = batch.run_batch(photo_folder, AdjustmentSettings(exposure=0.2), 'gpu')
    warned = [f for f in report.files if f.status == 'warning']
    assert len(warned) == 1 and warned[0].device == 'cpu'
    assert report.device_used == 'mixed' and report.succeeded == 10


@pytest.mark.gpu
def test_gpu_batch_matches_cpu_batch(photo_folder: Path, tmp_path: Path):
    from core.batch import run_batch

    s = AdjustmentSettings(
        exposure=0.3,
        highlights=-30,
        shadows=30,
        clarity=20,
        noise_reduction=30,
        sharpening_amount=40,
        vignette_amount=-20,
        vibrance=20,
    )
    a = run_batch(photo_folder, s, 'cpu', output_dir=tmp_path / 'cpu')
    b = run_batch(photo_folder, s, 'gpu', output_dir=tmp_path / 'gpu')
    assert b.device_used == 'gpu'
    for fa, fb in zip(a.files, b.files, strict=True):
        assert fa.output_path and fb.output_path
        xa = np.asarray(Image.open(fa.output_path), dtype=np.float32) / 255
        xb = np.asarray(Image.open(fb.output_path), dtype=np.float32) / 255
        _close(xa, xb, mx=4 / 255)  # +1 for JPEG re-encoding
