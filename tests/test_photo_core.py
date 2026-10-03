"""Photo editor helpers without Qt: history, geometry, effects, export, collage."""

import math

import numpy as np
import pytest
from PIL import Image

from core.backend import get_cpu_backend
from core.photo.collage import TEMPLATES, cell_rects, clamp_offset, cover_rect
from core.photo.effects import PhotoAdjust, apply_photo_adjust, gaussian_blur, pixelate
from core.photo.geometry import (
    ZOOM_MAX,
    ZOOM_MIN,
    clamp_zoom,
    cover_scale,
    crop_rect,
    flip_point,
    next_zoom,
    rotate90_point,
    round_rect,
    zoom_at,
)
from core.photo.history import History
from core.photo.image_io import default_export_path, editor_dir, load_rgba, save_image
from core.pipeline import apply_adjustments, to_uint8
from tests.conftest import make_photo_like, save_png, save_png_alpha, to_u8

# History --------------------------------------------------------------------------------------


def test_history_undo_redo_and_limit():
    h: History[int] = History(limit=50)
    state = 0
    for i in range(1, 41):  # 40 steps (requirement: at least 30)
        h.push(f'step {i}', state)
        state = i
    assert len(h) == 40
    for expected in range(39, -1, -1):
        undone = h.undo(state)
        assert undone == expected
        state = expected
    assert h.undo(state) is None
    redone = h.redo(state)
    assert redone == 1 and h.can_redo()
    state = redone
    h.push('new', state)  # a new change clears the redo list
    assert not h.can_redo()


def test_history_limit_drops_oldest():
    h: History[int] = History(limit=3)
    for i in range(5):
        h.push('s', i)
    assert len(h) == 3
    assert h.undo(99) == 4


def test_history_merges_same_key():
    h: History[int] = History()
    assert h.push('slider', 0, key='a', now=10.0)
    assert not h.push('slider', 1, key='a', now=10.5)  # same drag: merged
    assert h.push('slider', 2, key='a', now=12.0)  # pause > 1 s: new step
    assert h.push('other', 3, key='b', now=12.1)
    assert len(h) == 3
    assert h.undo(4) == 3 and h.undo(3) == 2 and h.undo(2) == 0


# Geometry --------------------------------------------------------------------------------------


def test_zoom_range_and_steps():
    assert clamp_zoom(0.01) == ZOOM_MIN == 0.10
    assert clamp_zoom(100) == ZOOM_MAX == 16.0
    assert next_zoom(1.0, 1) > 1.0 and next_zoom(1.0, -1) < 1.0
    assert next_zoom(16.0, 1) == 16.0 and next_zoom(0.1, -1) == 0.1


def test_zoom_at_keeps_point_under_cursor():
    offset, anchor = (30.0, -12.0), (412.0, 250.0)
    old, new = 0.5, 2.0
    image_pt = ((anchor[0] - offset[0]) / old, (anchor[1] - offset[1]) / old)
    ox, oy = zoom_at(old, new, offset, anchor)
    assert ox + image_pt[0] * new == pytest.approx(anchor[0])
    assert oy + image_pt[1] * new == pytest.approx(anchor[1])


@pytest.mark.parametrize('ratio', [None, 1.0, 4 / 3, 16 / 9, 9 / 16])
def test_crop_rect_ratio_and_bounds(ratio):
    for anchor, pointer in (((100, 100), (900, 400)), ((500, 500), (-50, 20)), ((10, 590), (790, 0))):
        x, y, w, h = crop_rect(anchor, pointer, ratio, (800, 600))
        assert x >= -1e-6 and y >= -1e-6 and x + w <= 800 + 1e-6 and y + h <= 600 + 1e-6
        if ratio:
            assert w / h == pytest.approx(ratio, rel=1e-6)


def test_round_rect_inside_image():
    assert round_rect((-5, -5, 50.4, 20.6), (40, 30)) == (0, 0, 40, 16)


def test_rotate_and_flip_points():
    size = (200, 100)
    assert rotate90_point(0, 0, size, clockwise=True) == (100, 0)
    assert rotate90_point(200, 100, size, clockwise=True) == (0, 200)
    assert rotate90_point(0, 0, size, clockwise=False) == (0, 200)
    x, y = rotate90_point(*rotate90_point(30, 40, size, True), (100, 200), False)
    assert (x, y) == (30, 40)
    assert flip_point(30, 40, size, horizontal=True) == (170, 40)
    assert flip_point(30, 40, size, horizontal=False) == (30, 60)


def test_cover_scale():
    assert cover_scale(400, 300, 0) == pytest.approx(1.0)
    assert cover_scale(400, 300, 10) > 1.0
    k = cover_scale(400, 300, 30)
    # the four canvas corners are inside the rotated, scaled image
    a = math.radians(30)
    for cx, cy in ((-200, -150), (200, -150), (200, 150), (-200, 150)):
        lx = (cx * math.cos(a) + cy * math.sin(a)) / k
        ly = (-cx * math.sin(a) + cy * math.cos(a)) / k
        assert abs(lx) <= 200 + 1e-6 and abs(ly) <= 150 + 1e-6


# Effects --------------------------------------------------------------------------------------


def test_pixelate_blocks_are_uniform_and_aligned():
    rng = np.random.default_rng(0)
    img = np.concatenate(
        [rng.integers(0, 255, (64, 64, 3), dtype=np.uint8), np.full((64, 64, 1), 255, np.uint8)], axis=2
    )
    out = pixelate(img[5:45, 7:47], 8, origin=(7, 5))
    assert out.shape == (40, 40, 4)
    # cells start at absolute multiples of 8: region (7, 5) → first full cell at local (1, 3)
    cell = out[3:11, 1:9]
    assert (cell == cell[0, 0]).all()
    assert out[..., :3].std() < img[..., :3].std()


def test_gaussian_blur_smooths_and_keeps_alpha_edges():
    rng = np.random.default_rng(1)
    img = np.concatenate(
        [rng.integers(0, 255, (50, 50, 3), dtype=np.uint8), np.full((50, 50, 1), 255, np.uint8)], axis=2
    )
    out = gaussian_blur(img, 3.0)
    assert out[..., :3].std() < img[..., :3].std() / 3
    clear = np.zeros((20, 20, 4), np.uint8)
    clear[5:15, 5:15] = (255, 255, 255, 255)
    soft = gaussian_blur(clear, 2.0)
    visible = soft[..., 3] > 30
    assert (soft[visible][:, :3] > 200).all()  # no dark fringe from transparent black


def test_photo_adjust_uses_the_batch_algorithms():
    rgb = to_u8(make_photo_like(60, 80))
    rgba = np.concatenate([rgb, np.full((60, 80, 1), 200, np.uint8)], axis=2)
    adj = PhotoAdjust(brightness=20, contrast=15, saturation=-30, temperature=25, sharpness=40)
    out = apply_photo_adjust(rgba, adj)
    expected = to_uint8(apply_adjustments(rgb, adj.to_settings(), get_cpu_backend()), np)
    assert np.array_equal(out[..., :3], expected)
    assert (out[..., 3] == 200).all()
    assert apply_photo_adjust(rgba, PhotoAdjust()) is rgba


def test_photo_adjust_from_dict_clamps():
    a = PhotoAdjust.from_dict({'brightness': 500, 'sharpness': -3, 'contrast': 'x'})
    assert a.brightness == 100 and a.sharpness == 0 and a.contrast == 0
    assert PhotoAdjust.from_dict(None) == PhotoAdjust()


# Reading / export ------------------------------------------------------------------------------


def test_load_rgba_keeps_transparency(tmp_path):
    rgba = load_rgba(save_png_alpha(tmp_path / 'a.png'))
    assert rgba.shape == (40, 50, 4)
    assert rgba[0, 0, 3] == 0 and rgba[0, -1, 3] == 255


def test_export_paths_use_editor_folder(tmp_path):
    src = tmp_path / 'shoot' / 'ảnh 01.jpg'
    assert editor_dir(src) == tmp_path / 'shoot' / 'editor'
    assert default_export_path(src, 'jpeg') == tmp_path / 'shoot' / 'editor' / 'ảnh 01.jpg'
    assert default_export_path(src, 'png').suffix == '.png'
    assert default_export_path(None, 'jpeg', 'collage').name == 'collage.jpg'


def test_save_image_jpeg_and_png(tmp_path):
    rgb = to_u8(make_photo_like(80, 120))
    rgba = np.concatenate([rgb, np.full((80, 120, 1), 255, np.uint8)], axis=2)
    rgba[:10, :10, 3] = 0
    png = save_image(tmp_path / 'editor' / 'x.png', rgba, 'png')
    with Image.open(png) as im:
        assert im.mode == 'RGBA' and np.array_equal(np.asarray(im), rgba)
    hi = save_image(tmp_path / 'editor' / 'hi.jpg', rgba, 'jpeg', 95)
    lo = save_image(tmp_path / 'editor' / 'lo.jpg', rgba, 'jpeg', 30)
    assert hi.stat().st_size > lo.stat().st_size
    with Image.open(hi) as im:
        px = np.asarray(im.convert('RGB'))
        assert px[2, 2].min() > 240  # transparent corner flattened on white
    assert not list((tmp_path / 'editor').glob('*.tmp'))


def test_save_image_unicode_path(tmp_path):
    rgba = np.zeros((10, 10, 4), np.uint8)
    rgba[..., 3] = 255
    out = save_image(tmp_path / 'thư mục' / 'ảnh.jpg', rgba)
    assert out.exists()
    save_png(tmp_path / 'dummy.png', to_u8(make_photo_like(8, 8)))


# Collage -------------------------------------------------------------------------------------


@pytest.mark.parametrize('template', list(TEMPLATES))
def test_collage_cells_spacing(template):
    cells = cell_rects(template, 1000, 800, 20)
    assert len(cells) == len(TEMPLATES[template])
    for c in cells:
        assert c.x >= 20 - 1e-6 and c.y >= 20 - 1e-6
        assert c.x + c.w <= 980 + 1e-6 and c.y + c.h <= 780 + 1e-6
    for i, a in enumerate(cells):  # no overlap, at least the spacing between cells
        for b in cells[i + 1 :]:
            gap_x = max(b.x - (a.x + a.w), a.x - (b.x + b.w))
            gap_y = max(b.y - (a.y + a.h), a.y - (b.y + b.h))
            assert max(gap_x, gap_y) >= 20 - 1e-6


def test_collage_cover_and_clamp():
    cell = cell_rects('two_horizontal', 1000, 500, 0)[0]
    x, y, w, h = cover_rect(400, 200, cell)
    assert w >= cell.w - 1e-6 and h >= cell.h - 1e-6
    assert x <= cell.x + 1e-6 and y <= cell.y + 1e-6 and x + w >= cell.x + cell.w - 1e-6
    dx, dy = clamp_offset(400, 200, cell, 1.0, (10_000, 10_000))
    x2, y2, w2, h2 = cover_rect(400, 200, cell, 1.0, (dx, dy))
    assert x2 <= cell.x + 1e-6 and y2 <= cell.y + 1e-6  # still covers the cell
