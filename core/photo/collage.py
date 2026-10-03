"""Collage templates: cell geometry (fractions of the canvas) → pixel rectangles."""

from __future__ import annotations

from dataclasses import dataclass

# Each cell is (x, y, w, h) as fractions of the canvas; spacing is removed afterwards.
TEMPLATES: dict[str, tuple[tuple[float, float, float, float], ...]] = {
    'two_horizontal': ((0, 0, 0.5, 1), (0.5, 0, 0.5, 1)),  # 2 photos side by side
    'two_vertical': ((0, 0, 1, 0.5), (0, 0.5, 1, 0.5)),  # 2 photos stacked
    'grid_2x2': ((0, 0, 0.5, 0.5), (0.5, 0, 0.5, 0.5), (0, 0.5, 0.5, 0.5), (0.5, 0.5, 0.5, 0.5)),
    'one_big_two_small': ((0, 0, 2 / 3, 1), (2 / 3, 0, 1 / 3, 0.5), (2 / 3, 0.5, 1 / 3, 0.5)),
}
TEMPLATE_LABELS: dict[str, str] = {
    'two_horizontal': '2 photos side by side',
    'two_vertical': '2 photos stacked',
    'grid_2x2': 'Grid 2 × 2',
    'one_big_two_small': '1 large + 2 small',
}
SIZE_PRESETS: dict[str, tuple[int, int]] = {
    '1:1': (2000, 2000),
    '4:3': (2400, 1800),
    '3:4': (1800, 2400),
    '16:9': (2560, 1440),
    '9:16': (1440, 2560),
}


@dataclass(frozen=True)
class Cell:
    x: float
    y: float
    w: float
    h: float


def cell_rects(template: str, width: int, height: int, spacing: float) -> list[Cell]:
    """Pixel rectangles of the cells; ``spacing`` px between cells and around the border."""
    cells = TEMPLATES[template]
    s = max(0.0, spacing)
    out: list[Cell] = []
    eps = 1e-6
    for fx, fy, fw, fh in cells:
        x0, y0 = fx * width, fy * height
        x1, y1 = (fx + fw) * width, (fy + fh) * height
        # full spacing on the outer border, half on each side of an inner seam
        left = s if fx < eps else s / 2
        top = s if fy < eps else s / 2
        right = s if fx + fw > 1 - eps else s / 2
        bottom = s if fy + fh > 1 - eps else s / 2
        w = max(1.0, x1 - x0 - left - right)
        h = max(1.0, y1 - y0 - top - bottom)
        out.append(Cell(x0 + left, y0 + top, w, h))
    return out


def cover_rect(
    img_w: int, img_h: int, cell: Cell, zoom: float = 1.0, offset: tuple[float, float] = (0.0, 0.0)
) -> tuple[float, float, float, float]:
    """Where to draw an image so it covers ``cell`` (centre + ``offset`` in cell px, ``zoom`` ≥ 1).

    The offset is clamped so the image always covers the cell. Returns ``(x, y, w, h)``.
    """
    k = max(cell.w / img_w, cell.h / img_h) * max(1.0, zoom)
    w, h = img_w * k, img_h * k
    max_dx, max_dy = (w - cell.w) / 2, (h - cell.h) / 2
    dx = min(max(offset[0], -max_dx), max_dx)
    dy = min(max(offset[1], -max_dy), max_dy)
    return cell.x + (cell.w - w) / 2 + dx, cell.y + (cell.h - h) / 2 + dy, w, h


def clamp_offset(
    img_w: int, img_h: int, cell: Cell, zoom: float, offset: tuple[float, float]
) -> tuple[float, float]:
    x, y, w, h = cover_rect(img_w, img_h, cell, zoom, offset)
    return x - (cell.x + (cell.w - w) / 2), y - (cell.y + (cell.h - h) / 2)
