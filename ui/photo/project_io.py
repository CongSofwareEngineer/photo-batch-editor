"""Photo editor project files (``.pbep``): a ZIP with ``project.json`` + PNG images.

Every layer is kept (text stays editable, images keep their full resolution), so a project
can be reopened and edited later. The original photo is stored inside the file: the
project does not depend on the source file still being there.
"""

from __future__ import annotations

import json
import zipfile
from dataclasses import fields
from pathlib import Path

from PySide6.QtCore import QBuffer, QByteArray, QIODevice
from PySide6.QtGui import QImage

from core.photo.effects import PhotoAdjust
from ui.photo.document import LAYER_TYPES, BlurLayer, Document, ImageLayer, Layer, Stroke, new_id

PROJECT_EXT = '.pbep'
FORMAT_VERSION = 1


class ProjectError(Exception):
    pass


def _png_bytes(img: QImage) -> bytes:
    data = QByteArray()
    buf = QBuffer(data)
    buf.open(QIODevice.OpenModeFlag.WriteOnly)
    img.save(buf, 'PNG')  # pyright: ignore[reportCallIssue, reportArgumentType] - PySide6 stub wants bytes
    buf.close()
    return bytes(data.data())


def _layer_to_dict(lay: Layer, images: dict[str, QImage]) -> dict:
    out: dict = {'kind': lay.kind}
    for f in fields(lay):
        if f.name == 'id':
            continue
        v = getattr(lay, f.name)
        if isinstance(v, QImage):
            name = f'images/{lay.id}.png'
            images[name] = v
            v = name
        elif isinstance(lay, BlurLayer) and f.name == 'strokes':
            v = [{'radius': s.radius, 'points': [list(p) for p in s.points]} for s in v]
        elif isinstance(v, tuple):
            v = list(v)
        out[f.name] = v
    return out


def save_project(doc: Document, path: Path) -> None:
    path = Path(path)
    images: dict[str, QImage] = {'background.png': doc.background.image}
    data = {
        'format': FORMAT_VERSION,
        'app': 'PhotoBatchEditor',
        'name': doc.name,
        'source': str(doc.source) if doc.source else '',
        'background': {
            'image': 'background.png',
            'adjust': doc.background.adjust.to_dict(),
            'angle': doc.background.angle,
            'visible': doc.background.visible,
        },
        'layers': [_layer_to_dict(lay, images) for lay in doc.layers],
    }
    tmp = path.with_name(path.name + '.tmp')
    try:
        with zipfile.ZipFile(tmp, 'w') as z:
            z.writestr(
                'project.json',
                json.dumps(data, indent=2, ensure_ascii=False),
                compress_type=zipfile.ZIP_DEFLATED,
            )
            for name, img in images.items():
                z.writestr(name, _png_bytes(img), compress_type=zipfile.ZIP_STORED)
        tmp.replace(path)
    finally:
        if tmp.exists():
            tmp.unlink()
    doc.project_path = path
    doc.mark_saved()


def _layer_from_dict(d: dict, z: zipfile.ZipFile) -> Layer | None:
    cls = LAYER_TYPES.get(d.get('kind', ''))
    if cls is None:
        return None
    kwargs: dict = {}
    for f in fields(cls):
        if f.name == 'id' or f.name not in d:
            continue
        v = d[f.name]
        if cls is ImageLayer and f.name == 'image':
            img = QImage()
            img.loadFromData(z.read(v), 'PNG')  # pyright: ignore[reportArgumentType]
            v = img.convertToFormat(QImage.Format.Format_ARGB32_Premultiplied)
        elif cls is BlurLayer and f.name == 'strokes':
            v = [Stroke(float(s['radius']), tuple((float(x), float(y)) for x, y in s['points'])) for s in v]
        elif cls is BlurLayer and f.name == 'rect':
            v = tuple(float(x) for x in v)
        kwargs[f.name] = v
    return cls(id=new_id(), **kwargs)


def load_project(path: Path) -> Document:
    path = Path(path)
    try:
        with zipfile.ZipFile(path) as z:
            data = json.loads(z.read('project.json').decode('utf-8'))
            if not isinstance(data, dict) or int(data.get('format', 0)) > FORMAT_VERSION:
                raise ProjectError('Unsupported project file version')
            bg = data.get('background', {})
            img = QImage()
            img.loadFromData(z.read(bg.get('image', 'background.png')), 'PNG')  # pyright: ignore[reportArgumentType]
            if img.isNull():
                raise ProjectError('The project has no background image')
            source = data.get('source') or None
            doc = Document(img, Path(source) if source else None, data.get('name', '') or path.stem)
            doc.state.background.adjust = PhotoAdjust.from_dict(bg.get('adjust'))
            doc.state.background.angle = float(bg.get('angle', 0.0))
            doc.state.background.visible = bool(bg.get('visible', True))
            for d in data.get('layers', []):
                lay = _layer_from_dict(d, z)
                if lay is not None:
                    doc.state.layers.append(lay)
    except (OSError, KeyError, ValueError, zipfile.BadZipFile) as exc:
        raise ProjectError(f'Cannot open project: {exc}') from exc
    doc.project_path = path
    doc.modified = False
    return doc
