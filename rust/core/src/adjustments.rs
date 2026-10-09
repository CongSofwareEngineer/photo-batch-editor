//! 15 chỉnh sửa kiểu Camera Raw — bản fused (fast path) mà code sản xuất dùng.
//! Port của `core/adjustments.py` (các hàm nhóm: to_planar, tone_local, color, detail, effects…).
//!
//! Dùng mặt phẳng float32 `(3, H, W)` như bản Python. Các hàm reference một-thông-số chỉ phục vụ
//! test Python nên không port riêng; test Rust chứng minh tương đương bằng cách khớp output fixture.

use crate::backend::{self, Plane};
use crate::settings::AdjustmentSettings;

pub const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
pub const GAIN_MAX: f32 = 8.0;
pub const NR_SUBSAMPLE_RADIUS: usize = 3;
pub const VIGNETTE_GRID: usize = 8;

/// Ảnh HWC 3 kênh uint8.
#[derive(Debug, Clone)]
pub struct ImageU8 {
    pub h: usize,
    pub w: usize,
    pub data: Vec<u8>, // h*w*3, thứ tự RGB
}

/// Ảnh HWC 3 kênh float32 (trong [0, 1]).
#[derive(Debug, Clone)]
pub struct ImageF32 {
    pub h: usize,
    pub w: usize,
    pub data: Vec<f32>, // h*w*3
}

/// Ảnh HWC 3 kênh uint16 (nguồn 16-bit, vd. TIFF 16-bit).
#[derive(Debug, Clone)]
pub struct ImageU16 {
    pub h: usize,
    pub w: usize,
    pub data: Vec<u16>, // h*w*3, thứ tự RGB
}

/// Ảnh planar float32 (3, H, W): mỗi kênh một mặt phẳng liền nhau.
#[derive(Debug, Clone)]
pub struct Planar {
    pub h: usize,
    pub w: usize,
    pub p: [Vec<f32>; 3], // mỗi Vec dài h*w
}

impl Planar {
    fn new(h: usize, w: usize) -> Self {
        Planar {
            h,
            w,
            p: [vec![0.0; h * w], vec![0.0; h * w], vec![0.0; h * w]],
        }
    }
}

pub fn long_side_of(h: usize, w: usize) -> usize {
    h.max(w)
}

#[inline]
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// --- Pointwise (White Balance → Exposure → Brightness → Contrast), theo từng điểm ---------

fn exposure_ch(x: f32, factor: f32) -> f32 {
    let xx = x.max(0.0);
    let lin = if xx <= 0.04045 {
        xx / 12.92
    } else {
        ((xx + 0.055) / 1.055).powf(2.4)
    };
    let lin = lin * factor;
    if lin <= 0.0031308 {
        12.92 * lin
    } else {
        1.055 * lin.max(0.0).powf(1.0 / 2.4) - 0.055
    }
}

fn pointwise_active(s: &AdjustmentSettings) -> bool {
    s.temperature != 0.0
        || s.tint != 0.0
        || s.exposure != 0.0
        || s.brightness != 0.0
        || s.contrast != 0.0
}

/// White Balance (+clip) → Exposure → Brightness → Contrast cho một bộ ba RGB.
fn pointwise_value(rgb: [f32; 3], s: &AdjustmentSettings) -> [f32; 3] {
    let mut v = rgb;
    if s.temperature != 0.0 || s.tint != 0.0 {
        if s.temperature != 0.0 {
            let t = s.temperature;
            let g = [
                (1.0 + 0.15 * t / 100.0) as f32,
                1.0,
                (1.0 - 0.15 * t / 100.0) as f32,
            ];
            v = [v[0] * g[0], v[1] * g[1], v[2] * g[2]];
        }
        if s.tint != 0.0 {
            let m = s.tint;
            let g1 = (1.0 - 0.12 * m / 100.0) as f32;
            v = [v[0], v[1] * g1, v[2]];
        }
        for x in &mut v {
            *x = x.clamp(0.0, 1.0);
        }
    }
    if s.exposure != 0.0 {
        let f = 2.0f64.powf(s.exposure) as f32;
        v = [exposure_ch(v[0], f), exposure_ch(v[1], f), exposure_ch(v[2], f)];
    }
    if s.brightness != 0.0 {
        let e = 2.0f64.powf(-s.brightness / 100.0) as f32;
        for x in &mut v {
            *x = x.max(0.0).powf(e);
        }
    }
    if s.contrast != 0.0 {
        let f = 2.0f64.powf(s.contrast / 100.0) as f32;
        for x in &mut v {
            *x = (*x - 0.5) * f + 0.5;
        }
    }
    v
}

/// Bảng tra `(3, levels)` của pointwise tại mọi code đầu vào.
fn pointwise_lut(s: &AdjustmentSettings, levels: usize) -> [Vec<f32>; 3] {
    let mut lut = [vec![0.0f32; levels], vec![0.0f32; levels], vec![0.0f32; levels]];
    let denom = (levels - 1) as f32;
    for i in 0..levels {
        let g = i as f32 / denom;
        let out = pointwise_value([g, g, g], s);
        for c in 0..3 {
            lut[c][i] = out[c];
        }
    }
    lut
}

/// (H, W, 3) uint8 → planar float32 (3, H, W) với pointwise áp dụng qua LUT 256 mức.
pub fn to_planar_u8(img: &ImageU8, s: &AdjustmentSettings) -> Planar {
    let lut = pointwise_lut(s, 256);
    let mut out = Planar::new(img.h, img.w);
    let n = img.h * img.w;
    for i in 0..n {
        for c in 0..3 {
            out.p[c][i] = lut[c][img.data[i * 3 + c] as usize];
        }
    }
    out
}

/// (H, W, 3) uint16 → planar float32 với pointwise áp dụng qua LUT 65536 mức.
pub fn to_planar_u16(img: &ImageU16, s: &AdjustmentSettings) -> Planar {
    let lut = pointwise_lut(s, 65536);
    let mut out = Planar::new(img.h, img.w);
    let n = img.h * img.w;
    for i in 0..n {
        for c in 0..3 {
            out.p[c][i] = lut[c][img.data[i * 3 + c] as usize];
        }
    }
    out
}

/// (H, W, 3) float32 → planar float32; pointwise áp dụng trực tiếp nếu có bật.
pub fn to_planar_f32(img: &ImageF32, s: &AdjustmentSettings) -> Planar {
    let mut out = Planar::new(img.h, img.w);
    let n = img.h * img.w;
    let active = pointwise_active(s);
    for i in 0..n {
        let rgb = [img.data[i * 3], img.data[i * 3 + 1], img.data[i * 3 + 2]];
        let v = if active { pointwise_value(rgb, s) } else { rgb };
        for c in 0..3 {
            out.p[c][i] = v[c];
        }
    }
    out
}

// --- Chuyển đổi planar <-> HWC / uint8 ----------------------------------------------------

pub fn planar_to_hwc(p: &Planar) -> ImageF32 {
    let n = p.h * p.w;
    let mut data = vec![0.0f32; n * 3];
    for i in 0..n {
        for c in 0..3 {
            data[i * 3 + c] = p.p[c][i];
        }
    }
    ImageF32 { h: p.h, w: p.w, data }
}

pub fn planar_to_uint8(p: &Planar) -> ImageU8 {
    let n = p.h * p.w;
    let mut data = vec![0u8; n * 3];
    for i in 0..n {
        for c in 0..3 {
            let v = (p.p[c][i].clamp(0.0, 1.0) * 255.0).round_ties_even();
            data[i * 3 + c] = v as u8;
        }
    }
    ImageU8 { h: p.h, w: p.w, data }
}

fn lum_planar(p: &Planar) -> Vec<f32> {
    let n = p.h * p.w;
    let mut lum = vec![0.0f32; n];
    for i in 0..n {
        lum[i] = p.p[0][i] * LUMA[0] + p.p[1][i] * LUMA[1] + p.p[2][i] * LUMA[2];
    }
    lum
}

fn gain_from(lum: &[f32], lum_new: &[f32]) -> Vec<f32> {
    lum.iter()
        .zip(lum_new)
        .map(|(&l, &ln)| (ln / l.max(1e-4)).clamp(0.0, GAIN_MAX))
        .collect()
}

fn scale_planes(p: &mut Planar, gain: &[f32]) {
    for c in 0..3 {
        for i in 0..gain.len() {
            p.p[c][i] *= gain[i];
        }
    }
}

fn clip_planar(p: &mut Planar) {
    for c in 0..3 {
        for v in &mut p.p[c] {
            *v = v.clamp(0.0, 1.0);
        }
    }
}

// --- Nhóm 2: Highlights + Shadows → Whites → Blacks → clip --------------------------------

fn hs_delta_plane(lb: &Plane, h: f64, s: f64) -> Plane {
    let mut out = Plane::new(lb.h, lb.w);
    let ks = 0.4 * (s / 100.0);
    let kh = 0.4 * (h / 100.0);
    for (o, &v) in out.data.iter_mut().zip(lb.data.iter()) {
        let mut d = 0.0f32;
        if s != 0.0 {
            // k * (1 - smoothstep(0, 0.5, lb))
            d += (ks as f32) * (1.0 - smoothstep(0.0, 0.5, v));
        }
        if h != 0.0 {
            d += (kh as f32) * smoothstep(0.5, 1.0, v);
        }
        *o = d;
    }
    out
}

pub fn tone_local(p: &mut Planar, s: &AdjustmentSettings, ls: usize) {
    let mut scale = 1.0f64;
    let mut offset = 0.0f64;
    if s.whites != 0.0 {
        scale /= 1.0 - 0.2 * s.whites / 100.0;
    }
    if s.blacks != 0.0 {
        let bp = -0.1 * s.blacks / 100.0;
        scale /= 1.0 - bp;
        offset = -bp / (1.0 - bp);
    }
    if s.highlights != 0.0 || s.shadows != 0.0 {
        let lum = lum_planar(p);
        let lum_plane = Plane::from_vec(p.h, p.w, lum.clone());
        let h = s.highlights;
        let sh = s.shadows;
        let delta = backend::blur_map(&lum_plane, 0.01 * ls as f64, |lb| hs_delta_plane(lb, h, sh));
        let lum_new: Vec<f32> = delta
            .data
            .iter()
            .zip(lum.iter())
            .map(|(&d, &l)| d + l)
            .collect();
        let mut gain = gain_from(&lum, &lum_new);
        if scale != 1.0 {
            let sf = scale as f32;
            for g in &mut gain {
                *g *= sf;
            }
        }
        scale_planes(p, &gain);
    } else if scale != 1.0 {
        let sf = scale as f32;
        for c in 0..3 {
            for v in &mut p.p[c] {
                *v *= sf;
            }
        }
    }
    if offset != 0.0 {
        let of = offset as f32;
        for c in 0..3 {
            for v in &mut p.p[c] {
                *v += of;
            }
        }
    }
    clip_planar(p);
}

// --- Nhóm 3: Vibrance → Saturation → clip -------------------------------------------------

pub fn color(p: &mut Planar, s: &AdjustmentSettings) {
    let lum = lum_planar(p);
    let fs = (1.0 + s.saturation / 100.0) as f32;
    let n = p.h * p.w;
    if s.vibrance != 0.0 {
        let k = (s.vibrance / 100.0) as f32;
        for i in 0..n {
            let mx = p.p[0][i].max(p.p[1][i]).max(p.p[2][i]);
            let mn = p.p[0][i].min(p.p[1][i]).min(p.p[2][i]);
            let sat = (mx - mn) / mx.max(1e-4); // = -mn trong bản Python (dấu)
            let factor = fs * (1.0 + k) - fs * k * sat;
            let l = lum[i];
            for c in 0..3 {
                p.p[c][i] = (p.p[c][i] - l) * factor + l;
            }
        }
    } else {
        for i in 0..n {
            let l = lum[i];
            for c in 0..3 {
                p.p[c][i] = (p.p[c][i] - l) * fs + l;
            }
        }
    }
    clip_planar(p);
}

// --- Nhóm 4: Noise Reduction → Clarity → Sharpening → clip -------------------------------

fn nr_params(n: f64, ls: usize) -> (usize, f32) {
    let r = ((0.002 * ls as f64).round() as usize).max(1);
    let eps = (0.03 * n / 100.0).powi(2) as f32;
    (r, eps)
}

/// Guided filter của mỗi source với một guide. Khớp `core.adjustments.guided_filter`.
pub fn guided_filter(guide: &Plane, sources: &[Plane], r: usize, eps: f32) -> Vec<Plane> {
    let (h, w) = (guide.h, guide.w);
    let f = (r / NR_SUBSAMPLE_RADIUS).max(1);
    let (down, r): (Box<dyn Fn(&Plane) -> Plane>, usize) = if f > 1 {
        (Box::new(move |a: &Plane| backend::downscale(a, f)), ((r as f64 / f as f64).round() as usize).max(1))
    } else {
        (Box::new(|a: &Plane| a.clone()), r)
    };
    let mul = |a: &Plane, b: &Plane| -> Plane {
        Plane::from_vec(
            a.h,
            a.w,
            a.data.iter().zip(&b.data).map(|(&x, &y)| x * y).collect(),
        )
    };
    let m_i = backend::box_filter(&down(guide), r);
    let gg = mul(guide, guide);
    let var = {
        let bx = backend::box_filter(&down(&gg), r);
        Plane::from_vec(
            bx.h,
            bx.w,
            bx.data
                .iter()
                .zip(&m_i.data)
                .map(|(&b, &mi)| b - mi * mi)
                .collect(),
        )
    };
    let mut out = Vec::with_capacity(sources.len());
    for src in sources {
        let m_p = backend::box_filter(&down(src), r);
        let gp = mul(guide, src);
        let box_gp = backend::box_filter(&down(&gp), r);
        let a = Plane::from_vec(
            m_i.h,
            m_i.w,
            (0..m_i.data.len())
                .map(|i| (box_gp.data[i] - m_i.data[i] * m_p.data[i]) / (var.data[i] + eps))
                .collect(),
        );
        let b = Plane::from_vec(
            m_i.h,
            m_i.w,
            (0..m_i.data.len())
                .map(|i| m_p.data[i] - a.data[i] * m_i.data[i])
                .collect(),
        );
        let mut a_m = backend::box_filter(&a, r);
        let mut b_m = backend::box_filter(&b, r);
        if f > 1 {
            a_m = backend::upscale(&a_m, h, w, f);
            b_m = backend::upscale(&b_m, h, w, f);
        }
        out.push(Plane::from_vec(
            h,
            w,
            (0..h * w)
                .map(|i| a_m.data[i] * guide.data[i] + b_m.data[i])
                .collect(),
        ));
    }
    out
}

fn noise_reduction_planar(p: &mut Planar, n: f64, ls: usize) {
    let (r, eps) = nr_params(n, ls);
    let lum = lum_planar(p);
    let lum_plane = Plane::from_vec(p.h, p.w, lum.clone());
    let lum_dn = guided_filter(&lum_plane, std::slice::from_ref(&lum_plane), r, eps)
        .into_iter()
        .next()
        .unwrap();
    let chroma: Vec<Plane> = (0..3)
        .map(|c| {
            Plane::from_vec(
                p.h,
                p.w,
                (0..p.h * p.w).map(|i| p.p[c][i] - lum[i]).collect(),
            )
        })
        .collect();
    let chroma_dn = guided_filter(&lum_plane, &chroma, 2 * r, 4.0 * eps);
    for c in 0..3 {
        for i in 0..p.h * p.w {
            p.p[c][i] = chroma_dn[c].data[i] + lum_dn.data[i];
        }
    }
}

fn clarity_gain(lum: &Plane, c: f64, ls: usize) -> Vec<f32> {
    let blur = backend::gaussian_blur(lum, 0.015 * ls as f64);
    let k = (0.8 * c / 100.0) as f32;
    let detail: Vec<f32> = (0..lum.data.len())
        .map(|i| {
            let d = lum.data[i] - blur.data[i];
            let mid = {
                let m = 2.0 * lum.data[i] - 1.0;
                1.0 - m * m
            };
            d * mid * k + lum.data[i]
        })
        .collect();
    gain_from(&lum.data, &detail)
}

fn sharpen_gain(lum: &Plane, a: f64, ls: usize) -> Vec<f32> {
    let sigma = (ls as f64 / 4000.0).max(0.5);
    let blur = backend::gaussian_blur(lum, sigma);
    let amt = (a / 100.0) as f32;
    let sharp: Vec<f32> = (0..lum.data.len())
        .map(|i| (lum.data[i] - blur.data[i]) * amt + lum.data[i])
        .collect();
    gain_from(&lum.data, &sharp)
}

pub fn detail(p: &mut Planar, s: &AdjustmentSettings, ls: usize) {
    if s.noise_reduction != 0.0 {
        noise_reduction_planar(p, s.noise_reduction, ls);
    }
    if s.clarity != 0.0 || s.sharpening_amount != 0.0 {
        let mut lum = lum_planar(p);
        let mut gain: Option<Vec<f32>> = None;
        if s.clarity != 0.0 {
            let g = clarity_gain(&Plane::from_vec(p.h, p.w, lum.clone()), s.clarity, ls);
            for i in 0..lum.len() {
                lum[i] *= g[i];
            }
            gain = Some(g);
        }
        if s.sharpening_amount != 0.0 {
            let g2 = sharpen_gain(&Plane::from_vec(p.h, p.w, lum.clone()), s.sharpening_amount, ls);
            gain = Some(match gain {
                None => g2,
                Some(g) => g.iter().zip(&g2).map(|(&a, &b)| a * b).collect(),
            });
        }
        scale_planes(p, &gain.unwrap());
    }
    clip_planar(p);
}

// --- Nhóm 5: Post-Crop Vignetting → clip --------------------------------------------------

fn vignette_gain_grid(h: usize, w: usize, v: f64, f: usize) -> Plane {
    let gh = h.div_ceil(f);
    let gw = w.div_ceil(f);
    let mut out = Plane::new(gh, gw);
    let amt = (0.8 * v / 100.0) as f32;
    let off = (f as f64 - 1.0) / 2.0;
    let ys: Vec<f32> = (0..gh)
        .map(|iy| (((iy * f) as f64 + off + 0.5) / h as f64) as f32 * 2.0 - 1.0)
        .collect();
    let xs: Vec<f32> = (0..gw)
        .map(|ix| (((ix * f) as f64 + off + 0.5) / w as f64) as f32 * 2.0 - 1.0)
        .collect();
    let inv_sqrt2 = (2.0f32).powf(-0.5);
    for iy in 0..gh {
        for ix in 0..gw {
            let r = (ys[iy] * ys[iy] + xs[ix] * xs[ix]).sqrt() * inv_sqrt2;
            out.data[iy * gw + ix] = 1.0 + amt * smoothstep(0.35, 1.0, r);
        }
    }
    out
}

pub fn effects(p: &mut Planar, s: &AdjustmentSettings) {
    let (h, w) = (p.h, p.w);
    let f = (VIGNETTE_GRID.min(h.min(w) / 400)).max(1);
    let mut gain = vignette_gain_grid(h, w, s.vignette_amount, f);
    if f > 1 {
        gain = backend::upscale(&gain, h, w, f);
    }
    scale_planes(p, &gain.data);
    clip_planar(p);
}

// --- Pipeline chỉnh sửa (section 6) -------------------------------------------------------

fn apply_groups(mut p: Planar, s: &AdjustmentSettings, ls: usize) -> Planar {
    if s.exposure != 0.0
        || s.brightness != 0.0
        || s.contrast != 0.0
        || s.highlights != 0.0
        || s.shadows != 0.0
        || s.whites != 0.0
        || s.blacks != 0.0
    {
        tone_local(&mut p, s, ls);
    }
    if s.vibrance != 0.0 || s.saturation != 0.0 {
        color(&mut p, s);
    }
    if s.noise_reduction != 0.0 || s.clarity != 0.0 || s.sharpening_amount != 0.0 {
        detail(&mut p, s, ls);
    }
    if s.vignette_amount != 0.0 {
        effects(&mut p, s);
    }
    p
}

pub fn apply_adjustments_planar_u8(img: &ImageU8, s: &AdjustmentSettings) -> Planar {
    apply_adjustments_planar_u8_ls(img, s, None)
}

/// Như [`apply_adjustments_planar_u8`] nhưng `long_side` ghi đè kích thước dùng cho các tham số
/// theo không gian (bán kính sharpening, clarity…). Dùng khi `img` là bản xem trước thu nhỏ của
/// một ảnh lớn, để hiệu ứng trông giống nhau ở mọi cỡ xem trước (giống tham số `long_side` của
/// `core.pipeline.apply_adjustments`).
pub fn apply_adjustments_planar_u8_ls(
    img: &ImageU8,
    s: &AdjustmentSettings,
    long_side: Option<usize>,
) -> Planar {
    let ls = long_side.unwrap_or_else(|| long_side_of(img.h, img.w));
    apply_groups(to_planar_u8(img, s), s, ls)
}

pub fn apply_adjustments_planar_u16(img: &ImageU16, s: &AdjustmentSettings) -> Planar {
    let ls = long_side_of(img.h, img.w);
    apply_groups(to_planar_u16(img, s), s, ls)
}

pub fn apply_adjustments_planar_f32(img: &ImageF32, s: &AdjustmentSettings) -> Planar {
    let ls = long_side_of(img.h, img.w);
    apply_groups(to_planar_f32(img, s), s, ls)
}

pub fn apply_adjustments_u8(img: &ImageU8, s: &AdjustmentSettings) -> ImageF32 {
    planar_to_hwc(&apply_adjustments_planar_u8(img, s))
}

pub fn apply_adjustments_f32(img: &ImageF32, s: &AdjustmentSettings) -> ImageF32 {
    planar_to_hwc(&apply_adjustments_planar_f32(img, s))
}
