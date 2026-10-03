"""Drawing a photo document with QPainter (canvas display and export use the same code).

The painter is set up in *image* coordinates (1 unit = 1 image pixel). ``display_scale`` is
the number of device pixels per image pixel: it picks the background resolution (reduced
preview or full) and the resolution of the blur patches, so the canvas stays fluid at any
zoom while the export (scale 1) is always full quality.
"""

from __future__ import annotations

import logging
import math
from collections import OrderedDict
from collections.abc import Callable
from dataclasses import replace

import numpy as np
from PySide6.QtCore import QObject, QPointF, QRect, QRectF, QSize, Qt, QThreadPool
from PySide6.QtGui import (
    QBrush,
    QColor,
    QFont,
    QFontMetricsF,
    QImage,
    QPainter,
    QPainterPath,
    QPen,
    QPolygonF,
    QTransform,
)

from core.photo.effects import PhotoAdjust, apply_blur_effect, apply_photo_adjust, blur_sigma
from core.photo.geometry import cover_scale
from ui.photo.document import Background, BlurLayer, DocState, ImageLayer, Layer, TextLayer
from ui.workers import Task

log = logging.getLogger(__name__)

PREVIEW_LONG = 1600  # px: long side of the background used while zoomed out / dragging sliders
HANDLE_PAD = 6.0


# QImage <-> NumPy -------------------------------------------------------------------------------


def qimage_from_rgba(arr: np.ndarray) -> QImage:
    """(H, W, 4) uint8 RGBA → premultiplied ARGB32 QImage (deep copy)."""
    arr = np.ascontiguousarray(arr, dtype=np.uint8)
    h, w = arr.shape[:2]
    img = QImage(arr.data, w, h, 4 * w, QImage.Format.Format_RGBA8888)
    return img.convertToFormat(QImage.Format.Format_ARGB32_Premultiplied)


def rgba_from_qimage(img: QImage) -> np.ndarray:
    """Any QImage → (H, W, 4) uint8 RGBA (straight alpha, copy)."""
    img = img.convertToFormat(QImage.Format.Format_RGBA8888)
    w, h = img.width(), img.height()
    ptr = img.constBits()
    arr = np.frombuffer(ptr, np.uint8, count=img.bytesPerLine() * h).reshape(h, img.bytesPerLine())
    return arr[:, : w * 4].reshape(h, w, 4).copy()


# Layer geometry ---------------------------------------------------------------------------------


def text_font(layer: TextLayer) -> QFont:
    f = QFont(layer.font_family)
    f.setPixelSize(max(1, round(layer.font_size)))
    f.setBold(layer.bold)
    f.setItalic(layer.italic)
    f.setHintingPreference(QFont.HintingPreference.PreferNoHinting)
    return f


_text_cache: OrderedDict[tuple, tuple[QPainterPath, QRectF]] = OrderedDict()


def text_shape(layer: TextLayer) -> tuple[QPainterPath, QRectF]:
    """Glyph outlines centred on (0, 0) and the block's bounding box (local coordinates)."""
    key = (layer.text, layer.font_family, layer.font_size, layer.bold, layer.italic, layer.outline_width)
    hit = _text_cache.get(key)
    if hit is not None:
        _text_cache.move_to_end(key)
        return hit
    font = text_font(layer)
    fm = QFontMetricsF(font)
    lines = layer.text.split('\n') if layer.text else ['']
    lh = fm.lineSpacing()
    widths = [fm.horizontalAdvance(line) for line in lines]
    w = max(widths, default=0.0)
    h = lh * len(lines)
    path = QPainterPath()
    for i, line in enumerate(lines):
        if line:
            path.addText(QPointF(-widths[i] / 2, -h / 2 + i * lh + fm.ascent()), font, line)
    pad = layer.outline_width
    bounds = QRectF(-w / 2 - pad, -h / 2 - pad, w + 2 * pad, h + 2 * pad)
    if bounds.width() < 4:
        bounds.adjust(-layer.font_size / 4, 0, layer.font_size / 4, 0)
    _text_cache[key] = (path, bounds)
    while len(_text_cache) > 64:
        _text_cache.popitem(last=False)
    return path, bounds


def layer_transform(layer: Layer) -> QTransform:
    """Local → image coordinates (text and image layers)."""
    t = QTransform()
    if isinstance(layer, (TextLayer, ImageLayer)):
        t.translate(layer.x, layer.y)
        t.rotate(layer.rotation)
    return t


def local_bounds(layer: Layer) -> QRectF:
    if isinstance(layer, TextLayer):
        return text_shape(layer)[1]
    if isinstance(layer, ImageLayer):
        return QRectF(-layer.width / 2, -layer.height / 2, layer.width, layer.height)
    if isinstance(layer, BlurLayer):
        return QRectF(*layer.bounds())
    return QRectF()


def layer_polygon(layer: Layer) -> QPolygonF:
    """Outline of the layer in image coordinates (selection frame)."""
    return layer_transform(layer).map(QPolygonF(local_bounds(layer)))


def hit_test(layer: Layer, x: float, y: float, tolerance: float = 0.0) -> bool:
    if isinstance(layer, BlurLayer):
        if layer.shape == 'brush':
            for s in layer.strokes:
                r2 = (s.radius + tolerance) ** 2
                if any((px - x) ** 2 + (py - y) ** 2 <= r2 for px, py in s.points):
                    return True
            return False
        bx, by, bw, bh = layer.rect
        if layer.shape == 'ellipse' and bw > 0 and bh > 0:
            rx, ry = bw / 2 + tolerance, bh / 2 + tolerance
            return ((x - bx - bw / 2) / rx) ** 2 + ((y - by - bh / 2) / ry) ** 2 <= 1
        return bx - tolerance <= x <= bx + bw + tolerance and by - tolerance <= y <= by + bh + tolerance
    inv, ok = layer_transform(layer).inverted()
    if not ok:
        return False
    p = inv.map(QPointF(x, y))
    return local_bounds(layer).adjusted(-tolerance, -tolerance, tolerance, tolerance).contains(p)


# Background -------------------------------------------------------------------------------------


def draw_background(p: QPainter, bg: Background, img: QImage) -> None:
    """Draw the (already adjusted) background ``img`` over the canvas rect, with the free
    rotation; ``img`` may be a reduced copy: it is stretched to the canvas size."""
    w, h = bg.image.width(), bg.image.height()
    target = QRectF(0, 0, w, h)
    p.save()
    p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, p.transform().m11() < 2.0)
    if bg.angle:
        p.setClipRect(target, Qt.ClipOperation.IntersectClip)
        k = cover_scale(w, h, bg.angle)
        p.translate(w / 2, h / 2)
        p.rotate(bg.angle)
        p.scale(k, k)
        p.translate(-w / 2, -h / 2)
        p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)
        p.drawImage(target, img)
    else:
        # only the visible part: fast at high zoom on large photos
        visible = target
        inv_t, ok = p.transform().inverted()
        dev = p.device()
        if ok and dev is not None:
            visible = inv_t.mapRect(QRectF(0, 0, dev.width(), dev.height()))
        if p.hasClipping():
            visible = visible.intersected(p.clipBoundingRect())
        src_rect = visible.intersected(target)
        if src_rect.isEmpty():
            p.restore()
            return
        sx, sy = img.width() / w, img.height() / h
        src_px = QRectF(src_rect.x() * sx, src_rect.y() * sy, src_rect.width() * sx, src_rect.height() * sy)
        # align to whole source pixels so nothing shifts while panning
        aligned = src_px.toAlignedRect().intersected(img.rect())
        dst = QRectF(aligned.x() / sx, aligned.y() / sy, aligned.width() / sx, aligned.height() / sy)
        p.drawImage(dst, img, QRectF(aligned))
    p.restore()


class BaseCache(QObject):
    """Adjusted background images: a reduced preview (computed at once) and the full
    resolution (computed in the background; ``on_ready`` is called when it arrives)."""

    def __init__(self, on_ready: Callable[[], None] | None = None, parent: QObject | None = None) -> None:
        super().__init__(parent)
        self.on_ready = on_ready
        self.pool = QThreadPool(self)
        self.pool.setMaxThreadCount(1)
        self._small: OrderedDict[int, QImage] = OrderedDict()
        self._preview: OrderedDict[tuple, QImage] = OrderedDict()
        self._full: OrderedDict[tuple, QImage] = OrderedDict()
        self._pending: set[tuple] = set()

    @staticmethod
    def _key(bg: Background) -> tuple:
        return (bg.image.cacheKey(), bg.adjust)

    @staticmethod
    def _put(cache: OrderedDict, key: object, img: QImage, limit: int) -> None:
        cache[key] = img
        cache.move_to_end(key)
        while len(cache) > limit:
            cache.popitem(last=False)

    def small_source(self, img: QImage) -> QImage:
        key = img.cacheKey()
        hit = self._small.get(key)
        if hit is None:
            hit = img.scaled(
                QSize(PREVIEW_LONG, PREVIEW_LONG),
                Qt.AspectRatioMode.KeepAspectRatio,
                Qt.TransformationMode.SmoothTransformation,
            )
            self._put(self._small, key, hit, 3)
        return hit

    def is_large(self, bg: Background) -> bool:
        return max(bg.image.width(), bg.image.height()) > PREVIEW_LONG

    def preview(self, bg: Background) -> QImage:
        if not self.is_large(bg):
            return self.full_sync(bg)
        key = self._key(bg)
        hit = self._preview.get(key)
        if hit is None:
            small = self.small_source(bg.image)
            hit = small if bg.adjust.is_default() else adjust_qimage(small, bg.adjust, _long(bg.image))
            self._put(self._preview, key, hit, 4)
        return hit

    def full(self, bg: Background) -> QImage | None:
        """The full-resolution image, or ``None`` while it is computed in the background."""
        if bg.adjust.is_default():
            return bg.image
        key = self._key(bg)
        hit = self._full.get(key)
        if hit is not None or not self.is_large(bg):
            return hit if hit is not None else self.full_sync(bg)
        if key not in self._pending:
            self._pending.add(key)
            src, adjust = bg.image, bg.adjust
            Task.submit(
                self.pool,
                lambda: adjust_qimage(src, adjust, _long(src)),
                lambda img, k=key: self._full_ready(k, img),
                lambda err, k=key: self._pending.discard(k),
            )
        return None

    def _full_ready(self, key: tuple, img: QImage) -> None:
        self._pending.discard(key)
        self._put(self._full, key, img, 2)
        if self.on_ready is not None:
            self.on_ready()

    def full_sync(self, bg: Background) -> QImage:
        if bg.adjust.is_default():
            return bg.image
        key = self._key(bg)
        hit = self._full.get(key)
        if hit is None:
            hit = adjust_qimage(bg.image, bg.adjust, _long(bg.image))
            self._put(self._full, key, hit, 2)
        return hit

    def best(self, bg: Background, display_scale: float, full: bool) -> QImage:
        if full:
            return self.full_sync(bg)
        if self.is_large(bg) and display_scale * _long(bg.image) <= PREVIEW_LONG * 1.05:
            return self.preview(bg)
        return self.full(bg) or self.preview(bg)


def _long(img: QImage) -> int:
    return max(img.width(), img.height())


def adjust_qimage(img: QImage, adjust: PhotoAdjust, long_side: int) -> QImage:
    if adjust.is_default():
        return img
    return qimage_from_rgba(apply_photo_adjust(rgba_from_qimage(img), adjust, long_side=long_side))


def paint_text(p: QPainter, lay: TextLayer) -> None:
    """Draw a text layer (outline under the fill) in image coordinates."""
    if not lay.text.strip():
        return
    path, bounds = text_shape(lay)
    local = layer_transform(lay)
    if lay.opacity >= 0.999 or lay.outline_width <= 0:
        p.save()
        p.setTransform(local, True)
        p.setOpacity(p.opacity() * lay.opacity)
        _text_strokes(p, lay, path)
        p.restore()
        return
    # semi-transparent with an outline: draw opaque into a device-space buffer first,
    # otherwise the fill shows the inner half of the outline through it
    full_t = local * p.combinedTransform()
    dev = p.device()
    dev_rect = QRect(0, 0, dev.width(), dev.height()) if dev is not None else QRect()
    rect = full_t.mapRect(bounds).toAlignedRect().adjusted(-2, -2, 2, 2).intersected(dev_rect)
    if rect.isEmpty():
        return
    buf = QImage(rect.size(), QImage.Format.Format_ARGB32_Premultiplied)
    buf.fill(Qt.GlobalColor.transparent)
    bp = QPainter(buf)
    bp.setRenderHint(QPainter.RenderHint.Antialiasing)
    bp.setTransform(full_t * QTransform.fromTranslate(-rect.x(), -rect.y()))
    _text_strokes(bp, lay, path)
    bp.end()
    p.save()
    p.resetTransform()
    p.setOpacity(p.opacity() * lay.opacity)
    p.drawImage(rect.topLeft(), buf)
    p.restore()


def _text_strokes(p: QPainter, lay: TextLayer, path: QPainterPath) -> None:
    if lay.outline_width > 0:
        pen = QPen(QColor(lay.outline_color), lay.outline_width * 2)
        pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
        pen.setCapStyle(Qt.PenCapStyle.RoundCap)
        p.strokePath(path, pen)
    p.fillPath(path, QColor(lay.color))


# Renderer ---------------------------------------------------------------------------------------


class Renderer:
    def __init__(self, base: BaseCache) -> None:
        self.base = base
        self._blur_cache: OrderedDict[tuple, tuple[QImage, QRectF]] = OrderedDict()

    def paint(
        self,
        p: QPainter,
        state: DocState,
        display_scale: float = 1.0,
        full: bool = False,
        upto: int | None = None,
        hidden: set[int] | None = None,
    ) -> None:
        """Draw the background and the layers below index ``upto`` (all by default)."""
        p.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        bg = state.background
        if bg.visible:
            draw_background(p, bg, self.base.best(bg, display_scale, full))
        layers = state.layers if upto is None else state.layers[:upto]
        for i, lay in enumerate(layers):
            if not lay.visible or (hidden and lay.id in hidden):
                continue
            if isinstance(lay, TextLayer):
                self.draw_text(p, lay)
            elif isinstance(lay, ImageLayer):
                self.draw_image(p, lay)
            elif isinstance(lay, BlurLayer):
                self.draw_blur(p, state, i, lay, display_scale, full)

    def render_image(self, state: DocState) -> QImage:
        """Full-resolution composite (export, project thumbnails)."""
        w, h = state.background.image.width(), state.background.image.height()
        out = QImage(w, h, QImage.Format.Format_ARGB32_Premultiplied)
        out.fill(Qt.GlobalColor.transparent)
        p = QPainter(out)
        self.paint(p, state, 1.0, full=True)
        p.end()
        return out

    # text
    @staticmethod
    def draw_text(p: QPainter, lay: TextLayer) -> None:
        paint_text(p, lay)

    # image
    @staticmethod
    def draw_image(p: QPainter, lay: ImageLayer) -> None:
        if lay.image.isNull() or lay.width < 0.5 or lay.height < 0.5:
            return
        p.save()
        p.setTransform(layer_transform(lay), True)
        p.scale(-1 if lay.flip_h else 1, -1 if lay.flip_v else 1)
        p.setOpacity(p.opacity() * lay.opacity)
        p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)
        p.drawImage(QRectF(-lay.width / 2, -lay.height / 2, lay.width, lay.height), lay.image)
        p.restore()

    # blur
    def draw_blur(
        self, p: QPainter, state: DocState, index: int, lay: BlurLayer, display_scale: float, full: bool
    ) -> None:
        w, h = state.background.image.width(), state.background.image.height()
        canvas = QRectF(0, 0, w, h)
        region = QRectF(*lay.bounds()).intersected(canvas)
        if region.width() < 1 or region.height() < 1:
            return
        long_side = max(w, h)
        q = 1.0 if full else _quality_bucket(display_scale)
        margin = blur_sigma(lay.strength, long_side) * 3 if lay.mode == 'blur' else 0.0
        sample = region.adjusted(-margin, -margin, margin, margin).intersected(canvas).toAlignedRect()
        base_img = self.base.best(state.background, q, full)
        below = (
            state.background.signature(),
            base_img.cacheKey(),
            tuple(lay2.signature() for lay2 in state.layers[:index] if lay2.visible),
        )
        key = (lay.shape, lay.mode, lay.strength, lay.rect, tuple(lay.strokes), below, q, (w, h))
        hit = self._blur_cache.get(key)
        if hit is None:
            hit = (
                self._make_blur_patch(state, index, lay, QRectF(sample), q, long_side, full),
                QRectF(sample),
            )
            self._blur_cache[key] = hit
            while len(self._blur_cache) > 24:
                self._blur_cache.popitem(last=False)
        else:
            self._blur_cache.move_to_end(key)
        patch, rect = hit
        p.save()
        p.setOpacity(p.opacity() * lay.opacity)
        p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)
        p.drawImage(rect, patch)
        p.restore()

    def _make_blur_patch(
        self,
        state: DocState,
        index: int,
        lay: BlurLayer,
        sample: QRectF,
        q: float,
        long_side: int,
        full: bool,
    ) -> QImage:
        pw, ph = max(1, math.ceil(sample.width() * q)), max(1, math.ceil(sample.height() * q))
        below = QImage(pw, ph, QImage.Format.Format_ARGB32_Premultiplied)
        below.fill(Qt.GlobalColor.transparent)
        bp = QPainter(below)
        bp.scale(q, q)
        bp.translate(-sample.x(), -sample.y())
        bp.setClipRect(sample)
        self.paint(bp, state, q, full=full, upto=index)
        bp.end()
        effect = apply_blur_effect(
            rgba_from_qimage(below),
            lay.mode,
            lay.strength,
            max(1, round(long_side * q)),
            origin=(round(sample.x() * q), round(sample.y() * q)),
        )
        out = qimage_from_rgba(effect)
        mask = QImage(out.size(), QImage.Format.Format_ARGB32_Premultiplied)
        mask.fill(Qt.GlobalColor.transparent)
        mp = QPainter(mask)
        mp.setRenderHint(QPainter.RenderHint.Antialiasing)
        mp.scale(q, q)
        mp.translate(-sample.x(), -sample.y())
        paint_blur_shape(mp, lay, QColor('#ffffff'))
        mp.end()
        op = QPainter(out)
        op.setCompositionMode(QPainter.CompositionMode.CompositionMode_DestinationIn)
        op.drawImage(0, 0, mask)
        op.end()
        return out


def paint_blur_shape(p: QPainter, lay: BlurLayer, color: QColor) -> None:
    """Fill the area of a blur layer (mask, or the overlay while drawing)."""
    p.save()
    p.setBrush(QBrush(color))
    if lay.shape == 'brush':
        for s in lay.strokes:
            pen = QPen(color, s.radius * 2)
            pen.setCapStyle(Qt.PenCapStyle.RoundCap)
            pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
            if len(s.points) == 1:
                p.setPen(Qt.PenStyle.NoPen)
                p.drawEllipse(QPointF(*s.points[0]), s.radius, s.radius)
            else:
                path = QPainterPath(QPointF(*s.points[0]))
                for pt in s.points[1:]:
                    path.lineTo(QPointF(*pt))
                p.strokePath(path, pen)
    else:
        p.setPen(Qt.PenStyle.NoPen)
        r = QRectF(*lay.rect)
        if lay.shape == 'ellipse':
            p.drawEllipse(r)
        else:
            p.drawRect(r)
    p.restore()


def _quality_bucket(display_scale: float) -> float:
    """Blur patch resolution: a few fixed steps so zooming does not recompute constantly."""
    for q in (0.125, 0.25, 0.5):
        if display_scale <= q:
            return q
    return 1.0


def state_for_export(state: DocState) -> DocState:
    """A detached copy safe to render on a worker thread."""
    copy = state.clone()
    copy.background = replace(copy.background)
    return copy
