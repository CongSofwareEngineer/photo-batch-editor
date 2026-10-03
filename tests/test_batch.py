"""Batch processing (spec sections 9, 12)."""

from __future__ import annotations

import hashlib
import threading
from pathlib import Path

import numpy as np
import pytest
from PIL import Image

from core.backend import CpuBackend
from core.batch import LOG_FILENAME, FileResult, run_batch
from core.enhance import default_model_path
from core.settings import AdjustmentSettings, ImageSizeSettings
from tests.conftest import gpu_available


class FakeGpuBackend(CpuBackend):
    """CPU math behind the GPU code path, so the 3-stage pipeline is tested without a GPU."""

    name = 'gpu'

    def __init__(self) -> None:
        super().__init__()
        self.device_name = 'Fake GPU'


def devices() -> list[str]:
    return ['cpu', 'fake-gpu'] + (['gpu'] if gpu_available() else [])


def run(folder: Path, settings: AdjustmentSettings, device: str, **kw):
    if device == 'fake-gpu':
        return run_batch(folder, settings, 'gpu', backend=FakeGpuBackend(), **kw)
    return run_batch(folder, settings, device, workers=2, **kw)


def digest(folder: Path) -> dict[str, str]:
    return {
        p.relative_to(folder).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted(folder.rglob('*'))
        if p.is_file()
    }


@pytest.mark.parametrize('device', devices())
def test_batch_of_10(photo_folder: Path, device: str):
    before = digest(photo_folder)
    s = AdjustmentSettings(exposure=0.3, shadows=20, vibrance=15, clarity=10)
    report = run(photo_folder, s, device)
    assert digest(photo_folder) == before  # originals unchanged
    out = photo_folder.parent / 'clientA_update'
    assert report.output_dir == out
    jpgs = sorted(p.relative_to(out).as_posix() for p in out.rglob('*.jpg'))
    assert len(jpgs) == 10
    assert 'outdoor/ảnh cưới 01.jpg' in jpgs and 'outdoor/gray.jpg' in jpgs
    assert len(report.files) == 10 and not report.cancelled
    assert all(f.status == 'ok' for f in report.files), [f.message for f in report.files]
    expected_device = 'cpu' if device == 'cpu' else 'gpu'
    assert report.device_used == expected_device
    assert all(f.device == expected_device for f in report.files)
    for f in report.files:
        assert f.output_path
        with Image.open(f.output_path) as im:
            assert im.size == (f.width, f.height)
        assert f.size_bytes == f.output_path.stat().st_size
    assert [f.input_path for f in report.files] == sorted(
        [f.input_path for f in report.files],
        key=lambda p: tuple(x.casefold() for x in p.relative_to(photo_folder).parts),
    )
    assert not list(out.rglob('*.tmp'))


@pytest.mark.parametrize('device', devices())
def test_corrupt_file(photo_folder: Path, device: str):
    (photo_folder / 'IMG_003.png').write_bytes(b'this is not a png')
    report = run(photo_folder, AdjustmentSettings(contrast=10), device)
    assert report.succeeded == 9 and report.count('error') == 1
    bad = next(f for f in report.files if f.status == 'error')
    assert bad.input_path.name == 'IMG_003.png' and 'orrupt' in bad.message
    assert bad.output_path is None and bad.width is None


@pytest.mark.parametrize('device', devices())
def test_process_log(photo_folder: Path, device: str):
    report = run(photo_folder, AdjustmentSettings(), device)
    text = (report.output_dir / LOG_FILENAME).read_text(encoding='utf-8')
    assert 'Device used: ' + report.device_used in text
    assert 'IMG_000.png' in text and '"exposure": 0' in text


@pytest.mark.parametrize('device', devices())
def test_sizes_with_image_size(photo_folder: Path, device: str):
    s = AdjustmentSettings(image_size=ImageSizeSettings('long_edge', 50, 'bicubic_sharper'))
    report = run(photo_folder, s, device)
    for f in report.files:
        assert f.width and f.height and f.output_path
        assert max(f.width, f.height) == 50
        with Image.open(f.output_path) as im:
            assert im.size == (f.width, f.height)
            assert im.getexif().get(0x0112, 1) == 1


@pytest.mark.skipif(not default_model_path().is_file(), reason='SR model missing')
def test_sizes_with_super_resolution(tmp_path: Path):
    from tests.conftest import make_photo_like, save_png, to_u8

    folder = tmp_path / 'sr'
    save_png(folder / 'a.png', to_u8(make_photo_like(30, 40)))
    s = AdjustmentSettings(super_resolution='2x')
    report = run_batch(folder, s, 'cpu')
    [f] = report.files
    assert (f.width, f.height) == (80, 60) and f.status == 'ok'


@pytest.mark.parametrize('device', devices())
def test_cancel(photo_folder: Path, device: str):
    cancel = threading.Event()
    seen: list[FileResult] = []

    def progress(done, total, current, res):
        if res is not None:
            seen.append(res)
            cancel.set()

    report = run(
        photo_folder,
        AdjustmentSettings(noise_reduction=50),
        device,
        on_progress=progress,
        cancel_event=cancel,
    )
    assert report.cancelled
    statuses = [f.status for f in report.files]
    assert 'cancelled' in statuses and statuses.count('ok') >= 1
    assert len(report.files) == 10
    for f in report.files:
        if f.status == 'cancelled':
            assert f.output_path is None
        if f.status == 'ok':
            assert f.output_path.exists()
    assert not list(report.output_dir.rglob('*.tmp'))


def test_gpu_oom_falls_back_to_cpu(photo_folder: Path, monkeypatch):
    """Simulated out-of-memory on one photo: that photo runs on the CPU, the batch continues."""
    import core.batch as batch

    real = batch.process_image
    calls = {'gpu': 0}

    def flaky(img, s, backend, sr=None):
        if backend.name == 'gpu':
            calls['gpu'] += 1
            if calls['gpu'] == 2:
                raise MemoryError('Out of memory allocating 1,234 bytes')
        return real(img, s, backend, sr)

    monkeypatch.setattr(batch, 'process_image', flaky)
    report = run_batch(photo_folder, AdjustmentSettings(exposure=0.2), 'gpu', backend=FakeGpuBackend())
    assert report.succeeded == 10
    warned = [f for f in report.files if f.status == 'warning']
    assert len(warned) == 1 and warned[0].device == 'cpu'
    assert 'processed on CPU' in warned[0].message
    assert report.device_used == 'mixed'


def test_three_gpu_failures_switch_to_cpu(photo_folder: Path, monkeypatch):
    import core.batch as batch

    real = batch.process_image
    switched: list[str] = []

    def broken_gpu(img, s, backend, sr=None):
        if backend.name == 'gpu':
            raise RuntimeError('CUDA error: an illegal memory access was encountered')
        return real(img, s, backend, sr)

    monkeypatch.setattr(batch, 'process_image', broken_gpu)
    report = run_batch(
        photo_folder, AdjustmentSettings(), 'gpu', backend=FakeGpuBackend(), on_device_change=switched.append
    )
    assert switched == ['cpu']
    assert report.succeeded == 10 and report.device_used == 'cpu'


def test_too_large_for_vram_uses_cpu(photo_folder: Path):
    class TinyVram(FakeGpuBackend):
        def mem_info(self):
            return 1000, 4 * 2**30

    report = run_batch(photo_folder, AdjustmentSettings(), 'gpu', backend=TinyVram())
    assert all(f.status == 'warning' and f.device == 'cpu' for f in report.files)
    assert report.files[0].message.startswith('Photo too large for VRAM')


def test_cpu_and_fake_gpu_outputs_match(photo_folder: Path, tmp_path: Path):
    s = AdjustmentSettings(exposure=0.3, highlights=-30, clarity=20, noise_reduction=30, vignette_amount=-20)
    a = run_batch(photo_folder, s, 'cpu', output_dir=tmp_path / 'a', workers=2)
    b = run_batch(photo_folder, s, 'gpu', output_dir=tmp_path / 'b', backend=FakeGpuBackend())
    for fa, fb in zip(a.files, b.files, strict=True):
        assert fa.output_path and fb.output_path
        xa = np.asarray(Image.open(fa.output_path), dtype=np.float32)
        xb = np.asarray(Image.open(fb.output_path), dtype=np.float32)
        assert np.abs(xa - xb).mean() <= 1
