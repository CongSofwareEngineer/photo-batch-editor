"""Sinh fixture tham chiếu cho bản port Rust (xem docs/instruction/rust-port.md).

Chạy từ gốc dự án::

    python3 tools/gen_rust_fixtures.py

Ghi vào ``rust/core/tests/fixtures/``: các mảng input + output do **bản Python**
(NumPy + OpenCV) tạo ra, kèm ``manifest.json``. Test Rust nạp cùng input, tự tính, rồi so
với output này — đó là bằng chứng bản Rust giữ nguyên hành vi.

Chạy lại script này mỗi khi logic số học của ``core/`` thay đổi.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from core import adjustments as adj  # noqa: E402
from core.backend import get_cpu_backend  # noqa: E402
from core.pipeline import apply_adjustments, process_image  # noqa: E402
from core.settings import AdjustmentSettings  # noqa: E402
from tests.conftest import make_photo_like, to_u8  # noqa: E402

OUT = ROOT / 'rust' / 'core' / 'tests' / 'fixtures'

# Bộ thông số phủ từng nhóm của pipeline (section 6) + một bộ bật tất cả.
SETTING_CASES: dict[str, dict] = {
    'identity': {},
    'white_balance': dict(temperature=20, tint=-10),
    'tone_pointwise': dict(exposure=0.4, brightness=10, contrast=20),
    'tone_local': dict(highlights=-30, shadows=30, whites=10, blacks=-10),
    'color': dict(vibrance=20, saturation=10),
    'detail_clarity_sharpen': dict(clarity=30, sharpening_amount=40),
    'detail_noise': dict(noise_reduction=40),
    'effects_vignette': dict(vignette_amount=-30),
    'all': dict(
        temperature=20,
        tint=-10,
        exposure=0.4,
        brightness=10,
        contrast=20,
        highlights=-30,
        shadows=30,
        whites=10,
        blacks=-10,
        clarity=30,
        vibrance=20,
        saturation=10,
        sharpening_amount=40,
        noise_reduction=40,
        vignette_amount=-30,
    ),
}


class Manifest:
    def __init__(self) -> None:
        self.arrays: dict[str, dict] = {}

    def add(self, name: str, arr: np.ndarray) -> None:
        if arr.dtype == np.float32:
            dtype = 'f32'
        elif arr.dtype == np.uint8:
            dtype = 'u8'
        else:
            raise TypeError(f'{name}: dtype {arr.dtype} không được hỗ trợ')
        data = np.ascontiguousarray(arr)
        (OUT / f'{name}.bin').write_bytes(data.tobytes())
        self.arrays[name] = {'shape': list(data.shape), 'dtype': dtype, 'file': f'{name}.bin'}

    def write(self) -> None:
        payload = {'arrays': self.arrays}
        (OUT / 'manifest.json').write_text(
            json.dumps(payload, indent=1, sort_keys=True) + '\n', encoding='utf-8'
        )


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob('*.bin'):
        old.unlink()
    cpu = get_cpu_backend()
    m = Manifest()

    # --- Backend: mặt phẳng 2D (blur / box / block mean / resize) --------------------------
    plane = make_photo_like(48, 64, seed=5)[..., 1].astype(np.float32)  # kênh G, 48x64
    m.add('plane', plane)
    for sigma in (1.0, 2.5, 8.0, 20.0):
        tag = str(sigma).replace('.', '_')
        m.add(f'blur_sigma_{tag}', cpu.gaussian_blur(plane, sigma).astype(np.float32))
    for r in (1, 3, 7):
        m.add(f'box_r{r}', cpu.box_filter(plane, r).astype(np.float32))
    m.add('block_mean_f4', cpu._block_mean(plane, 4, 0, 0).astype(np.float32))  # noqa: SLF001
    m.add('block_mean_f5_pad', cpu._block_mean(plane, 5, 2, 1).astype(np.float32))  # noqa: SLF001
    m.add('resize_linear_97x131', cpu._resize_linear(plane, 97, 131).astype(np.float32))  # noqa: SLF001
    m.add('resize_linear_19x23', cpu._resize_linear(plane, 19, 23).astype(np.float32))  # noqa: SLF001
    m.add('upscale_f3', cpu.upscale(cpu.downscale(plane, 3), 48, 64, 3).astype(np.float32))

    # --- Guided filter (Noise Reduction) ---------------------------------------------------
    gf = adj.guided_filter(cpu, plane, [plane], 4, 0.0009)[0]
    m.add('guided_r4_eps9e-4', np.ascontiguousarray(gf, dtype=np.float32))

    # --- Adjustments / pipeline ------------------------------------------------------------
    img_f = make_photo_like(60, 80, seed=2)
    img_u8 = to_u8(img_f)
    m.add('img_f32', img_f)
    m.add('img_u8', img_u8)
    for name, kw in SETTING_CASES.items():
        s = AdjustmentSettings(**kw)
        m.add(f'adj_f32_{name}', np.ascontiguousarray(apply_adjustments(img_f, s, cpu), np.float32))
        m.add(f'adj_u8in_{name}', np.ascontiguousarray(apply_adjustments(img_u8, s, cpu), np.float32))
        out, warnings = process_image(img_u8, s, cpu)
        assert warnings == [], (name, warnings)
        m.add(f'proc_u8_{name}', np.ascontiguousarray(out, np.uint8))

    # (Image Size resample chỉ được test theo tính chất bên Rust — xem resize.rs — nên không
    #  cần fixture pixel ở đây.)

    m.write()
    print(f'{len(m.arrays)} mảng -> {OUT}')


if __name__ == '__main__':
    main()
