"""Photo editor document: background photo + layers, with snapshot undo/redo.

The original file is never written: the document holds the decoded pixels (``QImage``,
implicitly shared, never modified in place) and every edit creates new layer objects or
images. A snapshot (:class:`DocState`) is therefore cheap: lists of small dataclasses that
share their images.
"""

from __future__ import annotations

import itertools
from collections.abc import Callable
from dataclasses import dataclass, field, fields, replace
from pathlib import Path
from typing import ClassVar

from PySide6.QtCore import Qt
from PySide6.QtGui import QImage, QPainter, QTransform

from core.i18n import tr
from core.photo.effects import PhotoAdjust
from core.photo.geometry import flip_point, normalize_angle, rotate90_point
from core.photo.history import History
from core.system import DEFAULT_FONT_FAMILY

_ids = itertools.count(1)

# Undo step names (shown translated in the Undo / Redo tooltips)
HISTORY_LABELS = (
    'Add text',
    'Edit text',
    'Insert image',
    'Blur area',
    'Blur brush',
    'Blur effect',
    'Blur strength',
    'Move layer',
    'Resize layer',
    'Rotate layer',
    'Delete layer',
    'Duplicate layer',
    'Reorder layers',
    'Show / hide layer',
    'Rename layer',
    'Opacity',
    'Flip',
    'Adjust',
    'Rotate',
    'Rotate 90°',
    'Crop',
)


def new_id() -> int:
    return next(_ids)


# Layers ---------------------------------------------------------------------------------------


@dataclass
class Layer:
    kind: ClassVar[str] = 'layer'
    id: int = field(default_factory=new_id)
    name: str = ''
    visible: bool = True
    opacity: float = 1.0  # 0–1

    def clone(self) -> Layer:
        return replace(self)

    def signature(self) -> tuple:
        """Hashable summary of everything that changes the pixels (render caches)."""
        out = []
        for f in fields(self):
            v = getattr(self, f.name)
            if isinstance(v, QImage):
                v = v.cacheKey()
            elif isinstance(v, list):
                v = tuple(v)
            out.append(v)
        return (self.kind, *out)

    # canvas operations (crop / rotate / flip move every layer with the canvas)
    def translate(self, dx: float, dy: float) -> None:
        raise NotImplementedError

    def map_points(
        self, fn: Callable[[float, float], tuple[float, float]], rotate: float, mirror: str
    ) -> None:
        raise NotImplementedError


@dataclass
class TextLayer(Layer):
    kind: ClassVar[str] = 'text'
    text: str = ''
    font_family: str = DEFAULT_FONT_FAMILY
    font_size: float = 64.0  # px of the image
    color: str = '#ffffff'
    bold: bool = False
    italic: bool = False
    outline_width: float = 0.0  # px
    outline_color: str = '#000000'
    x: float = 0.0  # centre of the text block, image px
    y: float = 0.0
    rotation: float = 0.0  # degrees, clockwise

    def translate(self, dx: float, dy: float) -> None:
        self.x += dx
        self.y += dy

    def map_points(self, fn, rotate, mirror) -> None:  # noqa: ANN001
        self.x, self.y = fn(self.x, self.y)
        self.rotation = normalize_angle(self.rotation + rotate)
        if mirror:  # text stays readable; its angle is mirrored
            self.rotation = normalize_angle(-self.rotation)


@dataclass
class ImageLayer(Layer):
    kind: ClassVar[str] = 'image'
    image: QImage = field(default_factory=QImage)
    x: float = 0.0  # centre, image px
    y: float = 0.0
    width: float = 1.0  # drawn size, image px
    height: float = 1.0
    rotation: float = 0.0
    flip_h: bool = False
    flip_v: bool = False

    def translate(self, dx: float, dy: float) -> None:
        self.x += dx
        self.y += dy

    def map_points(self, fn, rotate, mirror) -> None:  # noqa: ANN001
        self.x, self.y = fn(self.x, self.y)
        if rotate:
            self.rotation = normalize_angle(self.rotation + rotate)
        if mirror == 'h':
            self.flip_h = not self.flip_h
            self.rotation = normalize_angle(-self.rotation)
        elif mirror == 'v':
            self.flip_v = not self.flip_v
            self.rotation = normalize_angle(-self.rotation)


@dataclass(frozen=True)
class Stroke:
    radius: float
    points: tuple[tuple[float, float], ...]


@dataclass
class BlurLayer(Layer):
    kind: ClassVar[str] = 'blur'
    shape: str = 'rect'  # rect | ellipse | brush
    mode: str = 'blur'  # blur | pixelate
    strength: float = 40.0  # 1–100
    rect: tuple[float, float, float, float] = (0.0, 0.0, 0.0, 0.0)  # rect / ellipse
    strokes: list[Stroke] = field(default_factory=list)  # brush

    def clone(self) -> BlurLayer:
        return replace(self, strokes=list(self.strokes))

    def translate(self, dx: float, dy: float) -> None:
        x, y, w, h = self.rect
        self.rect = (x + dx, y + dy, w, h)
        self.strokes = [
            Stroke(s.radius, tuple((px + dx, py + dy) for px, py in s.points)) for s in self.strokes
        ]

    def map_points(self, fn, rotate, mirror) -> None:  # noqa: ANN001
        x, y, w, h = self.rect
        if w or h:
            (ax, ay), (bx, by) = fn(x, y), fn(x + w, y + h)
            self.rect = (min(ax, bx), min(ay, by), abs(bx - ax), abs(by - ay))
        self.strokes = [Stroke(s.radius, tuple(fn(px, py) for px, py in s.points)) for s in self.strokes]

    def bounds(self) -> tuple[float, float, float, float]:
        if self.shape != 'brush':
            return self.rect
        xs: list[float] = []
        ys: list[float] = []
        for s in self.strokes:
            for px, py in s.points:
                xs += [px - s.radius, px + s.radius]
                ys += [py - s.radius, py + s.radius]
        if not xs:
            return (0.0, 0.0, 0.0, 0.0)
        return min(xs), min(ys), max(xs) - min(xs), max(ys) - min(ys)


LAYER_TYPES: dict[str, type[Layer]] = {c.kind: c for c in (TextLayer, ImageLayer, BlurLayer)}


@dataclass
class Background:
    image: QImage  # Format_ARGB32_Premultiplied, current canvas pixels before adjustments
    adjust: PhotoAdjust = PhotoAdjust()
    angle: float = 0.0  # free rotation (straighten), degrees; the photo is scaled to cover
    visible: bool = True

    def signature(self) -> tuple:
        return (self.image.cacheKey(), self.adjust, self.angle, self.visible)


@dataclass
class DocState:
    background: Background
    layers: list[Layer]  # bottom → top

    def clone(self) -> DocState:
        return DocState(replace(self.background), [lay.clone() for lay in self.layers])


# Document ----------------------------------------------------------------------------------


class Document:
    """Edit operations; each one records an undo step. Listeners get ``"content"``,
    ``"layers"`` (list changed), ``"selection"`` or ``"history"``."""

    def __init__(self, image: QImage, source: Path | None = None, name: str = '') -> None:
        img = image.convertToFormat(QImage.Format.Format_ARGB32_Premultiplied)
        self.state = DocState(Background(img), [])
        self.history: History[DocState] = History()
        self.source = Path(source) if source else None
        self.name = name or (self.source.name if self.source else '')
        self.project_path: Path | None = None
        self.selected_id: int | None = None
        self.modified = False
        self.revision = 0
        self.listeners: list[Callable[[str], None]] = []

    # basics
    @property
    def width(self) -> int:
        return self.state.background.image.width()

    @property
    def height(self) -> int:
        return self.state.background.image.height()

    @property
    def size(self) -> tuple[int, int]:
        return self.width, self.height

    @property
    def layers(self) -> list[Layer]:
        return self.state.layers

    @property
    def background(self) -> Background:
        return self.state.background

    def layer(self, layer_id: int | None) -> Layer | None:
        return next((lay for lay in self.state.layers if lay.id == layer_id), None)

    def index_of(self, layer_id: int) -> int:
        return next((i for i, lay in enumerate(self.state.layers) if lay.id == layer_id), -1)

    def selected(self) -> Layer | None:
        return self.layer(self.selected_id)

    def emit(self, what: str) -> None:
        for fn in list(self.listeners):
            fn(what)

    # history
    def checkpoint(self, label: str, key: str | None = None) -> None:
        """Call *before* changing the state."""
        self.history.push(label, self.state.clone(), key)
        self.emit('history')

    def changed(self, layers: bool = False) -> None:
        self.revision += 1
        self.modified = True
        self.emit('layers' if layers else 'content')

    def undo(self) -> bool:
        return self._restore(self.history.undo(self.state.clone()))

    def redo(self) -> bool:
        return self._restore(self.history.redo(self.state.clone()))

    def _restore(self, state: DocState | None) -> bool:
        if state is None:
            return False
        self.state = state
        if self.layer(self.selected_id) is None:
            self.selected_id = None
        self.changed(layers=True)
        self.emit('selection')
        self.emit('history')
        return True

    # selection
    def select(self, layer_id: int | None) -> None:
        if layer_id != self.selected_id:
            self.selected_id = layer_id
            self.history.break_merge()
            self.emit('selection')

    # layer operations
    def add_layer(self, layer: Layer, label: str) -> Layer:
        self.checkpoint(label)
        idx = self.index_of(self.selected_id) if self.selected_id is not None else -1
        if idx < 0:
            self.state.layers.append(layer)
        else:
            self.state.layers.insert(idx + 1, layer)
        if not layer.name:
            layer.name = self.unique_name(layer)
        self.changed(layers=True)
        self.select(layer.id)
        return layer

    def unique_name(self, layer: Layer) -> str:
        base = {'text': tr('Text'), 'image': tr('Image'), 'blur': tr('Blur')}.get(layer.kind, tr('Layer'))
        if isinstance(layer, TextLayer) and layer.text.strip():
            return layer.text.strip().splitlines()[0][:32]
        used = {lay.name for lay in self.state.layers}
        n = 1
        while f'{base} {n}' in used:
            n += 1
        return f'{base} {n}'

    def remove_layer(self, layer_id: int) -> None:
        idx = self.index_of(layer_id)
        if idx < 0:
            return
        self.checkpoint('Delete layer')
        del self.state.layers[idx]
        if self.selected_id == layer_id:
            below = self.state.layers[idx - 1].id if idx > 0 and self.state.layers else None
            self.selected_id = below
            self.emit('selection')
        self.changed(layers=True)

    def duplicate_layer(self, layer_id: int) -> Layer | None:
        lay = self.layer(layer_id)
        if lay is None:
            return None
        copy = lay.clone()
        copy.id = new_id()
        copy.name = f'{lay.name} copy'
        copy.translate(20, 20)
        return self.add_layer(copy, 'Duplicate layer')

    def move_layer(self, layer_id: int, delta: int) -> None:
        idx = self.index_of(layer_id)
        new = idx + delta
        if idx < 0 or not 0 <= new < len(self.state.layers):
            return
        self.checkpoint('Reorder layers')
        layers = self.state.layers
        layers.insert(new, layers.pop(idx))
        self.changed(layers=True)

    def set_visible(self, layer_id: int | None, visible: bool) -> None:
        """``layer_id=None`` is the background."""
        if layer_id is None:
            if self.background.visible == visible:
                return
            self.checkpoint('Show / hide layer')
            self.state.background = replace(self.background, visible=visible)
        else:
            lay = self.layer(layer_id)
            if lay is None or lay.visible == visible:
                return
            self.checkpoint('Show / hide layer')
            lay.visible = visible
        self.changed(layers=True)

    def rename_layer(self, layer_id: int, name: str) -> None:
        lay = self.layer(layer_id)
        name = name.strip()
        if lay is None or not name or name == lay.name:
            return
        self.checkpoint('Rename layer')
        lay.name = name
        self.changed(layers=True)

    def edit_layer(self, layer_id: int, label: str, key: str | None = None, **values: object) -> None:
        """Set fields of a layer as one (mergeable) undo step."""
        lay = self.layer(layer_id)
        if lay is None:
            return
        if all(getattr(lay, k) == v for k, v in values.items()):
            return
        self.checkpoint(label, key)
        for k, v in values.items():
            setattr(lay, k, v)
        self.changed(layers='name' in values)

    def set_adjust(self, adjust: PhotoAdjust, key: str | None = 'adjust') -> None:
        if adjust == self.background.adjust:
            return
        self.checkpoint('Adjust', key)
        self.state.background = replace(self.background, adjust=adjust)
        self.changed()

    def set_angle(self, angle: float, key: str | None = 'angle') -> None:
        if angle == self.background.angle:
            return
        self.checkpoint('Rotate', key)
        self.state.background = replace(self.background, angle=angle)
        self.changed()

    # canvas operations
    def crop(self, rect: tuple[int, int, int, int]) -> None:
        x, y, w, h = rect
        if (x, y, w, h) == (0, 0, self.width, self.height) or w < 1 or h < 1:
            return
        self.checkpoint('Crop')
        bg = self.background
        if bg.angle:
            bg = replace(bg, image=self._bake_angle(bg), angle=0.0)
        self.state.background = replace(bg, image=bg.image.copy(x, y, w, h))
        for lay in self.state.layers:
            lay.translate(-x, -y)
        self.changed()

    def _bake_angle(self, bg: Background) -> QImage:
        """The background with its free rotation applied (before a crop)."""
        from ui.photo.render import draw_background  # noqa: PLC0415

        out = QImage(bg.image.size(), QImage.Format.Format_ARGB32_Premultiplied)
        out.fill(Qt.GlobalColor.transparent)
        p = QPainter(out)
        draw_background(p, replace(bg, adjust=PhotoAdjust(), visible=True), bg.image)
        p.end()
        return out

    def rotate90(self, clockwise: bool) -> None:
        self.checkpoint('Rotate 90°')
        size = self.size
        bg = self.background
        t = QTransform().rotate(90 if clockwise else -90)
        self.state.background = replace(bg, image=bg.image.transformed(t))
        for lay in self.state.layers:
            lay.map_points(
                lambda px, py: rotate90_point(px, py, size, clockwise), 90 if clockwise else -90, ''
            )
        self.changed()

    def flip(self, horizontal: bool) -> None:
        self.checkpoint('Flip')
        size = self.size
        bg = self.background
        orient = Qt.Orientation.Horizontal if horizontal else Qt.Orientation.Vertical
        self.state.background = replace(bg, image=bg.image.flipped(orient), angle=-bg.angle)
        for lay in self.state.layers:
            lay.map_points(lambda px, py: flip_point(px, py, size, horizontal), 0, 'h' if horizontal else 'v')
        self.changed()

    def mark_saved(self) -> None:
        self.modified = False
        self.emit('history')
