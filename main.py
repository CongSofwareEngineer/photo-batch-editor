"""Photo Batch Editor — entry point.

``PhotoBatchEditor.exe --selftest report.json`` runs a quick diagnostic without the UI
(GPU detection, a small batch on every available device, Super Resolution) and writes the
results as JSON — useful to check a packaged build on the target machine.
"""

import multiprocessing
import sys


def selftest(report_path: str) -> int:
    import json
    import tempfile
    import time
    import traceback
    from pathlib import Path

    import numpy as np
    from PIL import Image

    from core.backend import detect_gpu
    from core.batch import run_batch
    from core.enhance import SuperResolver, cuda_provider_available
    from core.paths import add_cuda_dll_dirs
    from core.settings import AdjustmentSettings

    out: dict = {'python': sys.version, 'frozen': bool(getattr(sys, 'frozen', False))}
    try:
        out['cuda_dll_dirs'] = [str(p) for p in add_cuda_dll_dirs()]
        t0 = time.perf_counter()
        info = detect_gpu()
        out['gpu'] = {**info.__dict__, 'detect_seconds': round(time.perf_counter() - t0, 2)}
        out['sr_cuda_provider'] = cuda_provider_available()
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / 'selftest'
            src.mkdir()
            rng = np.random.default_rng(0)
            for i in range(4):
                Image.fromarray((rng.random((300, 400, 3)) * 255).astype(np.uint8)).save(src / f'img_{i}.jpg')
            s = AdjustmentSettings(
                exposure=0.3, shadows=20, clarity=15, noise_reduction=20, vignette_amount=-20
            )
            devices = ['cpu'] + (['gpu'] if info.available else [])
            for dev in devices:
                rep = run_batch(src, s, dev, output_dir=Path(tmp) / f'out_{dev}')
                out[f'batch_{dev}'] = {
                    'succeeded': rep.succeeded,
                    'total': len(rep.files),
                    'device_used': rep.device_used,
                    'seconds': round(rep.total_seconds, 2),
                    'messages': [f.message for f in rep.files if f.message],
                }
            if info.available:
                a = np.asarray(Image.open(Path(tmp) / 'out_cpu' / 'img_0.jpg'), np.float32)
                b = np.asarray(Image.open(Path(tmp) / 'out_gpu' / 'img_0.jpg'), np.float32)
                out['gpu_vs_cpu_mean_diff_255'] = float(np.abs(a - b).mean())
        img = rng.random((64, 64, 3)).astype(np.float32)
        for cuda in [False, True] if out['sr_cuda_provider'] else [False]:
            r = SuperResolver(use_cuda=cuda)
            t0 = time.perf_counter()
            res = r.upscale(img, 2)
            out[f'super_resolution_{"cuda" if cuda else "cpu"}'] = {
                'shape': list(res.shape),
                'device': r.device,
                'seconds': round(time.perf_counter() - t0, 2),
            }
        out['ok'] = True
    except Exception:  # noqa: BLE001
        out['ok'] = False
        out['error'] = traceback.format_exc()
    Path(report_path).write_text(json.dumps(out, indent=2, default=str), encoding='utf-8')
    return 0 if out['ok'] else 1


def main() -> int:
    if len(sys.argv) >= 3 and sys.argv[1] == '--selftest':
        return selftest(sys.argv[2])
    # Qt is imported here, not at module level: worker processes (spawn) re-import this
    # module and must not load the UI.
    from ui.main_window import run_app

    return run_app()


if __name__ == '__main__':
    multiprocessing.freeze_support()  # required for the PyInstaller .exe
    sys.exit(main())
