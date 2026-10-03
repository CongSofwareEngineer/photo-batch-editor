"""Run the app from source with live reload (``dev.bat``) — no PyInstaller build needed.

Edit and save a ``.py`` file → the app restarts by itself in ~2 s on the same page;
edit ``ui/theme.qss`` → the new style is applied instantly. If the app crashes (e.g. a
syntax error), fix the file and save: it starts again. Close the window to stop.
"""

import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
RELOAD_EXIT_CODE = 3  # ui/devtools.py


def stamp() -> str:
    return time.strftime('%H:%M:%S')


def snapshot() -> dict[Path, float]:
    files = [ROOT / 'main.py', *(ROOT / 'core').rglob('*.py'), *(ROOT / 'ui').rglob('*.py')]
    out = {}
    for f in files:
        try:
            out[f] = f.stat().st_mtime
        except OSError:
            pass
    return out


def wait_for_change() -> None:
    before = snapshot()
    while True:
        time.sleep(0.5)
        after = snapshot()
        if after != before:
            names = sorted(
                str(p.relative_to(ROOT)) for p in set(before) | set(after) if before.get(p) != after.get(p)
            )
            print(f'[dev {stamp()}] changed: {", ".join(names)}', flush=True)
            return


def main() -> int:
    env = {**os.environ, 'PBE_DEV': '1', 'PYTHONUNBUFFERED': '1'}
    print(
        f'[dev {stamp()}] live reload on: save a .py file to restart, theme.qss applies instantly. '
        'Close the window (or Ctrl+C) to stop.',
        flush=True,
    )
    while True:
        t0 = time.perf_counter()
        print(f'[dev {stamp()}] starting app...', flush=True)
        try:
            rc = subprocess.call([sys.executable, str(ROOT / 'main.py'), *sys.argv[1:]], cwd=ROOT, env=env)
        except KeyboardInterrupt:
            return 0
        if rc == RELOAD_EXIT_CODE:
            continue
        if rc == 0:
            print(f'[dev {stamp()}] window closed, bye.', flush=True)
            return 0
        print(
            f'[dev {stamp()}] app exited with code {rc} after {time.perf_counter() - t0:.1f}s '
            '- fix the error above and save, it will start again.',
            flush=True,
        )
        try:
            wait_for_change()
        except KeyboardInterrupt:
            return 0


if __name__ == '__main__':
    sys.exit(main())
