//! Mẫu cắt ghép (collage): hình học ô (theo phần trăm canvas) → hình chữ nhật pixel.
//! Port của `core/photo/collage.py`, thêm [`render_collage`] để GUI và test dùng chung.

use crate::photo::RgbaImage;

/// Một ô của mẫu, theo phần trăm canvas: `(x, y, w, h)`.
type Frac = (f64, f64, f64, f64);

/// Các mẫu layout. Khoảng cách giữa ô được trừ ra sau (xem [`cell_rects`]).
pub const TEMPLATES: [(&str, &[Frac]); 8] = [
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
    (
        "three_horizontal",
        &[
            (0.0, 0.0, 1.0 / 3.0, 1.0),
            (1.0 / 3.0, 0.0, 1.0 / 3.0, 1.0),
            (2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0),
        ],
    ),
    (
        "three_vertical",
        &[
            (0.0, 0.0, 1.0, 1.0 / 3.0),
            (0.0, 1.0 / 3.0, 1.0, 1.0 / 3.0),
            (0.0, 2.0 / 3.0, 1.0, 1.0 / 3.0),
        ],
    ),
    (
        "one_top_two_bottom",
        &[
            (0.0, 0.0, 1.0, 0.5),
            (0.0, 0.5, 0.5, 0.5),
            (0.5, 0.5, 0.5, 0.5),
        ],
    ),
    (
        "grid_3x3",
        &[
            (0.0, 0.0, 1.0 / 3.0, 1.0 / 3.0),
            (1.0 / 3.0, 0.0, 1.0 / 3.0, 1.0 / 3.0),
            (2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0 / 3.0),
            (0.0, 1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0),
            (1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0),
            (2.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0),
            (0.0, 2.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0),
            (1.0 / 3.0, 2.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0),
            (2.0 / 3.0, 2.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0),
        ],
    ),
];

/// Nhãn tiếng Anh của mẫu (UI dịch qua `i18n`).
pub const TEMPLATE_LABELS: [(&str, &str); 8] = [
    ("two_horizontal", "2 photos side by side"),
    ("two_vertical", "2 photos stacked"),
    ("grid_2x2", "Grid 2 × 2"),
    ("one_big_two_small", "1 large + 2 small"),
    ("three_horizontal", "3 photos side by side"),
    ("three_vertical", "3 photos stacked"),
    ("one_top_two_bottom", "1 on top + 2 below"),
    ("grid_3x3", "Grid 3 × 3"),
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

/// Ảnh của một ô khi ghép: ảnh nguồn + zoom (≥ 1) + lệch (px canvas thật).
pub struct CellSource<'a> {
    pub img: &'a RgbaImage,
    pub zoom: f64,
    pub offset: (f64, f64),
}

/// Thông số chung của ảnh ghép (đơn vị px của canvas thật).
#[derive(Debug, Clone, PartialEq)]
pub struct CollageStyle {
    pub template: String,
    pub width: u32,
    pub height: u32,
    pub spacing: f64,
    pub radius: f64,
    pub background: [u8; 4],
}

/// Ghép ảnh theo mẫu ở tỉ lệ `scale` (1.0 = cỡ thật, < 1 = xem trước).
///
/// Ô `None` để lộ nền; nếu `empty_fill` có giá trị thì ô trống được tô màu đó (xem trước).
/// Góc bo được khử răng cưa theo độ phủ của pixel. Ảnh nguồn lớn hơn ô nhiều lần được thu
/// nhỏ bằng trung bình khối trước khi lấy mẫu song tuyến tính → không bị răng cưa / moiré.
/// Hình học dùng [`cover_rect`] nên kết quả không phụ thuộc độ phân giải ảnh nguồn (chỉ tỉ lệ),
/// vì vậy GUI có thể xem trước bằng ảnh thu nhỏ và xuất bằng ảnh gốc.
pub fn render_collage(
    style: &CollageStyle,
    cells: &[Option<CellSource>],
    scale: f64,
    empty_fill: Option<[u8; 4]>,
) -> RgbaImage {
    let ow = ((style.width as f64 * scale).round() as usize).max(1);
    let oh = ((style.height as f64 * scale).round() as usize).max(1);
    // tỉ lệ thực sau làm tròn
    let k = ow as f64 / style.width.max(1) as f64;
    let mut out = RgbaImage::filled(oh, ow, style.background);
    let rects = cell_rects(&style.template, style.width, style.height, style.spacing);
    for (i, cell) in rects.iter().enumerate() {
        let src = cells.get(i).and_then(|c| c.as_ref());
        if src.is_none() && empty_fill.is_none() {
            continue;
        }
        let (cx, cy, cw, ch) = (cell.x * k, cell.y * k, cell.w * k, cell.h * k);
        let radius = (style.radius * k).min(cw / 2.0).min(ch / 2.0).max(0.0);
        // ảnh đã đặt vào ô (px output) + ảnh nguồn đã thu nhỏ hợp lý
        let placed = src.map(|s| {
            let (x, y, w, h) = cover_rect(s.img.w as u32, s.img.h as u32, cell, s.zoom, s.offset);
            let (x, y, w, h) = (x * k, y * k, w * k, h * k);
            let factor = (s.img.w as f64 / w.max(1.0)).floor() as usize;
            let img = if factor >= 2 {
                box_downscale(s.img, factor)
            } else {
                s.img.clone()
            };
            (img, x, y, w, h)
        });
        let x0 = cx.floor().max(0.0) as usize;
        let y0 = cy.floor().max(0.0) as usize;
        let x1 = ((cx + cw).ceil() as usize).min(ow);
        let y1 = ((cy + ch).ceil() as usize).min(oh);
        for py in y0..y1 {
            for px in x0..x1 {
                let (fx, fy) = (px as f64 + 0.5, py as f64 + 0.5);
                let cover = coverage(fx, fy, cx, cy, cw, ch, radius);
                if cover <= 0.0 {
                    continue;
                }
                let color = match &placed {
                    Some((img, x, y, w, h)) => {
                        let u = (fx - x) / w * img.w as f64;
                        let v = (fy - y) / h * img.h as f64;
                        sample_bilinear(img, u, v)
                    }
                    None => empty_fill.unwrap_or(style.background),
                };
                let a = color[3] as f64 / 255.0 * cover;
                let base = out.px(py, px);
                let mut mixed = [0u8; 4];
                for c in 0..3 {
                    mixed[c] = (color[c] as f64 * a + base[c] as f64 * (1.0 - a)).round() as u8;
                }
                mixed[3] = (a * 255.0 + base[3] as f64 * (1.0 - a)).round().min(255.0) as u8;
                out.set_px(py, px, mixed);
            }
        }
    }
    out
}

/// Ô nào chứa điểm `(x, y)` (px canvas thật)? Dùng cho bấm / kéo trong GUI.
pub fn cell_at(template_name: &str, width: u32, height: u32, spacing: f64, x: f64, y: f64) -> Option<usize> {
    cell_rects(template_name, width, height, spacing)
        .iter()
        .position(|c| x >= c.x && y >= c.y && x <= c.x + c.w && y <= c.y + c.h)
}

/// Độ phủ (0..1) của pixel tâm `(fx, fy)` trong hình chữ nhật bo góc.
fn coverage(fx: f64, fy: f64, x: f64, y: f64, w: f64, h: f64, r: f64) -> f64 {
    // phần ngoài hình chữ nhật (khử răng cưa ở mép thẳng)
    let edge = (fx - x + 0.5).min(x + w - fx + 0.5).min(fy - y + 0.5).min(y + h - fy + 0.5);
    let mut c = edge.clamp(0.0, 1.0);
    if r > 0.0 {
        let qx = if fx < x + r {
            x + r - fx
        } else if fx > x + w - r {
            fx - (x + w - r)
        } else {
            0.0
        };
        let qy = if fy < y + r {
            y + r - fy
        } else if fy > y + h - r {
            fy - (y + h - r)
        } else {
            0.0
        };
        if qx > 0.0 && qy > 0.0 {
            let d = (qx * qx + qy * qy).sqrt();
            c = c.min((r - d + 0.5).clamp(0.0, 1.0));
        }
    }
    c
}

/// Thu nhỏ theo hệ số nguyên bằng trung bình khối.
fn box_downscale(img: &RgbaImage, factor: usize) -> RgbaImage {
    let w = (img.w / factor).max(1);
    let h = (img.h / factor).max(1);
    let mut out = RgbaImage::new(h, w);
    for oy in 0..h {
        for ox in 0..w {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for y in oy * factor..((oy + 1) * factor).min(img.h) {
                for x in ox * factor..((ox + 1) * factor).min(img.w) {
                    let p = img.px(y, x);
                    for c in 0..4 {
                        acc[c] += p[c] as u32;
                    }
                    n += 1;
                }
            }
            let n = n.max(1);
            out.set_px(oy, ox, [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, (acc[3] / n) as u8]);
        }
    }
    out
}

/// Lấy mẫu song tuyến tính, ngoài biên thì kẹp.
fn sample_bilinear(img: &RgbaImage, u: f64, v: f64) -> [u8; 4] {
    if img.is_empty() {
        return [0, 0, 0, 0];
    }
    let fx = (u - 0.5).clamp(0.0, img.w as f64 - 1.0);
    let fy = (v - 0.5).clamp(0.0, img.h as f64 - 1.0);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(img.w - 1), (y0 + 1).min(img.h - 1));
    let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
    let (p00, p10, p01, p11) = (img.px(y0, x0), img.px(y0, x1), img.px(y1, x0), img.px(y1, x1));
    let mut out = [0u8; 4];
    for c in 0..4 {
        let top = p00[c] as f64 * (1.0 - tx) + p10[c] as f64 * tx;
        let bot = p01[c] as f64 * (1.0 - tx) + p11[c] as f64 * tx;
        out[c] = (top * (1.0 - ty) + bot * ty).round().clamp(0.0, 255.0) as u8;
    }
    out
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

    fn style(template: &str, spacing: f64, radius: f64) -> CollageStyle {
        CollageStyle {
            template: template.to_string(),
            width: 400,
            height: 200,
            spacing,
            radius,
            background: [255, 255, 255, 255],
        }
    }

    #[test]
    fn render_collage_places_images_and_background() {
        let red = RgbaImage::filled(50, 80, [200, 0, 0, 255]);
        let blue = RgbaImage::filled(300, 300, [0, 0, 200, 255]);
        let st = style("two_horizontal", 20.0, 0.0);
        let cells = [
            Some(CellSource { img: &red, zoom: 1.0, offset: (0.0, 0.0) }),
            Some(CellSource { img: &blue, zoom: 1.0, offset: (0.0, 0.0) }),
        ];
        let out = render_collage(&st, &cells, 1.0, None);
        assert_eq!((out.w, out.h), (400, 200));
        assert_eq!(out.px(100, 100), [200, 0, 0, 255]); // giữa ô trái
        assert_eq!(out.px(100, 300), [0, 0, 200, 255]); // giữa ô phải (ảnh lớn được thu nhỏ)
        assert_eq!(out.px(5, 5), [255, 255, 255, 255]); // viền = nền
        assert_eq!(out.px(100, 200), [255, 255, 255, 255]); // khe giữa 2 ô
        // xem trước nửa tỉ lệ cùng bố cục
        let half = render_collage(&st, &cells, 0.5, None);
        assert_eq!((half.w, half.h), (200, 100));
        assert_eq!(half.px(50, 50), [200, 0, 0, 255]);
    }

    #[test]
    fn render_collage_rounds_corners_and_fills_empty_cells() {
        let red = RgbaImage::filled(10, 10, [200, 0, 0, 255]);
        let st = style("two_horizontal", 0.0, 40.0);
        let cells = [Some(CellSource { img: &red, zoom: 1.0, offset: (0.0, 0.0) }), None];
        let out = render_collage(&st, &cells, 1.0, None);
        assert_eq!(out.px(1, 1), [255, 255, 255, 255]); // góc bo → nền
        assert_eq!(out.px(100, 100), [200, 0, 0, 255]);
        assert_eq!(out.px(100, 300), [255, 255, 255, 255]); // ô trống khi xuất = nền
        let preview = render_collage(&st, &cells, 1.0, Some([40, 40, 40, 255]));
        assert_eq!(preview.px(100, 300), [40, 40, 40, 255]); // ô trống khi xem trước
    }

    #[test]
    fn cell_at_finds_the_cell_under_the_point() {
        assert_eq!(cell_at("two_horizontal", 400, 200, 20.0, 100.0, 100.0), Some(0));
        assert_eq!(cell_at("two_horizontal", 400, 200, 20.0, 300.0, 100.0), Some(1));
        assert_eq!(cell_at("two_horizontal", 400, 200, 20.0, 200.0, 100.0), None); // khe
        assert_eq!(cell_at("grid_3x3", 900, 900, 0.0, 850.0, 850.0), Some(8));
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
