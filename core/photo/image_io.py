"""Reading and exporting single photos (keeps transparency, unlike the batch reader)."""

from __future__ import annotations

import os
from pathlib import Path

import numpy as np
from PIL import Image, ImageOps

from core.io_utils import _save_jpeg, read_image, tmp_path_for

EDITOR_FOLDER = 'editor'
EXPORT_FORMATS = {'jpeg': '.jpg', 'png': '.png'}
OPEN_FILTER_EXTS = ('.jpg', '.jpeg', '.png', '.tif', '.tiff', '.webp', '.bmp')


def load_rgba(path: Path) -> np.ndarray:
    """EXIF-oriented ``(H, W, 4)`` uint8 RGBA; transparency is kept (PNG, WebP…)."""
    path = Path(path)
    try:
        with Image.open(path) as im:
            if im.mode.startswith(('I', 'F')):
                raise ValueError('high bit depth')
            im = ImageOps.exif_transpose(im)
            return np.ascontiguousarray(np.asarray(im.convert('RGBA')))
    except Exception:  # noqa: BLE001 - 16-bit TIFF etc.: the batch reader handles them
        px = read_image(path).pixels
        if px.dtype != np.uint8:
            px = np.rint(np.clip(px, 0, 1) * 255).astype(np.uint8)
        alpha = np.full(px.shape[:2] + (1,), 255, np.uint8)
        return np.ascontiguousarray(np.concatenate([px, alpha], axis=2))


def editor_dir(source: Path | None) -> Path:
    """``<folder of the photo>/editor`` (or ``Pictures/editor`` for a new image)."""
    if source is not None:
        return Path(source).parent / EDITOR_FOLDER
    pictures = Path.home() / 'Pictures'
    return (pictures if pictures.is_dir() else Path.home()) / EDITOR_FOLDER


def default_export_path(source: Path | None, fmt: str, fallback_name: str = 'image') -> Path:
    stem = Path(source).stem if source is not None else fallback_name
    return editor_dir(source) / f'{stem}{EXPORT_FORMATS.get(fmt, ".jpg")}'


def flatten_on(rgba: np.ndarray, color: tuple[int, int, int] = (255, 255, 255)) -> np.ndarray:
    """RGBA → RGB composited over a solid colour (JPEG has no transparency)."""
    a = rgba[..., 3:4].astype(np.float32) / 255.0
    bg = np.asarray(color, np.float32).reshape(1, 1, 3)
    out = rgba[..., :3].astype(np.float32) * a + bg * (1.0 - a)
    return np.clip(np.rint(out), 0, 255).astype(np.uint8)


def save_image(path: Path, rgba: np.ndarray, fmt: str = 'jpeg', quality: int = 92) -> Path:
    """Write ``rgba`` as JPEG (``quality`` 1–100) or PNG, via a ``.tmp`` file + rename.

    Protecting the original is the caller's job (the default path is in the ``editor`` folder).
    """
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = tmp_path_for(path)
    try:
        if fmt == 'png':
            Image.fromarray(np.ascontiguousarray(rgba), 'RGBA').save(
                tmp, 'PNG', optimize=False, compress_level=6
            )
        else:
            q = int(min(100, max(1, quality)))
            img = Image.fromarray(flatten_on(rgba), 'RGB')
            _save_jpeg(img, tmp, quality=q, subsampling=0 if q >= 90 else 2, optimize=True)
        os.replace(tmp, path)
    finally:
        if tmp.exists():
            try:
                tmp.unlink()
            except OSError:
                pass
    return path
