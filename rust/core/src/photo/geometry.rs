//! Hình học của photo editor: bước zoom, zoom tại một điểm, tỉ lệ crop, xoay canvas.
//! Port của `core/photo/geometry.py`.

pub const ZOOM_MIN: f64 = 0.10; // 10 %
pub const ZOOM_MAX: f64 = 16.0; // 1600 %

/// Các mức zoom kiểu Photoshop.
pub const ZOOM_STEPS: [f64; 17] = [
    0.10,
    0.125,
    1.0 / 6.0,
    0.25,
    1.0 / 3.0,
    0.5,
    2.0 / 3.0,
    1.0,
    1.5,
    2.0,
    3.0,
    4.0,
    5.0,
    6.0,
    8.0,
    12.0,
    16.0,
];

/// Nhãn → tỉ lệ rộng / cao (`None` = tự do). "original" do người gọi tự giải.
pub const CROP_RATIOS: [(&str, Option<f64>); 5] = [
    ("free", None),
    ("1:1", Some(1.0)),
    ("4:3", Some(4.0 / 3.0)),
    ("16:9", Some(16.0 / 9.0)),
    ("9:16", Some(9.0 / 16.0)),
];

pub fn crop_ratio(label: &str) -> Option<f64> {
    CROP_RATIOS
        .iter()
        .find(|(name, _)| *name == label)
        .and_then(|(_, r)| *r)
}

pub fn clamp_zoom(scale: f64) -> f64 {
    scale.clamp(ZOOM_MIN, ZOOM_MAX)
}

/// Mức zoom kế tiếp phía trên (`direction > 0`) hoặc phía dưới mức hiện tại.
pub fn next_zoom(scale: f64, direction: i32) -> f64 {
    if direction > 0 {
        for z in ZOOM_STEPS {
            if z > scale * 1.001 {
                return z;
            }
        }
        return ZOOM_MAX;
    }
    for z in ZOOM_STEPS.iter().rev() {
        if *z < scale / 1.001 {
            return *z;
        }
    }
    ZOOM_MIN
}

/// Offset mới của khung xem để điểm ảnh dưới `anchor` (px màn hình) nằm nguyên tại đó.
///
/// Khung xem map `screen = offset + image * scale`.
pub fn zoom_at(scale: f64, new_scale: f64, offset: (f64, f64), anchor: (f64, f64)) -> (f64, f64) {
    let ix = (anchor.0 - offset.0) / scale;
    let iy = (anchor.1 - offset.1) / scale;
    (anchor.0 - ix * new_scale, anchor.1 - iy * new_scale)
}

/// Mức zoom để ảnh vừa khung xem, chừa lề `margin` px mỗi bên.
pub fn fit_scale(img_w: usize, img_h: usize, view_w: i64, view_h: i64, margin: i64) -> f64 {
    let w = (view_w - 2 * margin).max(1) as f64;
    let h = (view_h - 2 * margin).max(1) as f64;
    clamp_zoom((w / img_w.max(1) as f64).min(h / img_h.max(1) as f64))
}

/// Lề mặc định của [`fit_scale`] (giống `margin=24` của bản Python).
pub const FIT_MARGIN: i64 = 24;

/// Hình chữ nhật crop kéo từ `anchor` tới `pointer`, trả về `(x, y, w, h)`.
///
/// Có `ratio` (rộng / cao) thì giữ đúng tỉ lệ; kết quả luôn nằm trong ảnh `bounds` (w, h).
pub fn crop_rect(
    anchor: (f64, f64),
    pointer: (f64, f64),
    ratio: Option<f64>,
    bounds: (usize, usize),
) -> (f64, f64, f64, f64) {
    let (bw, bh) = (bounds.0 as f64, bounds.1 as f64);
    let ax = anchor.0.clamp(0.0, bw);
    let ay = anchor.1.clamp(0.0, bh);
    let px = pointer.0.clamp(0.0, bw);
    let py = pointer.1.clamp(0.0, bh);
    let mut w = (px - ax).abs();
    let mut h = (py - ay).abs();
    let sx = if px >= ax { 1.0 } else { -1.0 };
    let sy = if py >= ay { 1.0 } else { -1.0 };
    if let Some(ratio) = ratio.filter(|r| *r != 0.0) {
        // ép hộp của con trỏ về tỉ lệ, rồi thu lại cho vừa trong ảnh
        if w / h.max(1e-9) > ratio {
            h = w / ratio;
        } else {
            w = h * ratio;
        }
        let max_w = if sx < 0.0 { ax } else { bw - ax };
        let max_h = if sy < 0.0 { ay } else { bh - ay };
        let k = 1.0f64.min(max_w / w.max(1e-9)).min(max_h / h.max(1e-9));
        w *= k;
        h *= k;
    }
    let x = if sx > 0.0 { ax } else { ax - w };
    let y = if sy > 0.0 { ay } else { ay - h };
    (x, y, w, h)
}

/// Hình chữ nhật pixel nguyên nằm trong ảnh (tối thiểu 1×1).
pub fn round_rect(rect: (f64, f64, f64, f64), bounds: (usize, usize)) -> (usize, usize, usize, usize) {
    let (bw, bh) = (bounds.0 as f64, bounds.1 as f64);
    // `round_ties_even` = `round()` của Python (làm tròn .5 về số chẵn).
    let x0 = rect.0.clamp(0.0, bw - 1.0).round_ties_even();
    let y0 = rect.1.clamp(0.0, bh - 1.0).round_ties_even();
    let x1 = (rect.0 + rect.2).clamp(x0 + 1.0, bw).round_ties_even();
    let y1 = (rect.1 + rect.3).clamp(y0 + 1.0, bh).round_ties_even();
    (x0 as usize, y0 as usize, (x1 - x0) as usize, (y1 - y0) as usize)
}

/// Điểm ảnh đi đâu khi canvas cỡ `size` (w, h) được xoay 90°.
pub fn rotate90_point(x: f64, y: f64, size: (usize, usize), clockwise: bool) -> (f64, f64) {
    let (w, h) = (size.0 as f64, size.1 as f64);
    if clockwise {
        (h - y, x)
    } else {
        (y, w - x)
    }
}

pub fn flip_point(x: f64, y: f64, size: (usize, usize), horizontal: bool) -> (f64, f64) {
    let (w, h) = (size.0 as f64, size.1 as f64);
    if horizontal {
        (w - x, y)
    } else {
        (x, h - y)
    }
}

/// Hệ số phóng để ảnh `w×h` xoay `angle` độ vẫn phủ kín canvas `w×h`.
pub fn cover_scale(w: f64, h: f64, angle_deg: f64) -> f64 {
    let a = angle_deg.to_radians();
    let (c, s) = (a.cos().abs(), a.sin().abs());
    ((w * c + h * s) / w).max((w * s + h * c) / h)
}

/// Góc trong khoảng (-180, 180].
pub fn normalize_angle(a: f64) -> f64 {
    let mut a = a % 360.0;
    if a <= -180.0 {
        a += 360.0;
    } else if a > 180.0 {
        a -= 360.0;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_range_and_steps() {
        // Giống test_zoom_range_and_steps.
        assert_eq!(clamp_zoom(0.01), ZOOM_MIN);
        assert_eq!(ZOOM_MIN, 0.10);
        assert_eq!(clamp_zoom(100.0), ZOOM_MAX);
        assert_eq!(ZOOM_MAX, 16.0);
        assert!(next_zoom(1.0, 1) > 1.0 && next_zoom(1.0, -1) < 1.0);
        assert_eq!(next_zoom(16.0, 1), 16.0);
        assert_eq!(next_zoom(0.1, -1), 0.1);
    }

    #[test]
    fn zoom_at_keeps_point_under_cursor() {
        // Giống test_zoom_at_keeps_point_under_cursor.
        let (offset, anchor) = ((30.0, -12.0), (412.0, 250.0));
        let (old, new) = (0.5, 2.0);
        let ipt = ((anchor.0 - offset.0) / old, (anchor.1 - offset.1) / old);
        let (ox, oy) = zoom_at(old, new, offset, anchor);
        assert!((ox + ipt.0 * new - anchor.0).abs() < 1e-9);
        assert!((oy + ipt.1 * new - anchor.1).abs() < 1e-9);
    }

    #[test]
    fn crop_rect_ratio_and_bounds() {
        // Giống test_crop_rect_ratio_and_bounds.
        for ratio in [None, Some(1.0), Some(4.0 / 3.0), Some(16.0 / 9.0), Some(9.0 / 16.0)] {
            for (anchor, pointer) in [
                ((100.0, 100.0), (900.0, 400.0)),
                ((500.0, 500.0), (-50.0, 20.0)),
                ((10.0, 590.0), (790.0, 0.0)),
            ] {
                let (x, y, w, h) = crop_rect(anchor, pointer, ratio, (800, 600));
                assert!(x >= -1e-6 && y >= -1e-6);
                assert!(x + w <= 800.0 + 1e-6 && y + h <= 600.0 + 1e-6);
                if let Some(r) = ratio {
                    assert!((w / h - r).abs() <= r * 1e-6, "{w}×{h} khác tỉ lệ {r}");
                }
            }
        }
    }

    #[test]
    fn round_rect_inside_image() {
        // Giống test_round_rect_inside_image.
        assert_eq!(round_rect((-5.0, -5.0, 50.4, 20.6), (40, 30)), (0, 0, 40, 16));
    }

    #[test]
    fn rotate_and_flip_points() {
        // Giống test_rotate_and_flip_points.
        let size = (200, 100);
        assert_eq!(rotate90_point(0.0, 0.0, size, true), (100.0, 0.0));
        assert_eq!(rotate90_point(200.0, 100.0, size, true), (0.0, 200.0));
        assert_eq!(rotate90_point(0.0, 0.0, size, false), (0.0, 200.0));
        let (x, y) = rotate90_point(30.0, 40.0, size, true);
        assert_eq!(rotate90_point(x, y, (100, 200), false), (30.0, 40.0));
        assert_eq!(flip_point(30.0, 40.0, size, true), (170.0, 40.0));
        assert_eq!(flip_point(30.0, 40.0, size, false), (30.0, 60.0));
    }

    #[test]
    fn cover_scale_covers_the_canvas() {
        // Giống test_cover_scale.
        assert!((cover_scale(400.0, 300.0, 0.0) - 1.0).abs() < 1e-9);
        assert!(cover_scale(400.0, 300.0, 10.0) > 1.0);
        let k = cover_scale(400.0, 300.0, 30.0);
        let a = 30f64.to_radians();
        for (cx, cy) in [(-200.0, -150.0), (200.0, -150.0), (200.0, 150.0), (-200.0, 150.0)] {
            let lx: f64 = (cx * a.cos() + cy * a.sin()) / k;
            let ly: f64 = (-cx * a.sin() + cy * a.cos()) / k;
            assert!(lx.abs() <= 200.0 + 1e-6 && ly.abs() <= 150.0 + 1e-6);
        }
    }

    #[test]
    fn normalize_angle_range() {
        assert_eq!(normalize_angle(180.0), 180.0);
        assert_eq!(normalize_angle(-180.0), 180.0);
        assert_eq!(normalize_angle(450.0), 90.0);
        assert_eq!(normalize_angle(-270.0), 90.0);
    }

    #[test]
    fn fit_scale_leaves_a_margin() {
        assert!(fit_scale(1000, 1000, 500, 500, FIT_MARGIN) < 0.5);
        assert_eq!(fit_scale(10, 10, 5000, 5000, FIT_MARGIN), ZOOM_MAX);
    }
}
