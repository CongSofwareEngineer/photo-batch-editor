"""Make ``assets/app.icns`` (macOS app icon) from ``assets/app.ico``. Runs on any OS (Pillow).

``build_mac.sh`` runs it when the .icns is missing; run it again after changing ``app.ico``.

    python tools/make_icns.py
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / 'assets' / 'app.ico'
DST = ROOT / 'assets' / 'app.icns'


def main() -> int:
    with Image.open(SRC) as ico:
        biggest = max(ico.info.get('sizes', {ico.size}), key=lambda s: s[0] * s[1])
        ico.size = biggest  # pyright: ignore[reportAttributeAccessIssue] - ICO frame choice
        img = ico.convert('RGBA')
    # macOS wants up to 1024 px; upscale the largest .ico frame if needed
    if img.width < 1024:
        img = img.resize((1024, 1024), Image.Resampling.LANCZOS)
    img.save(DST, format='ICNS')
    print(f'icon ready: {DST} (from {biggest[0]}x{biggest[1]})')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
