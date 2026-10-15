//! Tách nền (xoá phông / thay nền) — phần không phụ thuộc ONNX: chuẩn bị đầu vào cho model
//! MODNet, phóng matte về cỡ ảnh, tinh chỉnh viền, khử màu nền ở viền, vẽ nền thay thế.
//! Chạy model: [`crate::bg_remove`] (feature `bgremove`). Xem `docs/instruction/background-removal.md`.
//!
//! Matte được giữ **riêng** với ảnh nền (không ghi vào alpha của ảnh gốc) nên luôn chỉnh viền,
//! đổi nền hay bỏ tách nền được; nó đi theo ảnh khi crop / xoay 90° / lật.

use std::sync::Arc;

use super::RgbaImage;
use crate::settings::SliderSpec;

/// Cạnh ngắn của ảnh đưa vào model (MODNet được huấn luyện ở 512).
pub const MODEL_SHORT_SIDE: usize = 512;
/// Giới hạn cạnh dài (ảnh rất dài / panorama) để model không quá chậm.
pub const MODEL_LONG_MAX: usize = 1024;
/// Hai cạnh đầu vào phải chia hết cho 32.
pub const MODEL_DIVISOR: usize = 32;

/// Màu nền gợi ý (ảnh thẻ): (nhãn, RGB).
pub const BACKDROP_COLORS: [(&str, [u8; 3]); 5] = [
    ("White", [255, 255, 255]),
    ("Blue", [0x1e, 0x6f, 0xd9]),
    ("Light blue", [0xd6, 0xe8, 0xf7]),
    ("Red", [0xd3, 0x2f, 0x2f]),
    ("Gray", [0xc8, 0xc8, 0xc8]),
];

const fn cs(
    key: &'static str,
    label: &'static str,
    minimum: f64,
    maximum: f64,
    step: f64,
    decimals: usize,
) -> SliderSpec {
    SliderSpec {
        key,
        label,
        panel: "Background removal",
        group: "Background removal",
        minimum,
        maximum,
        step,
        default: 0.0,
        decimals,
    }
}

/// 2 thanh trượt tinh chỉnh viền (px ảnh gốc): Edge (âm = co vào, dương = giãn ra), Feather.
pub const CUTOUT_SLIDERS: [SliderSpec; 2] = [
    cs("edge", "Shift edge", -10.0, 10.0, 0.5, 1),
    cs("feather", "Feather", 0.0, 10.0, 0.5, 1),
];

/// Matte 1 kênh (0 = nền, 255 = người), cùng cỡ ảnh nền. `Arc` để ảnh chụp undo rẻ.
#[derive(Debug, Clone, PartialEq)]
pub struct Matte {
    pub h: usize,
    pub w: usize,
    pub data: Arc<Vec<u8>>,
}

impl Matte {
    pub fn new(h: usize, w: usize, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), h * w, "Matte: độ dài data không khớp h*w");
        Matte {
            h,
            w,
            data: Arc::new(data),
        }
    }

    /// Cắt giống [`RgbaImage::crop`] (tự kẹp vào trong).
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Matte {
        let (x0, y0) = (x.min(self.w), y.min(self.h));
        let (x1, y1) = ((x + w).min(self.w), (y + h).min(self.h));
        let (cw, ch) = (x1 - x0, y1 - y0);
        let mut out = Vec::with_capacity(cw * ch);
        for yy in y0..y1 {
            out.extend_from_slice(&self.data[yy * self.w + x0..yy * self.w + x1]);
        }
        Matte::new(ch, cw, out)
    }

    /// Xoay 90° — cùng phép biến đổi với `rotate90_image` của photo editor.
    pub fn rotate90(&self, clockwise: bool) -> Matte {
        let (h, w) = (self.h, self.w);
        let mut out = vec![0u8; h * w];
        // ảnh ra: rộng h, cao w
        for y in 0..h {
            for x in 0..w {
                let (nx, ny) = if clockwise {
                    (h - 1 - y, x)
                } else {
                    (y, w - 1 - x)
                };
                out[ny * h + nx] = self.data[y * w + x];
            }
        }
        Matte::new(w, h, out)
    }

    pub fn flip(&self, horizontal: bool) -> Matte {
        let (h, w) = (self.h, self.w);
        let mut out = vec![0u8; h * w];
        for y in 0..h {
            for x in 0..w {
                let (ny, nx) = if horizontal {
                    (y, w - 1 - x)
                } else {
                    (h - 1 - y, x)
                };
                out[ny * w + nx] = self.data[y * w + x];
            }
        }
        Matte::new(h, w, out)
    }

    /// Matte ở cỡ `h × w`, giá trị 0…1.
    pub fn resized(&self, h: usize, w: usize) -> Vec<f32> {
        let src: Vec<f32> = self.data.iter().map(|&v| v as f32 / 255.0).collect();
        resize_plane(&src, self.h, self.w, h, w)
    }
}

/// Tinh chỉnh viền. Đơn vị px của ảnh gốc (xem trước thu nhỏ thì bán kính thu theo).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutoutSettings {
    /// Âm = co viền vào trong (bỏ bớt nền còn sót), dương = giãn ra.
    pub edge: f64,
    /// Làm mềm viền (bán kính mờ).
    pub feather: f64,
    /// Khử màu phông cũ còn ám ở viền / tóc (xem [`decontaminate`]).
    pub decontaminate: bool,
}

impl Default for CutoutSettings {
    fn default() -> Self {
        CutoutSettings {
            edge: 0.0,
            feather: 0.0,
            decontaminate: true,
        }
    }
}

impl CutoutSettings {
    pub fn get(&self, key: &str) -> Option<f64> {
        Some(match key {
            "edge" => self.edge,
            "feather" => self.feather,
            _ => return None,
        })
    }

    /// Đặt theo khoá, kẹp vào khoảng của [`CUTOUT_SLIDERS`].
    pub fn set(&mut self, key: &str, value: f64) -> bool {
        let Some(spec) = CUTOUT_SLIDERS.iter().find(|s| s.key == key) else {
            return false;
        };
        let v = value.clamp(spec.minimum, spec.maximum);
        match key {
            "edge" => self.edge = v,
            "feather" => self.feather = v,
            _ => return false,
        }
        true
    }
}

/// Nền thay cho phông cũ.
#[derive(Debug, Clone, PartialEq)]
pub enum Backdrop {
    Transparent,
    Color([u8; 3]),
    /// Ảnh nền mới, phủ kín khung (cover) và canh giữa.
    Image(Arc<RgbaImage>),
}

/// Kết quả tách nền gắn vào ảnh nền của photo editor.
#[derive(Debug, Clone, PartialEq)]
pub struct Cutout {
    pub matte: Matte,
    pub settings: CutoutSettings,
    pub backdrop: Backdrop,
}

impl Cutout {
    pub fn new(matte: Matte) -> Self {
        Cutout {
            matte,
            settings: CutoutSettings::default(),
            backdrop: Backdrop::Transparent,
        }
    }
}

// --- Đầu vào / đầu ra model ---------------------------------------------------------------

/// Cỡ `(w, h)` đưa vào model: cạnh ngắn 512 (cạnh dài ≤ 1024), làm tròn tới bội của 32.
pub fn model_input_size(w: usize, h: usize) -> (usize, usize) {
    let (w, h) = (w.max(1) as f64, h.max(1) as f64);
    let k = (MODEL_SHORT_SIDE as f64 / w.min(h)).min(MODEL_LONG_MAX as f64 / w.max(h));
    let round = |v: f64| {
        let d = MODEL_DIVISOR as f64;
        (((v * k) / d).round() as usize).max(1) * MODEL_DIVISOR
    };
    (round(w), round(h))
}

/// Tensor NCHW `[1, 3, mh, mw]` chuẩn hoá `(x/255 − 0.5) / 0.5`. Pixel trong suốt được ghép lên
/// nền trắng trước.
pub fn preprocess(img: &RgbaImage, mw: usize, mh: usize) -> Vec<f32> {
    let n = img.h * img.w;
    let mut planes = [vec![0f32; n], vec![0f32; n], vec![0f32; n]];
    for i in 0..n {
        let a = img.data[i * 4 + 3] as f32 / 255.0;
        for (c, plane) in planes.iter_mut().enumerate() {
            plane[i] = (img.data[i * 4 + c] as f32 * a + 255.0 * (1.0 - a)) / 255.0;
        }
    }
    let mut out = Vec::with_capacity(3 * mh * mw);
    for plane in &planes {
        let small = resize_plane(plane, img.h, img.w, mh, mw);
        out.extend(small.iter().map(|v| (v - 0.5) / 0.5));
    }
    out
}

/// Phóng matte của model (`mh × mw`, 0…1) lên cỡ ảnh. Sau khi phóng song tuyến, chạy guided
/// filter lấy độ sáng của ảnh gốc làm "guide" để viền (tóc, vai) bám theo cạnh thật của ảnh
/// thay vì bị nhoè như khi chỉ phóng to.
pub fn matte_from_model(img: &RgbaImage, low: &[f32], mh: usize, mw: usize) -> Matte {
    let (h, w) = (img.h, img.w);
    let up = resize_plane(low, mh, mw, h, w);
    let factor = (h as f64 / mh as f64).max(w as f64 / mw as f64);
    let alpha = if factor > 1.5 {
        let guide: Vec<f32> = (0..h * w)
            .map(|i| {
                let p = &img.data[i * 4..i * 4 + 3];
                (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0
            })
            .collect();
        let r = ((factor * 1.5).round() as usize).max(2);
        let refined = guided_filter(&guide, &up, h, w, r, 1e-3);
        // Chỉ dùng kết quả guided ở dải viền chưa chắc chắn; vùng chắc chắn là người / nền giữ
        // nguyên để không bị "rỉ" sang nền có hoạ tiết.
        let band = band_mask(&up, h, w, r);
        up.iter()
            .zip(&refined)
            .zip(&band)
            .map(|((&u, &g), &b)| if b { g } else { u })
            .collect()
    } else {
        up
    };
    Matte::new(
        h,
        w,
        alpha
            .iter()
            .map(|&a| (a.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect(),
    )
}

/// `true` ở những pixel cách vùng chưa chắc chắn (0.02 < a < 0.98) không quá `r` px.
fn band_mask(a: &[f32], h: usize, w: usize, r: usize) -> Vec<bool> {
    let unsure: Vec<f32> = a
        .iter()
        .map(|&v| if v > 0.02 && v < 0.98 { 1.0 } else { 0.0 })
        .collect();
    max_filter(&unsure, h, w, r)
        .iter()
        .map(|&v| v > 0.0)
        .collect()
}

// --- Áp matte, khử màu viền, nền thay thế -------------------------------------------------

/// Alpha 0…1 cỡ `h × w` sau khi co / giãn và làm mềm viền. `scale` = tỉ lệ ảnh đang vẽ so với
/// ảnh gốc.
pub fn refined_alpha(
    matte: &Matte,
    h: usize,
    w: usize,
    s: &CutoutSettings,
    scale: f64,
) -> Vec<f32> {
    let mut a = matte.resized(h, w);
    let r = (s.edge.abs() * scale).round() as usize;
    if r > 0 {
        a = if s.edge > 0.0 {
            max_filter(&a, h, w, r)
        } else {
            min_filter(&a, h, w, r)
        };
    }
    let sigma = s.feather * scale;
    if sigma >= 0.3 {
        a = approx_gaussian(&a, h, w, sigma);
    }
    a
}

/// Áp tách nền lên `img` (đã chỉnh màu, ở tỉ lệ `scale`): alpha = alpha cũ × matte.
pub fn apply_cutout(img: &mut RgbaImage, cutout: &Cutout, scale: f64) {
    if img.is_empty() {
        return;
    }
    let alpha = refined_alpha(&cutout.matte, img.h, img.w, &cutout.settings, scale);
    if cutout.settings.decontaminate {
        decontaminate(img, &alpha);
    }
    for (i, &a) in alpha.iter().enumerate() {
        let old = img.data[i * 4 + 3] as f32;
        img.data[i * 4 + 3] = (old * a.clamp(0.0, 1.0)).round() as u8;
    }
}

/// Khử màu phông cũ ở viền bán trong suốt (tóc, mép áo): ước lượng màu nền `B` quanh mỗi pixel
/// (trung bình các pixel nền gần đó) rồi tách màu người `F` từ `I = a·F + (1 − a)·B`.
/// Ghép lên nền mới `N` cho `I + (1 − a)(N − B)` — tức là thay đúng phần màu của phông cũ,
/// không còn viền ám xanh / hồng quanh tóc.
pub fn decontaminate(img: &mut RgbaImage, alpha: &[f32]) {
    let (h, w) = (img.h, img.w);
    let n = h * w;
    // Chỉ lấy pixel chắc chắn là nền; quanh tóc dày có thể không có → tìm ở bán kính rộng hơn.
    let wb: Vec<f32> = alpha
        .iter()
        .map(|&a| if a < 0.1 { 1.0 } else { 0.0 })
        .collect();
    let r0 = (img.long_side() / 60).max(2);
    let mut bg = [vec![-1f32; n], vec![-1f32; n], vec![-1f32; n]];
    for r in [r0, r0 * 4] {
        let wsum = box_mean(&wb, h, w, r);
        for (c, plane) in bg.iter_mut().enumerate() {
            let weighted: Vec<f32> = (0..n).map(|i| img.data[i * 4 + c] as f32 * wb[i]).collect();
            let blurred = box_mean(&weighted, h, w, r);
            for i in 0..n {
                if plane[i] < 0.0 && wsum[i] > 1e-4 {
                    plane[i] = blurred[i] / wsum[i];
                }
            }
        }
    }
    for i in 0..n {
        let a = alpha[i];
        if !(0.01..0.995).contains(&a) || bg[0][i] < 0.0 {
            continue;
        }
        for (c, plane) in bg.iter().enumerate() {
            let v = img.data[i * 4 + c] as f32;
            let f = (v - (1.0 - a) * plane[i]) / a;
            img.data[i * 4 + c] = f.round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Vẽ `person` (đã có alpha) lên nền thay thế.
pub fn composite_backdrop(person: &RgbaImage, backdrop: &Backdrop) -> RgbaImage {
    let mut base = match backdrop {
        Backdrop::Transparent => return person.clone(),
        Backdrop::Color(c) => RgbaImage::filled(person.h, person.w, [c[0], c[1], c[2], 255]),
        Backdrop::Image(img) => cover_resize(img, person.h, person.w),
    };
    for i in 0..person.h * person.w {
        let s = &person.data[i * 4..i * 4 + 4];
        let a = s[3] as f32 / 255.0;
        if a <= 0.0 {
            continue;
        }
        let d = &mut base.data[i * 4..i * 4 + 4];
        let da = d[3] as f32 / 255.0;
        let oa = a + da * (1.0 - a);
        for c in 0..3 {
            let v = (s[c] as f32 * a + d[c] as f32 * da * (1.0 - a)) / oa.max(1e-6);
            d[c] = v.round().clamp(0.0, 255.0) as u8;
        }
        d[3] = (oa * 255.0).round() as u8;
    }
    base
}

/// Phóng `img` để phủ kín `h × w` (giữ tỉ lệ, cắt phần thừa ở giữa).
pub fn cover_resize(img: &RgbaImage, h: usize, w: usize) -> RgbaImage {
    if img.is_empty() || h == 0 || w == 0 {
        return RgbaImage::new(h, w);
    }
    let k = (w as f64 / img.w as f64).max(h as f64 / img.h as f64);
    let (cw, ch) = (
        ((w as f64 / k).round() as usize).clamp(1, img.w),
        ((h as f64 / k).round() as usize).clamp(1, img.h),
    );
    let cropped = img.crop((img.w - cw) / 2, (img.h - ch) / 2, cw, ch);
    let n = cropped.h * cropped.w;
    let mut out = RgbaImage::new(h, w);
    for c in 0..4 {
        let plane: Vec<f32> = (0..n).map(|i| cropped.data[i * 4 + c] as f32).collect();
        let res = resize_plane(&plane, cropped.h, cropped.w, h, w);
        for (i, v) in res.iter().enumerate() {
            out.data[i * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

// --- Bộ lọc 1 kênh ------------------------------------------------------------------------

/// Đổi cỡ một kênh: thu nhỏ = trung bình vùng (không răng cưa), phóng to = song tuyến.
pub fn resize_plane(src: &[f32], h: usize, w: usize, nh: usize, nw: usize) -> Vec<f32> {
    if (h, w) == (nh, nw) {
        return src.to_vec();
    }
    if h == 0 || w == 0 || nh == 0 || nw == 0 {
        return vec![0.0; nh * nw];
    }
    // Từng chiều riêng: ngang rồi dọc.
    let tmp = resize_axis(src, h, w, nw, false);
    resize_axis(&tmp, h, nw, nh, true)
}

/// Đổi cỡ theo một trục. `vertical = false`: `h × w` → `h × n`; `true`: `h × w` → `n × w`.
fn resize_axis(src: &[f32], h: usize, w: usize, n: usize, vertical: bool) -> Vec<f32> {
    let len = if vertical { h } else { w };
    let lines = if vertical { w } else { h };
    let at = |line: usize, i: usize| -> usize {
        if vertical {
            i * w + line
        } else {
            line * w + i
        }
    };
    let (oh, ow) = if vertical { (n, w) } else { (h, n) };
    let mut out = vec![0f32; oh * ow];
    let put = |line: usize, i: usize| -> usize {
        if vertical {
            i * ow + line
        } else {
            line * ow + i
        }
    };
    let ratio = len as f64 / n as f64;
    if n < len {
        // trung bình vùng, có trọng số phần pixel bị cắt ở 2 mép
        for line in 0..lines {
            for o in 0..n {
                let (s0, s1) = (o as f64 * ratio, (o + 1) as f64 * ratio);
                let mut acc = 0.0f64;
                let mut i = s0.floor() as usize;
                while (i as f64) < s1 && i < len {
                    let cover = (s1.min(i as f64 + 1.0) - s0.max(i as f64)).max(0.0);
                    acc += src[at(line, i)] as f64 * cover;
                    i += 1;
                }
                out[put(line, o)] = (acc / ratio) as f32;
            }
        }
    } else {
        for line in 0..lines {
            for o in 0..n {
                let f = ((o as f64 + 0.5) * ratio - 0.5).clamp(0.0, (len - 1) as f64);
                let i0 = f.floor() as usize;
                let i1 = (i0 + 1).min(len - 1);
                let t = (f - i0 as f64) as f32;
                out[put(line, o)] = src[at(line, i0)] * (1.0 - t) + src[at(line, i1)] * t;
            }
        }
    }
    out
}

/// Trung bình trong cửa sổ `(2r+1)²` (ở mép chỉ tính phần nằm trong ảnh). O(n), không phụ
/// thuộc `r`.
pub fn box_mean(src: &[f32], h: usize, w: usize, r: usize) -> Vec<f32> {
    let pass = |src: &[f32], vertical: bool| -> Vec<f32> {
        let (len, lines) = if vertical { (h, w) } else { (w, h) };
        let idx = |line: usize, i: usize| if vertical { i * w + line } else { line * w + i };
        let mut out = vec![0f32; h * w];
        let mut prefix = vec![0f64; len + 1];
        for line in 0..lines {
            for i in 0..len {
                prefix[i + 1] = prefix[i] + src[idx(line, i)] as f64;
            }
            for i in 0..len {
                let a = i.saturating_sub(r);
                let b = (i + r + 1).min(len);
                out[idx(line, i)] = ((prefix[b] - prefix[a]) / (b - a) as f64) as f32;
            }
        }
        out
    };
    let tmp = pass(src, false);
    pass(&tmp, true)
}

/// Gần đúng Gaussian `sigma` bằng 3 lần box filter.
fn approx_gaussian(src: &[f32], h: usize, w: usize, sigma: f64) -> Vec<f32> {
    // bán kính box sao cho 3 lần box ≈ Gaussian sigma: var = 3·((2r+1)² − 1)/12
    let r = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0)
        .round()
        .max(1.0) as usize;
    let mut a = src.to_vec();
    for _ in 0..3 {
        a = box_mean(&a, h, w, r);
    }
    a
}

fn extreme_filter(src: &[f32], h: usize, w: usize, r: usize, take_max: bool) -> Vec<f32> {
    let pick = |a: f32, b: f32| if take_max { a.max(b) } else { a.min(b) };
    let mut tmp = vec![0f32; h * w];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            let row = &src[y * w..y * w + w];
            tmp[y * w + x] = row[a..=b].iter().copied().reduce(pick).unwrap_or(0.0);
        }
    }
    let mut out = vec![0f32; h * w];
    for y in 0..h {
        let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
        for x in 0..w {
            let mut v = tmp[a * w + x];
            for yy in a + 1..=b {
                v = pick(v, tmp[yy * w + x]);
            }
            out[y * w + x] = v;
        }
    }
    out
}

/// Giãn (lấy max trong cửa sổ vuông bán kính `r`).
pub fn max_filter(src: &[f32], h: usize, w: usize, r: usize) -> Vec<f32> {
    extreme_filter(src, h, w, r, true)
}

/// Co (lấy min trong cửa sổ vuông bán kính `r`).
pub fn min_filter(src: &[f32], h: usize, w: usize, r: usize) -> Vec<f32> {
    extreme_filter(src, h, w, r, false)
}

/// Guided filter (He et al.) một kênh guide: kết quả bám theo cạnh của `guide`.
pub fn guided_filter(
    guide: &[f32],
    src: &[f32],
    h: usize,
    w: usize,
    r: usize,
    eps: f32,
) -> Vec<f32> {
    let n = h * w;
    let mean_i = box_mean(guide, h, w, r);
    let mean_p = box_mean(src, h, w, r);
    let ip: Vec<f32> = (0..n).map(|i| guide[i] * src[i]).collect();
    let ii: Vec<f32> = guide.iter().map(|v| v * v).collect();
    let corr_ip = box_mean(&ip, h, w, r);
    let corr_ii = box_mean(&ii, h, w, r);
    let mut a = vec![0f32; n];
    let mut b = vec![0f32; n];
    for i in 0..n {
        let var = corr_ii[i] - mean_i[i] * mean_i[i];
        let cov = corr_ip[i] - mean_i[i] * mean_p[i];
        a[i] = cov / (var + eps);
        b[i] = mean_p[i] - a[i] * mean_i[i];
    }
    let mean_a = box_mean(&a, h, w, r);
    let mean_b = box_mean(&b, h, w, r);
    (0..n).map(|i| mean_a[i] * guide[i] + mean_b[i]).collect()
}

// --- Cọ sửa vùng chọn ---------------------------------------------------------------------

/// Tô một chấm cọ tròn vào matte `h × w` (u8) tại tâm `(cx, cy)` px, bán kính `radius`.
/// `hardness` 0…1: phần lõi đặc = `radius × hardness`, ra tới mép thì mờ dần (smoothstep).
/// `add = true`: thêm vào vùng giữ lại (matte → 255); `false`: bớt (matte → 0).
/// Trả về hình chữ nhật `(x0, y0, x1, y1)` đã chạm (rỗng nếu ngoài ảnh).
#[allow(clippy::too_many_arguments)]
pub fn brush_dab(
    data: &mut [u8],
    h: usize,
    w: usize,
    cx: f64,
    cy: f64,
    radius: f64,
    hardness: f64,
    add: bool,
) -> (usize, usize, usize, usize) {
    let r = radius.max(0.5);
    let inner = r * hardness.clamp(0.0, 1.0);
    let x0 = ((cx - r).floor().max(0.0) as usize).min(w);
    let y0 = ((cy - r).floor().max(0.0) as usize).min(h);
    let x1 = ((cx + r).ceil().max(0.0) as usize).min(w);
    let y1 = ((cy + r).ceil().max(0.0) as usize).min(h);
    for y in y0..y1 {
        let dy = y as f64 + 0.5 - cy;
        for x in x0..x1 {
            let dx = x as f64 + 0.5 - cx;
            let d = (dx * dx + dy * dy).sqrt();
            if d > r {
                continue;
            }
            let v = if d <= inner || r - inner < 1e-9 {
                1.0
            } else {
                let t = (r - d) / (r - inner);
                t * t * (3.0 - 2.0 * t)
            };
            let v = (v * 255.0).round() as u8;
            let px = &mut data[y * w + x];
            *px = if add {
                (*px).max(v)
            } else {
                (*px).min(255 - v)
            };
        }
    }
    (x0, y0, x1, y1)
}

/// Tô nét cọ từ `from` tới `to` (các chấm cách nhau ¼ bán kính) — kéo chuột nhanh không bị đứt.
/// `from = None`: chỉ một chấm ở `to`. Trả về hình chữ nhật bao đã chạm.
#[allow(clippy::too_many_arguments)]
pub fn brush_line(
    data: &mut [u8],
    h: usize,
    w: usize,
    from: Option<(f64, f64)>,
    to: (f64, f64),
    radius: f64,
    hardness: f64,
    add: bool,
) -> (usize, usize, usize, usize) {
    let start = from.unwrap_or(to);
    let len = ((to.0 - start.0).powi(2) + (to.1 - start.1).powi(2)).sqrt();
    let step = (radius * 0.25).max(0.5);
    let n = (len / step).ceil().max(1.0) as usize;
    let mut bb = (usize::MAX, usize::MAX, 0, 0);
    for i in 0..=n {
        if from.is_some() && i == 0 {
            continue; // chấm ở `from` đã tô ở lần trước
        }
        let t = i as f64 / n as f64;
        let (x, y) = (
            start.0 + (to.0 - start.0) * t,
            start.1 + (to.1 - start.1) * t,
        );
        let r = brush_dab(data, h, w, x, y, radius, hardness, add);
        if r.0 < r.2 && r.1 < r.3 {
            bb = (bb.0.min(r.0), bb.1.min(r.1), bb.2.max(r.2), bb.3.max(r.3));
        }
    }
    if bb.0 == usize::MAX {
        (0, 0, 0, 0)
    } else {
        bb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matte_of(h: usize, w: usize, f: impl Fn(usize, usize) -> u8) -> Matte {
        let mut d = vec![0u8; h * w];
        for y in 0..h {
            for x in 0..w {
                d[y * w + x] = f(y, x);
            }
        }
        Matte::new(h, w, d)
    }

    #[test]
    fn model_size_is_short_side_512_and_multiple_of_32() {
        assert_eq!(model_input_size(512, 512), (512, 512));
        assert_eq!(model_input_size(3000, 4000), (512, 672)); // 682.7 → 672
        assert_eq!(model_input_size(4000, 3000), (672, 512));
        // ảnh rất dài: cạnh dài bị giới hạn 1024
        let (w, h) = model_input_size(10000, 1000);
        assert_eq!(w, 1024);
        assert!(h % 32 == 0 && h >= 32);
        // ảnh nhỏ cũng được phóng lên
        assert_eq!(model_input_size(100, 100), (512, 512));
    }

    #[test]
    fn preprocess_normalizes_to_minus_one_one() {
        let img = RgbaImage::filled(10, 20, [255, 0, 128, 255]);
        let t = preprocess(&img, 64, 32);
        assert_eq!(t.len(), 3 * 32 * 64);
        assert!((t[0] - 1.0).abs() < 1e-6); // R
        assert!((t[32 * 64] + 1.0).abs() < 1e-6); // G
        assert!(t[2 * 32 * 64].abs() < 0.01); // B ≈ 0.5
                                              // trong suốt → trắng
        let clear = RgbaImage::new(4, 4);
        assert!(preprocess(&clear, 32, 32)
            .iter()
            .all(|v| (v - 1.0).abs() < 1e-6));
    }

    #[test]
    fn matte_geometry_matches_image_geometry() {
        let m = matte_of(2, 3, |y, x| (y * 3 + x) as u8);
        let img = RgbaImage::from_vec(2, 3, (0..6u8).flat_map(|v| [v, v, v, 255]).collect());
        let as_matte = |i: &RgbaImage| -> Vec<u8> { i.data.chunks(4).map(|p| p[0]).collect() };
        for cw in [true, false] {
            let r = m.rotate90(cw);
            assert_eq!((r.h, r.w), (3, 2));
            // so với cùng công thức của `rotate90_image` (gui/src/photo/doc.rs)
            let mut expect = RgbaImage::new(3, 2);
            for y in 0..2 {
                for x in 0..3 {
                    let (nx, ny) = if cw { (2 - 1 - y, x) } else { (y, 3 - 1 - x) };
                    expect.set_px(ny, nx, img.px(y, x));
                }
            }
            assert_eq!(*r.data, as_matte(&expect));
        }
        assert_eq!(*m.flip(true).data, vec![2, 1, 0, 5, 4, 3]);
        assert_eq!(*m.flip(false).data, vec![3, 4, 5, 0, 1, 2]);
        assert_eq!(*m.crop(1, 1, 5, 5).data, vec![4, 5]);
    }

    #[test]
    fn resize_plane_keeps_constant_and_averages() {
        let c = resize_plane(&[0.7; 12], 3, 4, 7, 9);
        assert!(c.iter().all(|v| (v - 0.7).abs() < 1e-6));
        // 4 → 2: trung bình từng cặp
        let d = resize_plane(&[0.0, 1.0, 0.0, 1.0], 1, 4, 1, 2);
        assert!(d.iter().all(|v| (v - 0.5).abs() < 1e-6));
    }

    #[test]
    fn box_mean_matches_naive() {
        let (h, w) = (5, 7);
        let src: Vec<f32> = (0..h * w).map(|i| ((i * 37) % 11) as f32).collect();
        let r = 2;
        let fast = box_mean(&src, h, w, r);
        // tách được → so với trung bình ngang rồi dọc tính tay
        for y in 0..h {
            for x in 0..w {
                let ys = y.saturating_sub(r)..(y + r + 1).min(h);
                let xs = x.saturating_sub(r)..(x + r + 1).min(w);
                let mut acc = 0.0;
                for yy in ys.clone() {
                    for xx in xs.clone() {
                        acc += src[yy * w + xx];
                    }
                }
                let naive = acc / (ys.len() * xs.len()) as f32;
                assert!((fast[y * w + x] - naive).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn edge_shift_grows_and_shrinks() {
        // hình vuông đặc 4×4 ở giữa ảnh 12×12
        let m = matte_of(12, 12, |y, x| {
            if (4..8).contains(&y) && (4..8).contains(&x) {
                255
            } else {
                0
            }
        });
        let count = |a: &[f32]| a.iter().filter(|&&v| v > 0.5).count();
        let base = refined_alpha(&m, 12, 12, &CutoutSettings::default(), 1.0);
        assert_eq!(count(&base), 16);
        let grow = CutoutSettings {
            edge: 1.0,
            ..Default::default()
        };
        assert_eq!(count(&refined_alpha(&m, 12, 12, &grow, 1.0)), 36);
        let shrink = CutoutSettings {
            edge: -1.0,
            ..Default::default()
        };
        assert_eq!(count(&refined_alpha(&m, 12, 12, &shrink, 1.0)), 4);
        // xem trước ở 1/4: bán kính 1 × 0.25 → 0 → không đổi
        assert_eq!(count(&refined_alpha(&m, 12, 12, &grow, 0.25)), 16);
        // feather: viền mềm nhưng tổng gần như giữ nguyên
        let soft = CutoutSettings {
            feather: 1.5,
            ..Default::default()
        };
        let a = refined_alpha(&m, 12, 12, &soft, 1.0);
        assert!(a.iter().any(|&v| v > 0.05 && v < 0.95));
        assert!((a.iter().sum::<f32>() - 16.0).abs() < 0.5);
    }

    #[test]
    fn apply_cutout_sets_alpha_and_keeps_original_alpha() {
        let mut img = RgbaImage::filled(2, 2, [10, 20, 30, 255]);
        img.data[3] = 100; // pixel (0,0) vốn bán trong suốt
        let m = Matte::new(2, 2, vec![255, 255, 0, 128]);
        let mut c = Cutout::new(m);
        c.settings.decontaminate = false;
        apply_cutout(&mut img, &c, 1.0);
        let alphas: Vec<u8> = img.data.chunks(4).map(|p| p[3]).collect();
        assert_eq!(alphas, vec![100, 255, 0, 128]);
    }

    #[test]
    fn decontaminate_removes_old_background_tint() {
        // nền hồng (240, 120, 160), người xám (60, 60, 60); pixel viền a = 0.5 là trộn của hai màu
        let (h, w) = (9, 9);
        let (bgc, fg) = ([240.0f32, 120.0, 160.0], [60.0f32, 60.0, 60.0]);
        let mut alpha = vec![0f32; h * w];
        let mut img = RgbaImage::new(h, w);
        for y in 0..h {
            for x in 0..w {
                let a = if x < 4 {
                    1.0
                } else if x == 4 {
                    0.5
                } else {
                    0.0
                };
                alpha[y * w + x] = a;
                let px: Vec<u8> = (0..3)
                    .map(|c| (fg[c] * a + bgc[c] * (1.0 - a)).round() as u8)
                    .collect();
                img.set_px(y, x, [px[0], px[1], px[2], 255]);
            }
        }
        decontaminate(&mut img, &alpha);
        let p = img.px(4, 4);
        for c in 0..3 {
            assert!((p[c] as f32 - fg[c]).abs() <= 2.0, "{p:?}");
        }
        // người và nền thuần không đổi
        assert_eq!(img.px(4, 0)[..3], [60, 60, 60]);
        assert_eq!(img.px(4, 8)[..3], [240, 120, 160]);
    }

    #[test]
    fn backdrop_color_and_image() {
        let mut person = RgbaImage::filled(2, 2, [200, 0, 0, 255]);
        person.data[3] = 0;
        person.data[7] = 128;
        let out = composite_backdrop(&person, &Backdrop::Color([0, 0, 255]));
        assert_eq!(out.px(0, 0), [0, 0, 255, 255]);
        assert_eq!(out.px(1, 1), [200, 0, 0, 255]);
        let half = out.px(0, 1);
        assert_eq!(half[3], 255);
        assert!(half[0] > 90 && half[0] < 110 && half[2] > 120 && half[2] < 135);
        // trong suốt: giữ nguyên
        assert_eq!(composite_backdrop(&person, &Backdrop::Transparent), person);
        // ảnh nền phủ kín, giữ tỉ lệ (cắt giữa): ảnh 2×4 trái đỏ / phải xanh → khung 2×2 lấy giữa
        let mut bg = RgbaImage::filled(2, 4, [255, 0, 0, 255]);
        for y in 0..2 {
            for x in 2..4 {
                bg.set_px(y, x, [0, 255, 0, 255]);
            }
        }
        let cover = cover_resize(&bg, 2, 2);
        assert_eq!(cover.px(0, 0), [255, 0, 0, 255]);
        assert_eq!(cover.px(0, 1), [0, 255, 0, 255]);
    }

    #[test]
    fn matte_from_model_follows_image_edges() {
        // ảnh 64×64: nửa trái tối (người), nửa phải sáng (nền); matte model thô 8×8
        let (h, w) = (64, 64);
        let mut img = RgbaImage::new(h, w);
        for y in 0..h {
            for x in 0..w {
                let v = if x < 32 { 40 } else { 220 };
                img.set_px(y, x, [v, v, v, 255]);
            }
        }
        let low: Vec<f32> = (0..64).map(|i| if i % 8 < 4 { 1.0 } else { 0.0 }).collect();
        let m = matte_from_model(&img, &low, 8, 8);
        assert_eq!((m.h, m.w), (64, 64));
        // xa viền: chắc chắn
        assert_eq!(m.data[32 * 64 + 2], 255);
        assert_eq!(m.data[32 * 64 + 61], 0);
        // ngay cạnh viền: guided filter làm viền sắc hơn so với chỉ phóng song tuyến
        let plain = resize_plane(&low, 8, 8, 64, 64);
        let sharp_err = (m.data[32 * 64 + 30] as f32 / 255.0 - 1.0).abs();
        let plain_err = (plain[32 * 64 + 30] - 1.0).abs();
        assert!(sharp_err < plain_err, "{sharp_err} vs {plain_err}");
    }

    #[test]
    fn brush_adds_and_removes_with_soft_edge() {
        let (h, w) = (20, 20);
        let mut m = vec![0u8; h * w];
        let bb = brush_dab(&mut m, h, w, 10.0, 10.0, 5.0, 0.5, true);
        assert_eq!(bb, (5, 5, 15, 15));
        assert_eq!(m[10 * w + 10], 255); // lõi đặc
        let edge = m[10 * w + 14]; // gần mép: mờ dần
        assert!(edge > 0 && edge < 255, "{edge}");
        assert_eq!(m[0], 0); // ngoài cọ không đổi
                             // bớt: về 0 ở lõi, không bao giờ tăng
        brush_dab(&mut m, h, w, 10.0, 10.0, 3.0, 1.0, false);
        assert_eq!(m[10 * w + 10], 0);
        // ngoài ảnh: không panic, hình bao rỗng
        let bb = brush_dab(&mut m, h, w, -50.0, -50.0, 5.0, 1.0, true);
        assert!(bb.0 >= bb.2 || bb.1 >= bb.3);
    }

    #[test]
    fn brush_line_has_no_gaps() {
        let (h, w) = (10, 100);
        let mut m = vec![0u8; h * w];
        brush_line(&mut m, h, w, Some((5.0, 5.0)), (95.0, 5.0), 2.0, 1.0, true);
        // chấm đầu bỏ qua (đã tô trước đó) nhưng từ x = 7 trở đi phải liền
        assert!((7..95).all(|x| m[5 * w + x] == 255));
        let bb = brush_line(&mut m, h, w, None, (50.0, 5.0), 2.0, 1.0, false);
        assert_eq!(m[5 * w + 50], 0);
        assert!(bb.0 <= 50 && bb.2 > 50);
    }
}
