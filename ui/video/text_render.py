"""Video texts drawn with Qt (same code as the photo editor's text layers).

The preview paints these images over the frame; the export saves them as transparent PNGs
at the output resolution and FFmpeg overlays them — so what you see is what you get,
Vietnamese accents included.
"""

from __future__ import annotations

from collections import OrderedDict
from dataclasses import astuple
from pathlib import Path

from PySide6.QtCore import QRectF, Qt
from PySide6.QtGui import QImage, QPainter

from core.video.export import OverlayImage
from core.video.project import TextOverlay, VideoProject
from ui.photo.document import TextLayer
from ui.photo.render import paint_text, text_shape

_cache: OrderedDict[tuple, QImage] = OrderedDict()


def _layer(t: TextOverlay, scale: float) -> TextLayer:
    return TextLayer(
        text=t.text,
        font_family=t.font_family,
        font_size=max(1.0, t.font_size * scale),
        color=t.color,
        bold=t.bold,
        italic=t.italic,
        outline_width=t.outline_width * scale,
        outline_color=t.outline_color,
        opacity=t.opacity,
    )


def render_overlay(t: TextOverlay, scale: float) -> QImage:
    """The text on a transparent image just big enough for it (null image if empty)."""
    scale = round(scale, 4)
    key = (astuple(t)[1:10], scale)  # text + style: everything but id, position and timing
    hit = _cache.get(key)
    if hit is not None:
        _cache.move_to_end(key)
        return hit
    if not t.text.strip():
        return QImage()
    lay = _layer(t, scale)
    _path, bounds = text_shape(lay)
    r = bounds.adjusted(-2, -2, 2, 2).toAlignedRect()
    img = QImage(max(1, r.width()), max(1, r.height()), QImage.Format.Format_ARGB32_Premultiplied)
    img.fill(Qt.GlobalColor.transparent)
    p = QPainter(img)
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    p.translate(-r.x(), -r.y())
    paint_text(p, lay)
    p.end()
    _cache[key] = img
    while len(_cache) > 48:
        _cache.popitem(last=False)
    return img


def overlay_rect(t: TextOverlay, frame: QRectF, scale: float) -> QRectF:
    """Where the text image goes inside ``frame`` (its centre at ``x``, ``y``)."""
    img = render_overlay(t, scale)
    w, h = img.width(), img.height()
    cx = frame.x() + t.x * frame.width()
    cy = frame.y() + t.y * frame.height()
    return QRectF(cx - w / 2, cy - h / 2, w, h)


def export_overlays(p: VideoProject, out_w: int, out_h: int, folder: Path) -> list[OverlayImage]:
    """PNG files for every visible text at the output size (in ``folder``)."""
    folder.mkdir(parents=True, exist_ok=True)
    scale = out_h / max(1, p.info.height)
    out: list[OverlayImage] = []
    dur = p.duration
    for i, t in enumerate(p.texts):
        start, end = max(0.0, t.start), min(dur, t.end)
        img = render_overlay(t, scale)
        if img.isNull() or end - start <= 0.01:
            continue
        r = overlay_rect(t, QRectF(0, 0, out_w, out_h), scale)
        path = folder / f'text_{i:02d}.png'
        img.save(str(path), 'PNG')  # pyright: ignore[reportCallIssue, reportArgumentType] - PySide6 stub wants bytes
        out.append(OverlayImage(str(path), int(round(r.x())), int(round(r.y())), start, end))
    return out
