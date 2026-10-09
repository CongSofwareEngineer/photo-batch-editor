//! Ghép tài liệu photo editor thành pixel, **bằng CPU**. Port của `ui/photo/render.py`.
//!
//! Một hàm duy nhất [`render`] dùng cho cả xem trước (scale < 1, nhanh) và xuất file
//! (scale = 1), nên những gì thấy trên canvas đúng bằng những gì được ghi ra.

use pbe_core::photo::effects::{apply_blur_effect, apply_photo_adjust};
use pbe_core::photo::geometry::cover_scale;
use pbe_core::photo::RgbaImage;

use crate::photo::doc::{DocState, Layer};
use crate::preview::downscale_to;

/// Ghép tài liệu ở tỉ lệ `scale` (1.0 = độ phân giải thật).
pub fn render(state: &DocState, scale: f64) -> RgbaImage {
    let bg = &state.background;
    let full_long = bg.img.long_side();
    let mut canvas = if scale < 0.999 {
        let target = ((full_long as f64 * scale).round() as usize).max(1);
        downscale_to(&bg.img, target)
    } else {
        bg.img.clone()
    };
    if canvas.is_empty() {
        return canvas;
    }
    // tỉ lệ thực tế sau khi làm tròn
    let s = canvas.w as f64 / bg.img.w.max(1) as f64;

    if !bg.visible {
        canvas = RgbaImage::new(canvas.h, canvas.w);
    } else {
        if !bg.adjust.is_default() {
            canvas = apply_photo_adjust(&canvas, &bg.adjust, Some(full_long));
        }
        if bg.angle.abs() > 1e-6 {
            canvas = rotate_cover(&canvas, bg.angle);
        }
    }

    for layer in &state.layers {
        if !layer.visible() {
            continue;
        }
        match layer {
            Layer::Image(l) => draw_transformed(
                &mut canvas,
                &l.img,
                l.x * s,
                l.y * s,
                l.width * s,
                l.height * s,
                l.rotation,
                l.flip_h,
                l.flip_v,
                l.opacity,
            ),
            Layer::Blur(l) => {
                let rect = (l.rect.0 * s, l.rect.1 * s, l.rect.2 * s, l.rect.3 * s);
                apply_blur_layer(&mut canvas, rect, &l.mode, l.strength, &l.shape, l.opacity);
            }
        }
    }
    canvas
}

/// Xoay quanh tâm và phóng để vẫn phủ kín khung (làm thẳng ảnh, như Photoshop).
pub fn rotate_cover(img: &RgbaImage, angle_deg: f64) -> RgbaImage {
    let (w, h) = (img.w as f64, img.h as f64);
    let k = cover_scale(w, h, angle_deg);
    let a = angle_deg.to_radians();
    let (cos, sin) = (a.cos(), a.sin());
    let (cx, cy) = (w / 2.0, h / 2.0);
    let mut out = RgbaImage::new(img.h, img.w);
    for y in 0..img.h {
        for x in 0..img.w {
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            // nghịch đảo của: dest = center + R(a) * (src - center) * k
            let sx = (dx * cos + dy * sin) / k + cx;
            let sy = (-dx * sin + dy * cos) / k + cy;
            out.set_px(y, x, sample_bilinear(img, sx, sy));
        }
    }
    out
}

/// Vẽ `src` vào `dst` ở tâm `(cx, cy)`, cỡ `w×h`, xoay `rot` độ, có thể lật.
#[allow(clippy::too_many_arguments)]
pub fn draw_transformed(
    dst: &mut RgbaImage,
    src: &RgbaImage,
    cx: f64,
    cy: f64,
    w: f64,
    h: f64,
    rot_deg: f64,
    flip_h: bool,
    flip_v: bool,
    opacity: f32,
) {
    if src.is_empty() || w <= 0.0 || h <= 0.0 {
        return;
    }
    let a = rot_deg.to_radians();
    let (cos, sin) = (a.cos(), a.sin());
    // hộp bao của hình chữ nhật đã xoay
    let half_w = (w * cos.abs() + h * sin.abs()) / 2.0;
    let half_h = (w * sin.abs() + h * cos.abs()) / 2.0;
    let x0 = ((cx - half_w).floor().max(0.0) as usize).min(dst.w);
    let y0 = ((cy - half_h).floor().max(0.0) as usize).min(dst.h);
    let x1 = (((cx + half_w).ceil().max(0.0)) as usize).min(dst.w);
    let y1 = (((cy + half_h).ceil().max(0.0)) as usize).min(dst.h);
    let op = opacity.clamp(0.0, 1.0);
    for y in y0..y1 {
        for x in x0..x1 {
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            // về hệ của layer (chưa xoay)
            let lx = dx * cos + dy * sin;
            let ly = -dx * sin + dy * cos;
            // về px của ảnh nguồn
            let mut u = (lx + w / 2.0) / w * src.w as f64;
            let mut v = (ly + h / 2.0) / h * src.h as f64;
            if flip_h {
                u = src.w as f64 - u;
            }
            if flip_v {
                v = src.h as f64 - v;
            }
            if u < 0.0 || v < 0.0 || u >= src.w as f64 || v >= src.h as f64 {
                continue;
            }
            let mut c = sample_bilinear(src, u, v);
            c[3] = (c[3] as f32 * op).round() as u8;
            blend_px(dst, x, y, c);
        }
    }
}

/// Làm mờ / ô vuông hoá vùng `rect` của `canvas` ngay tại chỗ.
fn apply_blur_layer(
    canvas: &mut RgbaImage,
    rect: (f64, f64, f64, f64),
    mode: &str,
    strength: f64,
    shape: &str,
    opacity: f32,
) {
    let x0 = (rect.0.floor().max(0.0) as usize).min(canvas.w);
    let y0 = (rect.1.floor().max(0.0) as usize).min(canvas.h);
    let x1 = (((rect.0 + rect.2).ceil().max(0.0)) as usize).min(canvas.w);
    let y1 = (((rect.1 + rect.3).ceil().max(0.0)) as usize).min(canvas.h);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let region = canvas.crop(x0, y0, x1 - x0, y1 - y0);
    let long_side = canvas.long_side();
    let blurred = apply_blur_effect(&region, mode, strength, long_side, (x0 as i64, y0 as i64));
    let (rw, rh) = (blurred.w as f64, blurred.h as f64);
    let op = opacity.clamp(0.0, 1.0);
    for ry in 0..blurred.h {
        for rx in 0..blurred.w {
            if shape == "ellipse" {
                // trong hình elip nội tiếp vùng?
                let nx = (rx as f64 + 0.5) / rw * 2.0 - 1.0;
                let ny = (ry as f64 + 0.5) / rh * 2.0 - 1.0;
                if nx * nx + ny * ny > 1.0 {
                    continue;
                }
            }
            let mut c = blurred.px(ry, rx);
            c[3] = (c[3] as f32 * op).round() as u8;
            blend_px(canvas, x0 + rx, y0 + ry, c);
        }
    }
}

#[inline]
fn blend_px(dst: &mut RgbaImage, x: usize, y: usize, src: [u8; 4]) {
    if src[3] == 0 || x >= dst.w || y >= dst.h {
        return;
    }
    if src[3] == 255 {
        dst.set_px(y, x, src);
        return;
    }
    let base = dst.px(y, x);
    let a = src[3] as u32;
    let mut out = [0u8; 4];
    for c in 0..3 {
        out[c] = ((src[c] as u32 * a + base[c] as u32 * (255 - a)) / 255) as u8;
    }
    out[3] = (a + base[3] as u32 * (255 - a) / 255).min(255) as u8;
    dst.set_px(y, x, out);
}

/// Lấy mẫu song tuyến tính; ngoài biên thì kẹp.
pub fn sample_bilinear(img: &RgbaImage, u: f64, v: f64) -> [u8; 4] {
    if img.is_empty() {
        return [0, 0, 0, 0];
    }
    let fx = (u - 0.5).clamp(0.0, img.w as f64 - 1.0);
    let fy = (v - 0.5).clamp(0.0, img.h as f64 - 1.0);
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let x1 = (x0 + 1).min(img.w - 1);
    let y1 = (y0 + 1).min(img.h - 1);
    let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
    let (p00, p10, p01, p11) = (
        img.px(y0, x0),
        img.px(y0, x1),
        img.px(y1, x0),
        img.px(y1, x1),
    );
    let mut out = [0u8; 4];
    for c in 0..4 {
        let top = p00[c] as f32 * (1.0 - tx) + p10[c] as f32 * tx;
        let bot = p01[c] as f32 * (1.0 - tx) + p11[c] as f32 * tx;
        out[c] = (top * (1.0 - ty) + bot * ty).round().clamp(0.0, 255.0) as u8;
    }
    out
}
