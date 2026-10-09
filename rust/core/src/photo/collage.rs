//! Mẫu cắt ghép (collage): hình học ô (theo phần trăm canvas) → hình chữ nhật pixel.
//! Port của `core/photo/collage.py`.

/// Một ô của mẫu, theo phần trăm canvas: `(x, y, w, h)`.
type Frac = (f64, f64, f64, f64);

/// Các mẫu layout. Khoảng cách giữa ô được trừ ra sau (xem [`cell_rects`]).
pub const TEMPLATES: [(&str, &[Frac]); 4] = [
    // 2 ảnh cạnh nhau
    ("two_horizontal", &[(0.0, 0.0, 0.5, 1.0), (0.5, 0.0, 0.5, 1.0)]),
    // 2 ảnh trên dưới
    ("two_vertical", &[(0.0, 0.0, 1.0, 0.5), (0.0, 0.5, 1.0, 0.5)]),
    (
        "grid_2x2",
        &[
            (0.0, 0.0, 0.5, 0.5),
            (0.5, 0.0, 0.5, 0.5),
            (0.0, 0.5, 0.5, 0.5),
            (0.5, 0.5, 0.5, 0.5),
        ],
    ),
    (
        "one_big_two_small",
        &[
            (0.0, 0.0, 2.0 / 3.0, 1.0),
            (2.0 / 3.0, 0.0, 1.0 / 3.0, 0.5),
            (2.0 / 3.0, 0.5, 1.0 / 3.0, 0.5),
        ],
    ),
];

/// Nhãn tiếng Anh của mẫu (UI dịch qua `i18n`).
pub const TEMPLATE_LABELS: [(&str, &str); 4] = [
    ("two_horizontal", "2 photos side by side"),
    ("two_vertical", "2 photos stacked"),
    ("grid_2x2", "Grid 2 × 2"),
    ("one_big_two_small", "1 large + 2 small"),
];

/// Cỡ canvas sẵn có: nhãn tỉ lệ → `(rộng, cao)` px.
pub const SIZE_PRESETS: [(&str, (u32, u32)); 5] = [
    ("1:1", (2000, 2000)),
    ("4:3", (2400, 1800)),
    ("3:4", (1800, 2400)),
    ("16:9", (2560, 1440)),
    ("9:16", (1440, 2560)),
];

pub fn template(name: &str) -> Option<&'static [Frac]> {
    TEMPLATES.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

pub fn template_label(name: &str) -> &'static str {
    TEMPLATE_LABELS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, l)| *l)
        .unwrap_or("")
}

pub fn size_preset(name: &str) -> Option<(u32, u32)> {
    SIZE_PRESETS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Một ô collage theo pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Hình chữ nhật pixel của các ô; `spacing` px giữa các ô và quanh viền.
///
/// Trả về vector rỗng nếu `template` không tồn tại (bản Python raise `KeyError`; ở đây
/// người gọi chỉ truyền khoá từ [`TEMPLATES`] nên rỗng là đủ rõ và không panic).
pub fn cell_rects(template_name: &str, width: u32, height: u32, spacing: f64) -> Vec<Cell> {
    let Some(cells) = template(template_name) else {
        return vec![];
    };
    let s = spacing.max(0.0);
    let (width, height) = (width as f64, height as f64);
    let eps = 1e-6;
    cells
        .iter()
        .map(|&(fx, fy, fw, fh)| {
            let (x0, y0) = (fx * width, fy * height);
            let (x1, y1) = ((fx + fw) * width, (fy + fh) * height);
            // viền ngoài: cả `spacing`; đường ghép bên trong: nửa mỗi bên
            let left = if fx < eps { s } else { s / 2.0 };
            let top = if fy < eps { s } else { s / 2.0 };
            let right = if fx + fw > 1.0 - eps { s } else { s / 2.0 };
            let bottom = if fy + fh > 1.0 - eps { s } else { s / 2.0 };
            Cell {
                x: x0 + left,
                y: y0 + top,
                w: (x1 - x0 - left - right).max(1.0),
                h: (y1 - y0 - top - bottom).max(1.0),
            }
        })
        .collect()
}

/// Nơi vẽ ảnh để nó phủ kín `cell` (giữa + `offset` px trong ô, `zoom` ≥ 1).
///
/// `offset` bị kẹp để ảnh luôn phủ kín ô. Trả về `(x, y, w, h)`.
pub fn cover_rect(
    img_w: u32,
    img_h: u32,
    cell: &Cell,
    zoom: f64,
    offset: (f64, f64),
) -> (f64, f64, f64, f64) {
    let (iw, ih) = (img_w.max(1) as f64, img_h.max(1) as f64);
    let k = (cell.w / iw).max(cell.h / ih) * zoom.max(1.0);
    let (w, h) = (iw * k, ih * k);
    let (max_dx, max_dy) = ((w - cell.w) / 2.0, (h - cell.h) / 2.0);
    let dx = offset.0.clamp(-max_dx, max_dx);
    let dy = offset.1.clamp(-max_dy, max_dy);
    (
        cell.x + (cell.w - w) / 2.0 + dx,
        cell.y + (cell.h - h) / 2.0 + dy,
        w,
        h,
    )
}

/// `offset` đã kẹp (dùng khi người dùng kéo ảnh trong ô).
pub fn clamp_offset(img_w: u32, img_h: u32, cell: &Cell, zoom: f64, offset: (f64, f64)) -> (f64, f64) {
    let (x, y, w, h) = cover_rect(img_w, img_h, cell, zoom, offset);
    (
        x - (cell.x + (cell.w - w) / 2.0),
        y - (cell.y + (cell.h - h) / 2.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collage_cells_spacing() {
        // Giống test_collage_cells_spacing: trong viền, không chồng nhau, cách nhau ≥ spacing.
        for (name, fracs) in TEMPLATES {
            let cells = cell_rects(name, 1000, 800, 20.0);
            assert_eq!(cells.len(), fracs.len(), "{name}");
            for c in &cells {
                assert!(c.x >= 20.0 - 1e-6 && c.y >= 20.0 - 1e-6, "{name}");
                assert!(c.x + c.w <= 980.0 + 1e-6 && c.y + c.h <= 780.0 + 1e-6, "{name}");
            }
            for (i, a) in cells.iter().enumerate() {
                for b in &cells[i + 1..] {
                    let gap_x = (b.x - (a.x + a.w)).max(a.x - (b.x + b.w));
                    let gap_y = (b.y - (a.y + a.h)).max(a.y - (b.y + b.h));
                    assert!(gap_x.max(gap_y) >= 20.0 - 1e-6, "{name}: hai ô quá gần");
                }
            }
        }
    }

    #[test]
    fn collage_cover_and_clamp() {
        // Giống test_collage_cover_and_clamp.
        let cell = cell_rects("two_horizontal", 1000, 500, 0.0)[0];
        let (x, y, w, h) = cover_rect(400, 200, &cell, 1.0, (0.0, 0.0));
        assert!(w >= cell.w - 1e-6 && h >= cell.h - 1e-6);
        assert!(x <= cell.x + 1e-6 && y <= cell.y + 1e-6);
        assert!(x + w >= cell.x + cell.w - 1e-6);
        let (dx, dy) = clamp_offset(400, 200, &cell, 1.0, (10_000.0, 10_000.0));
        let (x2, y2, _, _) = cover_rect(400, 200, &cell, 1.0, (dx, dy));
        assert!(x2 <= cell.x + 1e-6 && y2 <= cell.y + 1e-6);
    }

    #[test]
    fn labels_and_presets_cover_every_template() {
        for (name, _) in TEMPLATES {
            assert!(!template_label(name).is_empty(), "{name} thiếu nhãn");
        }
        assert_eq!(size_preset("16:9"), Some((2560, 1440)));
        assert_eq!(size_preset("5:4"), None);
        assert!(cell_rects("không_có", 100, 100, 0.0).is_empty());
    }
}
