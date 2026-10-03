"""Reading images, EXIF orientation, JPEG writing (spec section 7).

Paths may contain Vietnamese / Unicode characters: never ``cv2.imread``/``imwrite``.
"""

from __future__ import annotations

import logging
import os
import subprocess
import sys
import threading
from dataclasses import dataclass
from pathlib import Path

import cv2
import numpy as np
from PIL import Image, ImageFile

log = logging.getLogger(__name__)

TAG_ORIENTATION = 0x0112
TAG_EXIF_IFD = 0x8769
TAG_PIXEL_X = 0xA002
TAG_PIXEL_Y = 0xA003
EXIF_NOT_KEPT = 'EXIF not kept'

Image.MAX_IMAGE_PIXELS = None  # large photos are expected; limits are enforced elsewhere


class ImageReadError(Exception):
    """The file could not be decoded (corrupt or unsupported)."""


@dataclass
class LoadedImage:
    """Decoded, EXIF-oriented RGB pixels plus the metadata to carry to the JPEG."""

    pixels: np.ndarray  # (H, W, 3) RGB, uint8 (8-bit sources) or float32 in [0, 1]
    exif: Image.Exif | None
    icc: bytes | None

    @property
    def width(self) -> int:
        return int(self.pixels.shape[1])

    @property
    def height(self) -> int:
        return int(self.pixels.shape[0])


def read_metadata(path: Path) -> tuple[Image.Exif | None, bytes | None, int, str]:
    """``(exif, icc, orientation, pil_mode)`` read with Pillow (metadata only)."""
    try:
        with Image.open(path) as im:
            exif = im.getexif()
            icc = im.info.get('icc_profile')
            orientation = int(exif.get(TAG_ORIENTATION, 1) or 1) if exif else 1
            return (exif if len(exif) else None), icc, orientation, im.mode
    except Exception:  # noqa: BLE001 - metadata is optional
        log.debug('No metadata for %s', path, exc_info=True)
        return None, None, 1, ''


def image_dimensions(path: Path) -> tuple[int, int] | None:
    """Oriented ``(width, height)`` from the file header, without decoding pixels."""
    try:
        with Image.open(path) as im:
            w, h = im.size
            exif = im.getexif()
            if int(exif.get(TAG_ORIENTATION, 1) or 1) in (5, 6, 7, 8):
                w, h = h, w
            return w, h
    except Exception:  # noqa: BLE001
        return None


def apply_orientation(img: np.ndarray, orientation: int) -> np.ndarray:
    """Apply EXIF Orientation 1–8 so the image displays upright."""
    if orientation == 2:
        img = img[:, ::-1]
    elif orientation == 3:
        img = img[::-1, ::-1]
    elif orientation == 4:
        img = img[::-1]
    elif orientation == 5:
        img = np.swapaxes(img, 0, 1)
    elif orientation == 6:
        img = np.rot90(img, k=-1)
    elif orientation == 7:
        img = np.swapaxes(img, 0, 1)[::-1, ::-1]
    elif orientation == 8:
        img = np.rot90(img, k=1)
    return np.ascontiguousarray(img)


def _decode(path: Path, pil_mode: str) -> np.ndarray:
    """Decode to a NumPy array: BGR/BGRA/gray from OpenCV, RGB for CMYK via Pillow."""
    if pil_mode == 'CMYK':
        with Image.open(path) as im:
            return np.asarray(im.convert('RGB'))[..., ::-1]  # to BGR like OpenCV
    try:
        data = np.fromfile(str(path), dtype=np.uint8)
    except OSError as exc:
        raise ImageReadError(f'Cannot read file: {exc}') from exc
    if data.size == 0:
        raise ImageReadError('Empty file')
    img = cv2.imdecode(data, cv2.IMREAD_UNCHANGED)
    if img is None:
        # OpenCV failed (unusual variants): try Pillow before giving up.
        try:
            with Image.open(path) as im:
                im.load()
                if im.mode in ('I;16', 'I;16B', 'I;16L'):
                    return np.asarray(im).astype(np.uint16)
                return _pil_to_bgr(im)
        except Exception as exc:  # noqa: BLE001
            raise ImageReadError('Corrupt file') from exc
    return img


def _pil_to_bgr(im: Image.Image) -> np.ndarray:
    if 'A' in im.getbands():
        rgba = np.asarray(im.convert('RGBA'))
        return rgba[..., [2, 1, 0, 3]]
    return np.asarray(im.convert('RGB'))[..., ::-1]


def to_rgb(img: np.ndarray) -> np.ndarray:
    """Decoded OpenCV array → RGB, 3 channels, alpha composited over white.

    8-bit sources stay ``uint8`` and 16-bit sources stay ``uint16`` (exact lookup tables in
    the pipeline); other sources become float32 in ``[0, 1]``.
    """
    if img.ndim == 2:
        img = img[..., None]
    out_dtype: type | None = None
    if img.dtype == np.uint8:
        scale, out_dtype = 255.0, np.uint8
    elif img.dtype == np.uint16:
        scale, out_dtype = 65535.0, np.uint16
    elif np.issubdtype(img.dtype, np.floating):
        scale = 1.0
    else:
        img = img.astype(np.float64)
        scale = float(max(img.max(), 1))
    ch = img.shape[2]
    if ch == 1:
        color, alpha = np.repeat(img, 3, axis=2), None
    elif ch == 2:  # gray + alpha
        color, alpha = np.repeat(img[..., :1], 3, axis=2), img[..., 1]
    elif ch == 3:
        color, alpha = img[..., ::-1], None
    else:
        color, alpha = img[..., 2::-1], img[..., 3]
    if alpha is None:
        if out_dtype is not None:
            return np.ascontiguousarray(color)
        return np.ascontiguousarray(np.clip(color.astype(np.float32) / scale, 0, 1), dtype=np.float32)
    a = (alpha.astype(np.float32) / scale)[..., None]
    rgb = color.astype(np.float32) / scale
    out = np.clip(rgb * a + (1.0 - a), 0.0, 1.0)  # composite over white
    if out_dtype is not None:
        return np.rint(out * scale).astype(out_dtype)
    return out.astype(np.float32)


def read_image(path: Path) -> LoadedImage:
    """Read an image file for processing (section 7.1)."""
    path = Path(path)
    exif, icc, orientation, mode = read_metadata(path)
    raw = _decode(path, mode)
    rgb = apply_orientation(to_rgb(raw), orientation)
    return LoadedImage(rgb, exif, icc)


def prepare_exif(exif: Image.Exif | None, width: int, height: int) -> bytes | None:
    """EXIF bytes with Orientation = 1 and updated pixel dimensions (if present)."""
    if exif is None:
        return None
    try:
        exif[TAG_ORIENTATION] = 1
        ifd = exif.get_ifd(TAG_EXIF_IFD)
        if TAG_PIXEL_X in ifd:
            ifd[TAG_PIXEL_X] = int(width)
        if TAG_PIXEL_Y in ifd:
            ifd[TAG_PIXEL_Y] = int(height)
        return exif.tobytes()
    except Exception:  # noqa: BLE001
        log.debug('EXIF could not be serialised', exc_info=True)
        return None


def tmp_path_for(out_path: Path) -> Path:
    return out_path.with_name(out_path.name + '.tmp')


_maxblock_lock = threading.Lock()


def _save_jpeg(img: Image.Image, path: Path, **kw: object) -> None:
    """``img.save`` as JPEG, robust to very noisy images.

    With ``optimize=True`` Pillow encodes into one buffer of about 2 bytes per pixel; a q100
    4:4:4 JPEG of very grainy content can be larger and fails ("Suspension not allowed
    here"). Then retry once with the buffer floor (``ImageFile.MAXBLOCK``) raised to
    4 bytes per pixel. The retry is serialized because MAXBLOCK is a global.
    """
    try:
        img.save(path, 'JPEG', **kw)
        return
    except OSError:
        log.info('Retrying %s with a larger JPEG buffer', Path(path).name)
    need = img.size[0] * img.size[1] * 4 + 1_048_576
    with _maxblock_lock:
        old = ImageFile.MAXBLOCK
        ImageFile.MAXBLOCK = max(old, need)
        try:
            img.save(path, 'JPEG', **kw)
        finally:
            ImageFile.MAXBLOCK = old


def write_jpeg(
    out_path: Path, pixels: np.ndarray, exif: Image.Exif | None = None, icc: bytes | None = None
) -> list[str]:
    """Write RGB uint8 as JPEG q100 4:4:4 via a ``.tmp`` file + rename. Returns warnings."""
    out_path = Path(out_path)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    h, w = pixels.shape[:2]
    img = Image.fromarray(np.ascontiguousarray(pixels, dtype=np.uint8), 'RGB')
    tmp = tmp_path_for(out_path)
    base = {'quality': 100, 'subsampling': 0, 'optimize': True}
    if icc:
        base['icc_profile'] = icc
    warnings: list[str] = []
    exif_bytes = prepare_exif(exif, w, h)
    if exif is not None and exif_bytes is None:
        warnings.append(EXIF_NOT_KEPT)
    try:
        try:
            if exif_bytes:
                _save_jpeg(img, tmp, exif=exif_bytes, **base)
            else:
                _save_jpeg(img, tmp, **base)
        except Exception:  # noqa: BLE001 - corrupt or too-large EXIF
            if not exif_bytes:
                raise
            log.info('Writing %s without EXIF', out_path.name)
            warnings.append(EXIF_NOT_KEPT)
            _save_jpeg(img, tmp, **base)
        os.replace(tmp, out_path)
    finally:
        if tmp.exists():
            try:
                tmp.unlink()
            except OSError:
                pass
    return warnings


def open_in_file_manager(path: Path, select: bool = False) -> None:
    """Open a folder (or reveal a file) in File Explorer / Finder / the desktop file manager."""
    path = Path(path)
    if sys.platform == 'win32':
        if select:
            subprocess.Popen(f'explorer /select,"{path}"')  # noqa: S603
        else:
            os.startfile(str(path))  # type: ignore[attr-defined]  # noqa: S606
    elif sys.platform == 'darwin':
        subprocess.Popen(['open', '-R', str(path)] if select else ['open', str(path)])  # noqa: S603, S607
    else:
        target = path.parent if select else path
        subprocess.Popen(['xdg-open', str(target)])  # noqa: S603, S607


def open_with_default_app(path: Path) -> None:
    if sys.platform == 'win32':
        os.startfile(str(path))  # type: ignore[attr-defined]  # noqa: S606
    elif sys.platform == 'darwin':
        subprocess.Popen(['open', str(path)])  # noqa: S603, S607
    else:
        subprocess.Popen(['xdg-open', str(path)])  # noqa: S603, S607
