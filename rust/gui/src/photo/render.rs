//! Ghép tài liệu photo editor thành pixel, **bằng CPU**. Port của `ui/photo/render.py`.
//! Thứ tự nền: Adjust → Làm mịn da → Unsharp Mask → tách nền (matte) → xoay tự do → nền thay thế.
//! Nền nằm ở vị trí `DocState::bg_index` trong ngăn layer (mặc định dưới cùng).
//!
//! Một hàm duy nhất [`render`] dùng cho cả xem trước (scale < 1, nhanh) và xuất file
//! (scale = 1), nên những gì thấy trên canvas đúng bằng những gì được ghi ra.

use std::sync::Arc;

use pbe_core::photo::effects::{
    apply_blur_effect, apply_photo_adjust, unsharp_mask, PhotoAdjust, SharpenSettings,
};
use pbe_core::photo::geometry::cover_scale;
use pbe_core::photo::matting::{apply_cutout, composite_backdrop, dab_weight};
use pbe_core::photo::skin::{smooth_skin, SkinSmoothSettings};
use pbe_core::photo::RgbaImage;

use crate::photo::doc::{CloneLayer, DocState, Layer};
use crate::preview::downscale_to;

/// Bộ nhớ đệm ảnh nền đã chỉnh (Adjust + làm mịn da + Unsharp Mask): kéo layer, tô Tem nhân bản…
/// không phải tính lại các bộ lọc nặng mỗi khung hình.
#[derive(Default)]
pub struct BaseCache {
    key: Option<BaseKey>,
    value: Option<(RgbaImage, f64)>,
}

struct BaseKey {
    img: Arc<RgbaImage>,
    adjust: PhotoAdjust,
    skin: SkinSmoothSettings,
    sharpen: SharpenSettings,
    scale: f64,
}

impl BaseKey {
    fn of(state: &DocState, scale: f64) -> Self {
        let bg = &state.background;
        BaseKey {
            img: bg.img.clone(),
            adjust: bg.adjust,
            skin: bg.skin,
            sharpen: bg.sharpen,
            scale,
        }
    }

    fn same(&self, o: &BaseKey) -> bool {
        Arc::ptr_eq(&self.img, &o.img)
            && self.adjust == o.adjust
            && self.skin == o.skin
            && self.sharpen == o.sharpen
            && self.scale == o.scale
    }
}

impl BaseCache {
    fn get(&mut self, state: &DocState, scale: f64) -> (RgbaImage, f64) {
        let key = BaseKey::of(state, scale);
        if let (Some(k), Some(v)) = (&self.key, &self.value) {
            if k.same(&key) {
                return v.clone();
            }
        }
        let v = adjusted_background(state, scale);
        self.key = Some(key);
        self.value = Some(v.clone());
        v
    }
}

/// Ảnh nền ở tỉ lệ `scale` đã Adjust + làm mịn da + Unsharp Mask (chưa tách nền, chưa xoay tự do)
/// và tỉ lệ thực tế sau khi làm tròn.
fn adjusted_background(state: &DocState, scale: f64) -> (RgbaImage, f64) {
    let bg = &state.background;
    let full_long = bg.img.long_side();
    let mut canvas = if scale < 0.999 {
        let target = ((full_long as f64 * scale).round() as usize).max(1);
        downscale_to(&bg.img, target)
    } else {
        bg.img.as_ref().clone()
    };
    let s = canvas.w as f64 / bg.img.w.max(1) as f64;
    if canvas.is_empty() {
        return (canvas, s);
    }
    if !bg.adjust.is_default() {
        canvas = apply_photo_adjust(&canvas, &bg.adjust, Some(full_long));
    }
    if !bg.skin.is_default() {
        canvas = smooth_skin(&canvas, &bg.skin, s);
    }
    if !bg.sharpen.is_default() {
        canvas = unsharp_mask(&canvas, &bg.sharpen, s);
    }
    (canvas, s)
}

/// Nền cho chế độ chỉnh vùng tách nền: chỉ ảnh nền đã chỉnh màu — không matte, không xoay tự do,
/// không layer — để pixel trên canvas khớp đúng toạ độ của matte khi tô cọ.
pub fn render_mask_base(state: &DocState, scale: f64) -> RgbaImage {
    adjusted_background(state, scale).0
}

/// [`render_mask_base`] dùng bộ nhớ đệm.
pub fn render_mask_base_cached(state: &DocState, scale: f64, cache: &mut BaseCache) -> RgbaImage {
    cache.get(state, scale).0
}

/// Ghép tài liệu ở tỉ lệ `scale` (1.0 = độ phân giải thật).
pub fn render(state: &DocState, scale: f64) -> RgbaImage {
    compose(state, adjusted_background(state, scale))
}

/// [`render`] dùng bộ nhớ đệm ảnh nền đã chỉnh (canvas).
pub fn render_cached(state: &DocState, scale: f64, cache: &mut BaseCache) -> RgbaImage {
    compose(state, cache.get(state, scale))
}

fn compose(state: &DocState, (mut base, s): (RgbaImage, f64)) -> RgbaImage {
    let bg = &state.background;
    if base.is_empty() {
        return base;
    }
    if bg.visible {
        // Tách nền: matte → alpha (sau chỉnh màu để khử màu viền dùng đúng màu đang thấy).
        if let Some(c) = &bg.cutout {
            apply_cutout(&mut base, c, s);
        }
        if bg.angle.abs() > 1e-6 {
            base = rotate_cover(&base, bg.angle);
        }
        // Nền thay thế vẽ sau khi xoay để luôn phủ kín khung.
        if let Some(c) = &bg.cutout {
            base = composite_backdrop(&base, &c.backdrop);
        }
    }
    let below = state.bg_pos();
    let opacity = bg.opacity.clamp(0.0, 1.0);
    if bg.visible && below == 0 && opacity >= 1.0 {
        // nền dưới cùng, đục (mặc định): dùng thẳng làm canvas
        draw_layers(&mut base, &state.layers, s);
        return base;
    }
    let mut canvas = RgbaImage::new(base.h, base.w);
    draw_layers(&mut canvas, &state.layers[..below], s);
    if bg.visible {
        blend_image(&mut canvas, &base, opacity);
    }
    draw_layers(&mut canvas, &state.layers[below..], s);
    canvas
}

fn draw_layers(canvas: &mut RgbaImage, layers: &[Layer], s: f64) {
    for layer in layers {
        if !layer.visible() {
            continue;
        }
        match layer {
            Layer::Image(l) => draw_transformed(
                canvas,
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
                apply_blur_layer(canvas, rect, &l.mode, l.strength, &l.shape, l.opacity);
            }
            Layer::Clone(l) => apply_clone_layer(canvas, l, s),
        }
    }
}

/// Vẽ cả ảnh `src` (cùng cỡ) lên `dst` với độ trong suốt `opacity`.
fn blend_image(dst: &mut RgbaImage, src: &RgbaImage, opacity: f32) {
    let op = opacity.clamp(0.0, 1.0);
    for y in 0..src.h.min(dst.h) {
        for x in 0..src.w.min(dst.w) {
            let mut c = src.px(y, x);
            c[3] = (c[3] as f32 * op).round() as u8;
            blend_px(dst, x, y, c);
        }
    }
}

/// Tem nhân bản: mỗi nét tô pixel lấy từ chỗ lệch `offset` của chính ảnh ghép bên dưới (lấy mẫu
/// trước khi nét được vẽ, nên nét không tự lặp lại), mép cọ mềm theo độ cứng.
fn apply_clone_layer(canvas: &mut RgbaImage, l: &CloneLayer, s: f64) {
    for st in &l.strokes {
        if st.points.is_empty() {
            continue;
        }
        let r = (st.radius * s).max(0.5);
        let inner = r * st.hardness.clamp(0.0, 1.0);
        let pts: Vec<(f64, f64)> = st.points.iter().map(|&(x, y)| (x * s, y * s)).collect();
        // hình bao nét (px canvas, kẹp trong ảnh)
        let (mut ax, mut ay, mut bx, mut by) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &(x, y) in &pts {
            (ax, ay, bx, by) = (ax.min(x - r), ay.min(y - r), bx.max(x + r), by.max(y + r));
        }
        let x0 = (ax.floor().max(0.0) as usize).min(canvas.w);
        let y0 = (ay.floor().max(0.0) as usize).min(canvas.h);
        let x1 = (bx.ceil().max(0.0) as usize).min(canvas.w);
        let y1 = (by.ceil().max(0.0) as usize).min(canvas.h);
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        let (mw, mh) = (x1 - x0, y1 - y0);
        // độ phủ của nét = max trọng số các chấm cọ (cách nhau ¼ bán kính)
        let mut mask = vec![0f32; mw * mh];
        let mut dab = |cx: f64, cy: f64| {
            let dx0 = ((cx - r).floor().max(x0 as f64) as usize).min(x1);
            let dy0 = ((cy - r).floor().max(y0 as f64) as usize).min(y1);
            let dx1 = ((cx + r).ceil().max(0.0) as usize).min(x1);
            let dy1 = ((cy + r).ceil().max(0.0) as usize).min(y1);
            for y in dy0..dy1 {
                let ddy = y as f64 + 0.5 - cy;
                for x in dx0..dx1 {
                    let ddx = x as f64 + 0.5 - cx;
                    let k = dab_weight((ddx * ddx + ddy * ddy).sqrt(), r, inner) as f32;
                    let m = &mut mask[(y - y0) * mw + (x - x0)];
                    *m = m.max(k);
                }
            }
        };
        dab(pts[0].0, pts[0].1);
        let step = (r * 0.25).max(0.5);
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let n = (len / step).ceil().max(1.0) as usize;
            for i in 1..=n {
                let t = i as f64 / n as f64;
                dab(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            }
        }
        // vùng nguồn (lệch `offset`), chụp lại trước khi vẽ
        let (ox, oy) = (st.offset.0 * s, st.offset.1 * s);
        let sx0 = ((x0 as f64 + ox - 1.0).floor().max(0.0) as usize).min(canvas.w);
        let sy0 = ((y0 as f64 + oy - 1.0).floor().max(0.0) as usize).min(canvas.h);
        let sx1 = ((x1 as f64 + ox + 1.0).ceil().max(0.0) as usize).min(canvas.w);
        let sy1 = ((y1 as f64 + oy + 1.0).ceil().max(0.0) as usize).min(canvas.h);
        if sx0 >= sx1 || sy0 >= sy1 {
            continue; // nguồn nằm hẳn ngoài ảnh
        }
        let src = canvas.crop(sx0, sy0, sx1 - sx0, sy1 - sy0);
        let op = st.opacity.clamp(0.0, 1.0) * l.opacity.clamp(0.0, 1.0);
        for y in y0..y1 {
            for x in x0..x1 {
                let k = mask[(y - y0) * mw + (x - x0)] * op;
                if k <= 0.0 {
                    continue;
                }
                let c = sample_bilinear(
                    &src,
                    x as f64 + 0.5 + ox - sx0 as f64,
                    y as f64 + 0.5 + oy - sy0 as f64,
                );
                let d = canvas.px(y, x);
                let mut out = [0u8; 4];
                for i in 0..4 {
                    out[i] = (d[i] as f32 + k * (c[i] as f32 - d[i] as f32)).round() as u8;
                }
                canvas.set_px(y, x, out);
            }
        }
    }
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
