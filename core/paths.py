"""Resource and data paths that work both from source and from a PyInstaller bundle."""

from __future__ import annotations

import os
import sys
from pathlib import Path

APP_NAME = 'PhotoBatchEditor'


def app_root() -> Path:
    """Folder containing bundled resources (``sys._MEIPASS`` when frozen)."""
    base = getattr(sys, '_MEIPASS', None)
    if base:
        return Path(base)
    return Path(__file__).resolve().parent.parent


def resource_path(*parts: str) -> Path:
    """Absolute path of a bundled resource, e.g. ``resource_path("models", "x.onnx")``."""
    return app_root().joinpath(*parts)


_dll_dirs_added = False


def nvidia_lib_roots() -> list[Path]:
    """Folders that may contain the ``nvidia/<component>`` CUDA runtime pip packages."""
    roots: list[Path] = []
    if getattr(sys, 'frozen', False):
        roots.append(app_root())
    for entry in sys.path:
        try:
            p = Path(entry)
        except TypeError:
            continue
        if p.name in ('site-packages', 'dist-packages'):
            roots.append(p)
    return [r for r in dict.fromkeys(roots) if (r / 'nvidia').is_dir()]


def add_cuda_dll_dirs() -> list[Path]:
    """Make the CUDA/cuDNN DLLs of the ``nvidia-*-cu12`` packages loadable (source and .exe).

    Safe to call many times; returns the folders that were added.
    """
    global _dll_dirs_added
    if _dll_dirs_added:
        return []
    _dll_dirs_added = True
    sub = 'bin' if sys.platform == 'win32' else 'lib'
    added: list[Path] = []
    for root in nvidia_lib_roots():
        for d in sorted((root / 'nvidia').glob(f'*/{sub}')):
            if not d.is_dir():
                continue
            added.append(d)
            if sys.platform == 'win32' and hasattr(os, 'add_dll_directory'):
                try:
                    os.add_dll_directory(str(d))
                except OSError:
                    pass
    if added:
        os.environ['PATH'] = os.pathsep.join([str(d) for d in added] + [os.environ.get('PATH', '')])
    return added


def local_appdata_dir() -> Path:
    """``%LOCALAPPDATA%\\PhotoBatchEditor`` on Windows, ``~/Library/Caches/PhotoBatchEditor`` on
    macOS, ``~/.cache/PhotoBatchEditor`` elsewhere."""
    base = os.environ.get('LOCALAPPDATA')
    if base:
        root = Path(base)
    elif sys.platform == 'darwin':
        root = Path.home() / 'Library' / 'Caches'
    else:
        root = Path.home() / '.cache'
    return root / APP_NAME
