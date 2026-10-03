"""Put ``ffmpeg.exe`` (Windows) / ``ffmpeg`` (macOS) into ``ffmpeg/`` so the build bundles it
(the Video editor needs it).

The binary comes from the ``imageio-ffmpeg`` pip package (``requirements.txt``) of the OS that
builds, so no manual download is needed. Run by ``build_windows.bat`` / ``build_mac.sh`` before
PyInstaller; safe to run again.

    python tools/fetch_ffmpeg.py
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET_DIR = ROOT / 'ffmpeg'
EXE = 'ffmpeg.exe' if sys.platform == 'win32' else 'ffmpeg'

NOTE = """FFmpeg {version}
Source binary: {source}

FFmpeg is free software licensed under the GNU GPL / LGPL (this build includes libx264, so the
GPL applies). Sources and licence texts: https://ffmpeg.org/legal.html and
https://github.com/imageio/imageio-binaries. Photo Batch Editor only runs ffmpeg as a
separate program for the Video editor.
"""


def main() -> int:
    try:
        import imageio_ffmpeg  # noqa: PLC0415
    except ImportError:
        print('imageio-ffmpeg is not installed: pip install -r requirements.txt', file=sys.stderr)
        return 1
    src = Path(imageio_ffmpeg.get_ffmpeg_exe())
    TARGET_DIR.mkdir(exist_ok=True)
    dst = TARGET_DIR / EXE
    if not dst.exists() or dst.stat().st_size != src.stat().st_size:
        shutil.copy2(src, dst)
    if sys.platform != 'win32':
        dst.chmod(0o755)
    version = subprocess.run(
        [str(dst), '-version'],
        capture_output=True,
        text=True,  # noqa: S603
        check=False,
    ).stdout.splitlines()[:1]
    (TARGET_DIR / 'README.txt').write_text(
        NOTE.format(version=version[0] if version else '?', source=src.name), encoding='utf-8'
    )
    print(f'ffmpeg ready: {dst} ({dst.stat().st_size // (1 << 20)} MB)')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
