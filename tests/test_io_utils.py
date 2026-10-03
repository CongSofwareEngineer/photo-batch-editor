"""Reading/writing images (spec section 7)."""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest
from PIL import Image, ImageOps

from core.io_utils import apply_orientation, read_image, write_jpeg
from tests.conftest import (
    make_photo_like,
    save_gray,
    save_jpeg_with_orientation,
    save_png,
    save_png_alpha,
    save_tiff16,
    to_u8,
)


def test_unicode_paths_read_write(tmp_path: Path):
    src = save_png(tmp_path / 'ảnh cưới' / 'ảnh 01.png', to_u8(make_photo_like(30, 40)))
    img = read_image(src)
    assert img.pixels.shape == (30, 40, 3) and img.pixels.dtype == np.uint8
    out = tmp_path / 'kết quả' / 'ảnh 01.jpg'
    write_jpeg(out, img.pixels)
    assert out.exists()
    assert read_image(out).pixels.shape == (30, 40, 3)


def test_png_alpha_composited_on_white(tmp_path: Path):
    img = read_image(save_png_alpha(tmp_path / 'a.png')).pixels
    assert img.shape == (40, 50, 3)
    assert (img[:, :20] == 255).all()  # transparent → white
    assert (img[:, 30:] == (255, 0, 0)).all()  # opaque red


def test_tiff16_read(tmp_path: Path):
    img = read_image(save_tiff16(tmp_path / 'x.tif')).pixels
    assert img.dtype == np.uint16 and img.shape == (40, 50, 3)  # kept at full 16-bit precision
    assert tuple(img[0, 0]) == (65535, 32768, 1000)


def test_grayscale_to_three_channels(tmp_path: Path):
    img = read_image(save_gray(tmp_path / 'g.bmp')).pixels
    assert img.shape == (40, 50, 3)
    assert (img[..., 0] == img[..., 1]).all() and (img[..., 1] == img[..., 2]).all()


def test_cmyk_jpeg(tmp_path: Path):
    p = tmp_path / 'cmyk.jpg'
    Image.fromarray(to_u8(make_photo_like(30, 40))).convert('CMYK').save(p, 'JPEG', quality=95)
    img = read_image(p).pixels
    assert img.shape == (30, 40, 3)
    ref = np.asarray(Image.open(p).convert('RGB'))
    assert np.abs(img.astype(int) - ref.astype(int)).mean() < 2


@pytest.mark.parametrize('orientation', range(1, 9))
def test_orientation_matches_pillow(orientation):
    arr = np.arange(4 * 6 * 3, dtype=np.uint8).reshape(4, 6, 3)
    im = Image.fromarray(arr)
    exif = im.getexif()
    exif[0x0112] = orientation
    im.info['exif'] = exif.tobytes()
    im._exif = exif  # noqa: SLF001 - make exif_transpose see the tag
    ref = np.asarray(ImageOps.exif_transpose(im))
    assert np.array_equal(apply_orientation(arr, orientation), ref)


def test_orientation_6_rotated_and_reset(tmp_path: Path):
    src_arr = to_u8(make_photo_like(60, 90))
    src = save_jpeg_with_orientation(tmp_path / 'r.jpg', src_arr, 6)
    img = read_image(src)
    assert img.pixels.shape == (90, 60, 3)  # rotated 90° clockwise
    out = tmp_path / 'out' / 'r.jpg'
    write_jpeg(out, img.pixels, img.exif, img.icc)
    with Image.open(out) as o:
        exif = o.getexif()
        assert exif.get(0x0112) == 1
        assert exif.get(0x010F) == 'TestCam'  # EXIF kept
        ifd = exif.get_ifd(0x8769)
        assert (ifd.get(0xA002), ifd.get(0xA003)) == (60, 90)
        assert o.size == (60, 90)


def test_jpeg_quality_100_444(tmp_path: Path):
    img = read_image(save_jpeg_with_orientation(tmp_path / 'q.jpg', to_u8(make_photo_like(64, 64)), 1))
    out = tmp_path / 'o.jpg'
    write_jpeg(out, img.pixels, img.exif, img.icc)
    with Image.open(out) as o:
        from PIL import JpegImagePlugin

        assert JpegImagePlugin.get_sampling(o) == 0  # 4:4:4
        q = o.quantization  # pyright: ignore[reportAttributeAccessIssue]
        assert all(max(table) == 1 for table in q.values())  # quality 100 → all-ones tables
        assert o.getexif().get(0x010F) == 'TestCam'
    assert not list(tmp_path.glob('*.tmp'))


def test_icc_profile_kept(tmp_path: Path):
    from PIL import ImageCms

    icc = ImageCms.ImageCmsProfile(ImageCms.createProfile('sRGB')).tobytes()
    p = tmp_path / 'icc.png'
    Image.fromarray(to_u8(make_photo_like(20, 20))).save(p, icc_profile=icc)
    img = read_image(p)
    assert img.icc == icc
    out = tmp_path / 'icc.jpg'
    write_jpeg(out, img.pixels, img.exif, img.icc)
    with Image.open(out) as o:
        assert o.info.get('icc_profile') == icc


def test_bad_exif_falls_back(tmp_path: Path):
    exif = Image.Exif()
    exif[0x010E] = 'x' * 70000  # ImageDescription larger than a JPEG APP1 segment
    out = tmp_path / 'big.jpg'
    warnings = write_jpeg(out, to_u8(make_photo_like(20, 20)), exif)
    assert out.exists() and warnings == ['EXIF not kept']


def test_incompressible_image_writes(tmp_path: Path):
    """Pure noise at q100 4:4:4 compresses to more bytes than pixels (Pillow buffer issue)."""
    rng = np.random.default_rng(0)
    for shape in ((300, 400, 3), (1000, 1500, 3)):
        noise = (rng.random(shape) * 255).astype(np.uint8)
        out = tmp_path / f'noise_{shape[0]}.jpg'
        assert write_jpeg(out, noise) == []
        with Image.open(out) as o:
            assert o.size == (shape[1], shape[0])
