"""Sinh fixture I/O + thư mục batch cho bản port Rust (xem docs/instruction/rust-port.md).

Chạy từ gốc dự án bằng venv::

    venv/bin/python tools/gen_io_fixtures.py

- ``rust/core/tests/fixtures/io/``: các file ảnh + mảng pixel kỳ vọng do ``core.io_utils.read_image``
  tạo ra + ``io_manifest.json``.
- ``rust/core/tests/fixtures/batch_src/clientA/``: thư mục 10 ảnh y như ``conftest.photo_folder``.

Chạy lại khi logic I/O của ``core/`` thay đổi.
"""

from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from PIL import Image  # noqa: E402

from core.io_utils import read_image  # noqa: E402
from tests.conftest import (  # noqa: E402
    make_photo_like,
    save_gray,
    save_jpeg_with_orientation,
    save_png,
    save_png_alpha,
    save_tiff16,
    to_u8,
)

FX = ROOT / 'rust' / 'core' / 'tests' / 'fixtures'
IO = FX / 'io'
BATCH = FX / 'batch_src'


def dtype_tag(arr: np.ndarray) -> str:
    return {np.dtype('uint8'): 'u8', np.dtype('uint16'): 'u16', np.dtype('float32'): 'f32'}[arr.dtype]


def main() -> None:
    if IO.exists():
        shutil.rmtree(IO)
    IO.mkdir(parents=True)
    manifest: dict[str, dict] = {}

    def record(key: str, src_file: Path) -> None:
        px = read_image(src_file).pixels
        px = np.ascontiguousarray(px)
        (IO / f'{key}.bin').write_bytes(px.tobytes())
        manifest[key] = {
            'file': src_file.name,
            'shape': list(px.shape),
            'dtype': dtype_tag(px),
        }

    record('png_rgb', save_png(IO / 'png_rgb.png', to_u8(make_photo_like(30, 40))))
    record('png_alpha', save_png_alpha(IO / 'png_alpha.png'))
    record('tiff16', save_tiff16(IO / 'tiff16.tif'))
    record('gray', save_gray(IO / 'gray.bmp'))
    for o in range(1, 9):
        record(f'orient_{o}', save_jpeg_with_orientation(IO / f'orient_{o}.jpg', to_u8(make_photo_like(60, 90)), o))

    (IO / 'io_manifest.json').write_text(
        json.dumps({'arrays': manifest}, indent=1, sort_keys=True) + '\n', encoding='utf-8'
    )

    # Thư mục batch giống conftest.photo_folder (10 ảnh + thư mục con).
    if BATCH.exists():
        shutil.rmtree(BATCH)
    folder = BATCH / 'clientA'
    for i in range(6):
        save_png(folder / f'IMG_{i:03d}.png', to_u8(make_photo_like(60 + i * 4, 80, seed=i)))
    save_jpeg_with_orientation(folder / 'rotated.jpg', to_u8(make_photo_like(60, 90)), 6)
    save_png_alpha(folder / 'alpha.png')
    save_tiff16(folder / 'outdoor' / 'ảnh cưới 01.tif')
    save_gray(folder / 'outdoor' / 'gray.bmp')

    # Kích thước (đã xoay) kỳ vọng của từng file, để test batch so sánh.
    expected = {}
    for p in sorted(folder.rglob('*')):
        if p.is_file():
            img = read_image(p)
            rel = p.relative_to(folder).as_posix()
            expected[rel] = {'width': img.width, 'height': img.height}
    (BATCH / 'expected.json').write_text(
        json.dumps(expected, indent=1, sort_keys=True, ensure_ascii=False) + '\n', encoding='utf-8'
    )

    print(f'io: {len(manifest)} mảng; batch: {len(expected)} file -> {FX}')


if __name__ == '__main__':
    main()
