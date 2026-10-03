"""Photo editor document, rendering, project files and page (Qt, offscreen)."""

from pathlib import Path

import numpy as np
import pytest
from PIL import Image

from core.photo.effects import PhotoAdjust
from tests.conftest import make_photo_like, pump, save_png, to_u8


def noisy_rgba(h: int = 200, w: int = 300, seed: int = 0) -> np.ndarray:
    rng = np.random.default_rng(seed)
    return np.concatenate(
        [rng.integers(0, 255, (h, w, 3), dtype=np.uint8), np.full((h, w, 1), 255, np.uint8)], axis=2
    )


@pytest.fixture
def doc(qapp):
    from ui.photo.document import Document
    from ui.photo.render import qimage_from_rgba

    return Document(qimage_from_rgba(noisy_rgba()), Path('C:/photos/x.jpg'))


@pytest.fixture
def renderer(qapp):
    from ui.photo.render import BaseCache, Renderer

    return Renderer(BaseCache())


def render(renderer, doc) -> np.ndarray:
    from ui.photo.render import rgba_from_qimage

    return rgba_from_qimage(renderer.render_image(doc.state))


def test_qimage_roundtrip(qapp):
    from ui.photo.render import qimage_from_rgba, rgba_from_qimage

    a = noisy_rgba(31, 17)
    assert np.array_equal(rgba_from_qimage(qimage_from_rgba(a)), a)


def test_background_untouched_without_layers(doc, renderer):
    assert np.array_equal(render(renderer, doc), noisy_rgba())


def test_text_layer_draws_vietnamese_text(doc, renderer):
    from ui.photo.document import TextLayer

    doc.add_layer(TextLayer(text='Đà Nẵng', font_size=40, color='#ff0000', x=150, y=100), 'Add text')
    out = render(renderer, doc)
    red = (out[..., 0] > 240) & (out[..., 1] < 20) & (out[..., 2] < 20)
    assert red[70:130, 60:240].sum() > 200
    assert doc.layers[0].name == 'Đà Nẵng'


def test_blur_and_pixelate_layers(doc, renderer):
    from ui.photo.document import BlurLayer, Stroke

    base = noisy_rgba()
    doc.add_layer(BlurLayer(shape='rect', rect=(20, 20, 80, 80), strength=50), 'Blur area')
    doc.add_layer(
        BlurLayer(shape='ellipse', mode='pixelate', rect=(150, 20, 100, 80), strength=60), 'Blur area'
    )
    doc.add_layer(
        BlurLayer(shape='brush', strokes=[Stroke(12, ((40, 160), (260, 160)))], strength=50), 'Blur area'
    )
    out = render(renderer, doc)
    assert out[30:90, 30:90, :3].std() < base[30:90, 30:90, :3].std() / 3
    assert out[50:70, 185:215, :3].std() < base[50:70, 185:215, :3].std() / 2
    assert out[155:165, 60:240, :3].std() < base[155:165, 60:240, :3].std() / 2
    assert np.array_equal(out[130:145, 5:15], base[130:145, 5:15])  # outside: unchanged


def test_hidden_layer_and_order(doc, renderer):
    from ui.photo.document import BlurLayer

    lay = doc.add_layer(BlurLayer(rect=(0, 0, 300, 200), strength=50), 'Blur area')
    doc.set_visible(lay.id, False)
    assert np.array_equal(render(renderer, doc), noisy_rgba())
    doc.set_visible(None, False)  # background hidden → transparent
    assert render(renderer, doc)[..., 3].max() == 0


def test_layer_operations_and_30_undo_steps(doc):
    from ui.photo.document import TextLayer

    a = doc.add_layer(TextLayer(text='A', x=10, y=10), 'Add text')
    b = doc.add_layer(TextLayer(text='B', x=20, y=20), 'Add text')
    assert [lay.id for lay in doc.layers] == [a.id, b.id]
    doc.move_layer(b.id, -1)
    assert [lay.id for lay in doc.layers] == [b.id, a.id]
    for i in range(35):
        doc.edit_layer(a.id, 'Move layer', x=float(100 + i))
    assert doc.layer(a.id).x == 134
    for _ in range(35):
        assert doc.undo()
    assert doc.layer(a.id).x == 10
    assert doc.redo() and doc.layer(a.id).x == 100
    doc.remove_layer(a.id)
    assert doc.layer(a.id) is None
    doc.undo()
    assert doc.layer(a.id) is not None


def test_crop_rotate_flip_move_layers(doc):
    from ui.photo.document import BlurLayer, ImageLayer, TextLayer

    t = doc.add_layer(TextLayer(text='T', x=100, y=50), 'Add text')
    b = doc.add_layer(BlurLayer(rect=(10, 20, 30, 40)), 'Blur area')
    doc.crop((50, 10, 200, 150))
    assert doc.size == (200, 150)
    assert (doc.layer(t.id).x, doc.layer(t.id).y) == (50, 40)
    assert doc.layer(b.id).rect == (-40, 10, 30, 40)
    doc.rotate90(clockwise=True)
    assert doc.size == (150, 200)
    assert (doc.layer(t.id).x, doc.layer(t.id).y) == (110, 50)
    assert doc.layer(t.id).rotation == 90
    doc.flip(horizontal=True)
    assert doc.layer(t.id).x == 40
    im = doc.add_layer(ImageLayer(x=10, y=10, width=20, height=10), 'Insert image')
    doc.flip(horizontal=True)
    assert doc.layer(im.id).flip_h
    for _ in range(5):
        doc.undo()
    assert doc.size == (300, 200)


def test_adjust_preview_and_full_resolution(qapp):
    from ui.photo.document import Document
    from ui.photo.render import PREVIEW_LONG, BaseCache, qimage_from_rgba

    rgba = np.concatenate(
        [
            to_u8(make_photo_like(900, PREVIEW_LONG + 400)),
            np.full((900, PREVIEW_LONG + 400, 1), 255, np.uint8),
        ],
        axis=2,
    )
    d = Document(qimage_from_rgba(rgba))
    d.set_adjust(PhotoAdjust(brightness=40))
    ready = []
    cache = BaseCache(lambda: ready.append(1))
    prev = cache.preview(d.background)
    assert max(prev.width(), prev.height()) == PREVIEW_LONG
    assert cache.full(d.background) is None  # computed in the background
    for _ in range(300):
        pump(0.02)
        if ready:
            break
    full = cache.full(d.background)
    assert full is not None and full.width() == PREVIEW_LONG + 400


def test_project_roundtrip(doc, renderer, tmp_path):
    from ui.photo.document import BlurLayer, ImageLayer, Stroke, TextLayer
    from ui.photo.project_io import load_project, save_project
    from ui.photo.render import qimage_from_rgba

    doc.add_layer(
        TextLayer(text='Chữ có dấu', font_size=30, x=100, y=60, bold=True, outline_width=2), 'Add text'
    )
    doc.add_layer(
        ImageLayer(
            image=qimage_from_rgba(noisy_rgba(20, 30, 5)), x=200, y=100, width=60, height=40, rotation=15
        ),
        'Insert image',
    )
    doc.add_layer(
        BlurLayer(shape='brush', mode='pixelate', strokes=[Stroke(8, ((10, 10), (50, 60)))]), 'Blur area'
    )
    doc.set_adjust(PhotoAdjust(contrast=20))
    path = tmp_path / 'dự án.pbep'
    save_project(doc, path)
    assert not doc.modified and doc.project_path == path
    loaded = load_project(path)
    assert [lay.kind for lay in loaded.layers] == ['text', 'image', 'blur']
    text, blur = loaded.layers[0], loaded.layers[2]
    assert isinstance(text, TextLayer) and isinstance(blur, BlurLayer)
    assert text.text == 'Chữ có dấu' and text.bold
    assert blur.strokes[0].points == ((10, 10), (50, 60))
    assert loaded.background.adjust == PhotoAdjust(contrast=20)
    assert np.array_equal(render(renderer, loaded), render(renderer, doc))


def test_project_errors(qapp, tmp_path):
    from ui.photo.project_io import ProjectError, load_project

    bad = tmp_path / 'bad.pbep'
    bad.write_bytes(b'not a zip')
    with pytest.raises(ProjectError):
        load_project(bad)


def test_canvas_zoom_keeps_cursor_point(doc):
    from PySide6.QtCore import QPointF

    from ui.photo.canvas import PhotoCanvas

    c = PhotoCanvas()
    c.resize(800, 600)
    c.set_document(doc)
    assert c.scale == 1.0  # opened: fit, but never enlarged
    anchor = QPointF(333, 222)
    before = c.to_image(anchor)
    c.zoom_to(4.0, anchor)
    after = c.to_image(anchor)
    assert (before.x(), before.y()) == pytest.approx((after.x(), after.y()))
    assert c.scale == 4.0
    c.zoom_to(1000, anchor)
    assert c.scale == 16.0
    c.fit()  # Ctrl+0 enlarges a small photo to the window (Photoshop "Fit on Screen")
    assert c.fit_mode and c.scale == pytest.approx(min((800 - 48) / 300, (600 - 48) / 200))


def _mouse(kind: str, pos, mods=None):
    from PySide6.QtCore import QEvent, QPointF, Qt
    from PySide6.QtGui import QMouseEvent

    t = {
        'press': QEvent.Type.MouseButtonPress,
        'move': QEvent.Type.MouseMove,
        'release': QEvent.Type.MouseButtonRelease,
        'double': QEvent.Type.MouseButtonDblClick,
    }[kind]
    button = Qt.MouseButton.NoButton if kind == 'move' else Qt.MouseButton.LeftButton
    buttons = Qt.MouseButton.NoButton if kind == 'release' else Qt.MouseButton.LeftButton
    p = QPointF(*pos)
    return QMouseEvent(t, p, p, button, buttons, mods or Qt.KeyboardModifier.NoModifier)


def _drag(canvas, a, b, mods=None, steps: int = 5) -> None:
    canvas.mousePressEvent(_mouse('press', a, mods))
    for k in range(1, steps + 1):
        canvas.mouseMoveEvent(
            _mouse('move', (a[0] + (b[0] - a[0]) * k / steps, a[1] + (b[1] - a[1]) * k / steps), mods)
        )
    canvas.mouseReleaseEvent(_mouse('release', b, mods))


@pytest.fixture
def canvas(doc):
    from PySide6.QtCore import QPointF

    from ui.photo.canvas import PhotoCanvas

    c = PhotoCanvas()
    c.resize(400, 300)
    c.set_document(doc)
    c.scale, c.offset, c.fit_mode = 1.0, QPointF(0, 0), False  # screen px = image px
    return c


def test_select_tool_moves_resizes_and_rotates(doc, canvas):
    from PySide6.QtCore import Qt

    from ui.photo.document import ImageLayer
    from ui.photo.render import qimage_from_rgba

    lay = doc.add_layer(
        ImageLayer(image=qimage_from_rgba(noisy_rgba(20, 40)), x=100, y=100, width=80, height=40),
        'Insert image',
    )
    doc.select(None)
    canvas.set_tool('select')
    n = len(doc.history)
    _drag(canvas, (100, 100), (130, 110))  # click selects, drag moves
    assert doc.selected_id == lay.id
    assert (lay.x, lay.y) == pytest.approx((130, 110))
    assert len(doc.history) == n + 1  # one undo step per drag
    # bottom-right corner handle with Shift: keeps the 2:1 ratio, top-left stays put
    _drag(canvas, (170, 130), (210, 135), mods=Qt.KeyboardModifier.ShiftModifier)
    assert lay.width / lay.height == pytest.approx(2.0)
    assert (lay.x - lay.width / 2, lay.y - lay.height / 2) == pytest.approx((90, 90))
    # rotation handle above the top edge
    top = lay.y - lay.height / 2
    _drag(canvas, (lay.x, top - 26), (lay.x + 200, lay.y))
    assert lay.rotation == pytest.approx(90, abs=1)
    doc.undo()
    assert lay.rotation == 0 or doc.layer(lay.id).rotation == 0


def test_crop_tool_with_ratio_and_enter(doc, canvas):
    from PySide6.QtCore import QEvent, Qt
    from PySide6.QtGui import QKeyEvent

    canvas.set_tool('crop')
    tool = canvas.tools['crop']
    tool.set_ratio(1.0)
    _drag(canvas, (10, 10), (210, 110))
    assert tool.rect.width() == pytest.approx(tool.rect.height())
    canvas.keyPressEvent(QKeyEvent(QEvent.Type.KeyPress, Qt.Key.Key_Return, Qt.KeyboardModifier.NoModifier))
    assert doc.size[0] == doc.size[1] == 190  # clamped to the 200 px image height from y = 10
    doc.undo()
    assert doc.size == (300, 200)


def test_blur_tool_creates_layers(doc, canvas):
    canvas.set_tool('blur')
    tool = canvas.tools['blur']
    tool.shape = 'ellipse'
    _drag(canvas, (20, 20), (120, 90))
    assert doc.layers[-1].kind == 'blur' and doc.layers[-1].shape == 'ellipse'
    tool.shape, tool.brush_size = 'brush', 20
    _drag(canvas, (150, 150), (250, 160))
    brush = doc.layers[-1]
    assert brush.shape == 'brush' and len(brush.strokes) == 1
    _drag(canvas, (150, 180), (250, 190))  # same selected brush layer gets the stroke
    assert len(doc.layers) == 2 and len(brush.strokes) == 2


def test_photo_editor_view_open_and_export(qapp, tmp_path, monkeypatch):
    from ui.photo.photo_editor_view import ExportDialog, PhotoEditorView

    src = save_png(tmp_path / 'shoot' / 'ảnh gốc.png', to_u8(make_photo_like(120, 160)))
    v = PhotoEditorView()
    v.resize(1300, 800)
    v.open_path(src, confirm=False)
    for _ in range(300):
        pump(0.02)
        if v.doc is not None:
            break
    assert v.doc is not None and v.doc.size == (160, 120)
    v.doc.set_adjust(PhotoAdjust(brightness=30))
    assert v.undo_btn.isEnabled()
    dlg = ExportDialog(v.doc, v)
    assert Path(dlg.path.text()) == tmp_path / 'shoot' / 'editor' / 'ảnh gốc.jpg'
    monkeypatch.setattr(ExportDialog, 'exec', lambda self: 1)
    v.export_dialog()
    out = tmp_path / 'shoot' / 'editor' / 'ảnh gốc.jpg'
    for _ in range(300):
        pump(0.02)
        if out.exists() and not v._busy:
            break
    assert out.exists()
    with Image.open(out) as im:
        assert im.size == (160, 120)
    assert src.exists()  # the original is untouched
