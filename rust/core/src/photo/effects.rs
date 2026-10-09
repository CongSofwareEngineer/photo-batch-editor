//! Hiệu ứng pixel của photo editor (RGBA uint8). Port của `core/photo/effects.py`.
//!
//! * Blur / pixelate một vùng (layer "Blur").
//! * Các thanh trượt "Adjust" map sang chỉnh sửa kiểu Camera Raw của trình chỉnh nhiều ảnh,
//!   nên hai trình sửa cho ra cùng một kết quả ([`crate::pipeline::apply_adjustments_u8`]).

use crate::adjustments::{self, ImageU8};
use crate::backend::{self, Plane};
use crate::photo::RgbaImage;
use crate::settings::{AdjustmentSettings, SliderSpec};

/// Hai chế độ của layer Blur.
pub const BLUR_MODES: [&str; 2] = ["blur", "pixelate"];

/// Sigma của Gaussian cho `strength` 1–100, tỉ lệ với cỡ ảnh.
pub fn blur_sigma(strength: f64, long_side: usize) -> f64 {
    (strength / 100.0 * long_side.max(1) as f64 / 60.0).max(0.5)
}

/// Cỡ ô (px) của pixelate cho `strength` 1–100.
pub fn pixel_block(strength: f64, long_side: usize) -> usize {
    let v = (strength / 100.0 * long_side.max(1) as f64 / 25.0).round();
    (v.max(0.0) as usize).max(2)
}

/// Blur không làm tối viền trong suốt (blur trên màu đã premultiply).
///
/// Giống bản Python: premultiply bằng alpha, blur cả 4 kênh viền `BORDER_REFLECT`, rồi
/// un-premultiply ở nơi alpha > 0.5 (nơi khác coi như trong suốt hẳn → màu 0).
pub fn gaussian_blur(img: &RgbaImage, sigma: f64) -> RgbaImage {
    if sigma <= 0.0 || img.is_empty() {
        return img.clone();
    }
    let n = img.h * img.w;
    let mut planes: [Plane; 4] = [
        Plane::new(img.h, img.w),
        Plane::new(img.h, img.w),
        Plane::new(img.h, img.w),
        Plane::new(img.h, img.w),
    ];
    for i in 0..n {
        let a = img.data[i * 4 + 3] as f32 / 255.0;
        planes[0].data[i] = img.data[i * 4] as f32 * a;
        planes[1].data[i] = img.data[i * 4 + 1] as f32 * a;
        planes[2].data[i] = img.data[i * 4 + 2] as f32 * a;
        planes[3].data[i] = img.data[i * 4 + 3] as f32;
    }
    for plane in planes.iter_mut() {
        *plane = backend::gaussian_blur_exact(plane, sigma);
    }
    let mut out = RgbaImage::new(img.h, img.w);
    for i in 0..n {
        let alpha = planes[3].data[i];
        let k = if alpha > 0.5 {
            1.0 / (alpha / 255.0).max(1e-6)
        } else {
            0.0
        };
        for c in 0..3 {
            out.data[i * 4 + c] = (planes[c].data[i] * k).round_ties_even().clamp(0.0, 255.0) as u8;
        }
        out.data[i * 4 + 3] = alpha.round_ties_even().clamp(0.0, 255.0) as u8;
    }
    out
}

/// Mosaic ô `block` px, **gióng theo lưới của cả ảnh** (`origin` = lệch của vùng đang xử lý),
/// nhờ vậy di chuyển layer Blur không làm các ô nhảy.
///
/// Tương đương `copyMakeBorder(BORDER_REPLICATE)` + `INTER_AREA` (hệ số nguyên = trung bình ô)
/// + `INTER_NEAREST` của bản Python, nhưng tính trực tiếp nên không cần cấp phát ảnh đệm.
pub fn pixelate(img: &RgbaImage, block: usize, origin: (i64, i64)) -> RgbaImage {
    let block = block.max(1);
    if block == 1 || img.is_empty() {
        return img.clone();
    }
    let b = block as i64;
    let ox = origin.0.rem_euclid(b) as usize;
    let oy = origin.1.rem_euclid(b) as usize;
    let (pad_l, pad_t) = (ox, oy);
    let pw = (img.w + pad_l).div_ceil(block) * block;
    let ph = (img.h + pad_t).div_ceil(block) * block;
    let (cells_x, cells_y) = (pw / block, ph / block);

    // Trung bình từng ô trên ảnh đã nhân bản viền (BORDER_REPLICATE = kẹp chỉ số).
    let mut means = vec![[0f32; 4]; cells_x * cells_y];
    for cy in 0..cells_y {
        for cx in 0..cells_x {
            let mut acc = [0f64; 4];
            for py in cy * block..(cy + 1) * block {
                let sy = (py as i64 - pad_t as i64).clamp(0, img.h as i64 - 1) as usize;
                for px in cx * block..(cx + 1) * block {
                    let sx = (px as i64 - pad_l as i64).clamp(0, img.w as i64 - 1) as usize;
                    let i = (sy * img.w + sx) * 4;
                    for c in 0..4 {
                        acc[c] += img.data[i + c] as f64;
                    }
                }
            }
            let n = (block * block) as f64;
            let cell = &mut means[cy * cells_x + cx];
            for c in 0..4 {
                cell[c] = (acc[c] / n) as f32;
            }
        }
    }

    let mut out = RgbaImage::new(img.h, img.w);
    for y in 0..img.h {
        let cy = (y + pad_t) / block;
        for x in 0..img.w {
            let cell = means[cy * cells_x + (x + pad_l) / block];
            let i = (y * img.w + x) * 4;
            for c in 0..4 {
                out.data[i + c] = cell[c].round_ties_even().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// Áp hiệu ứng của layer Blur theo `mode` ("blur" / "pixelate").
pub fn apply_blur_effect(
    img: &RgbaImage,
    mode: &str,
    strength: f64,
    long_side: usize,
    origin: (i64, i64),
) -> RgbaImage {
    if mode == "pixelate" {
        pixelate(img, pixel_block(strength, long_side), origin)
    } else {
        gaussian_blur(img, blur_sigma(strength, long_side))
    }
}

// --- Bảng Adjust ---------------------------------------------------------------------------

const fn s(key: &'static str, label: &'static str, minimum: f64, maximum: f64) -> SliderSpec {
    SliderSpec {
        key,
        label,
        panel: "Adjust",
        group: "Adjust",
        minimum,
        maximum,
        step: 1.0,
        default: 0.0,
        decimals: 0,
    }
}

/// 5 thanh trượt của bảng Adjust trong photo editor (tập con của 15 chỉnh sửa Camera Raw).
pub const ADJUST_SLIDERS: [SliderSpec; 5] = [
    s("brightness", "Brightness", -100.0, 100.0),
    s("contrast", "Contrast", -100.0, 100.0),
    s("saturation", "Saturation", -100.0, 100.0),
    s("temperature", "Temperature", -100.0, 100.0),
    s("sharpness", "Sharpness", 0.0, 150.0),
];

/// Giá trị 5 thanh trượt Adjust của một layer.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PhotoAdjust {
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub temperature: f64,
    pub sharpness: f64,
}

impl PhotoAdjust {
    pub fn is_default(&self) -> bool {
        *self == PhotoAdjust::default()
    }

    pub fn get(&self, key: &str) -> Option<f64> {
        Some(match key {
            "brightness" => self.brightness,
            "contrast" => self.contrast,
            "saturation" => self.saturation,
            "temperature" => self.temperature,
            "sharpness" => self.sharpness,
            _ => return None,
        })
    }

    /// Đặt một thanh trượt theo khoá, tự kẹp vào khoảng của [`ADJUST_SLIDERS`].
    pub fn set(&mut self, key: &str, value: f64) -> bool {
        let Some(spec) = ADJUST_SLIDERS.iter().find(|s| s.key == key) else {
            return false;
        };
        let v = value.clamp(spec.minimum, spec.maximum);
        match key {
            "brightness" => self.brightness = v,
            "contrast" => self.contrast = v,
            "saturation" => self.saturation = v,
            "temperature" => self.temperature = v,
            "sharpness" => self.sharpness = v,
            _ => return false,
        }
        true
    }

    /// Cùng thuật toán với trình chỉnh nhiều ảnh (chỉnh sửa kiểu Camera Raw).
    pub fn to_settings(&self) -> AdjustmentSettings {
        AdjustmentSettings {
            brightness: self.brightness,
            contrast: self.contrast,
            saturation: self.saturation,
            temperature: self.temperature,
            sharpening_amount: self.sharpness,
            ..Default::default()
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "brightness": self.brightness,
            "contrast": self.contrast,
            "saturation": self.saturation,
            "temperature": self.temperature,
            "sharpness": self.sharpness,
        })
    }

    /// Đọc từ JSON của file dự án; khoá thiếu / sai kiểu → 0, giá trị ngoài khoảng bị kẹp.
    pub fn from_json(data: &serde_json::Value) -> PhotoAdjust {
        let mut out = PhotoAdjust::default();
        let Some(map) = data.as_object() else {
            return out;
        };
        for spec in ADJUST_SLIDERS {
            let v = map.get(spec.key).and_then(|v| v.as_f64()).unwrap_or(0.0);
            out.set(spec.key, v);
        }
        out
    }
}

/// RGBA uint8 → RGBA uint8 đã chỉnh (alpha giữ nguyên).
///
/// `long_side` là cạnh dài của **ảnh gốc** khi `img` là bản xem trước thu nhỏ, để bán kính
/// sharpening trông như nhau ở mọi cỡ xem trước.
pub fn apply_photo_adjust(img: &RgbaImage, adjust: &PhotoAdjust, long_side: Option<usize>) -> RgbaImage {
    if adjust.is_default() || img.is_empty() {
        return img.clone();
    }
    let n = img.h * img.w;
    let mut rgb = ImageU8 {
        h: img.h,
        w: img.w,
        data: vec![0; n * 3],
    };
    for i in 0..n {
        rgb.data[i * 3..i * 3 + 3].copy_from_slice(&img.data[i * 4..i * 4 + 3]);
    }
    let settings = adjust.to_settings();
    let planar = adjustments::apply_adjustments_planar_u8_ls(&rgb, &settings, long_side);
    let out = adjustments::planar_to_uint8(&planar);
    let mut res = RgbaImage::new(img.h, img.w);
    for i in 0..n {
        res.data[i * 4..i * 4 + 3].copy_from_slice(&out.data[i * 3..i * 3 + 3]);
        res.data[i * 4 + 3] = img.data[i * 4 + 3];
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline;

    fn noise(h: usize, w: usize, seed: u64) -> RgbaImage {
        // LCG đơn giản: chỉ cần ảnh "nhiễu" tái lập được, không cần giống numpy.
        let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut data = vec![0u8; h * w * 4];
        for i in 0..h * w {
            for c in 0..3 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                data[i * 4 + c] = (state >> 33) as u8;
            }
            data[i * 4 + 3] = 255;
        }
        RgbaImage::from_vec(h, w, data)
    }

    fn rgb_std(img: &RgbaImage) -> f64 {
        let vals: Vec<f64> = (0..img.h * img.w)
            .flat_map(|i| (0..3).map(move |c| (i, c)))
            .map(|(i, c)| img.data[i * 4 + c] as f64)
            .collect();
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64).sqrt()
    }

    #[test]
    fn pixelate_blocks_are_uniform_and_aligned() {
        // Giống test_pixelate_blocks_are_uniform_and_aligned.
        let img = noise(64, 64, 0);
        let region = img.crop(7, 5, 40, 40);
        let out = pixelate(&region, 8, (7, 5));
        assert_eq!((out.h, out.w), (40, 40));
        // Ô gióng theo bội số 8 tuyệt đối: vùng (7, 5) → ô đầy đủ đầu tiên ở local (x=1, y=3).
        let first = out.px(3, 1);
        for y in 3..11 {
            for x in 1..9 {
                assert_eq!(out.px(y, x), first, "ô không đồng nhất tại ({y}, {x})");
            }
        }
        assert!(rgb_std(&out) < rgb_std(&region));
    }

    #[test]
    fn gaussian_blur_smooths_and_keeps_alpha_edges() {
        // Giống test_gaussian_blur_smooths_and_keeps_alpha_edges.
        let img = noise(50, 50, 1);
        let out = gaussian_blur(&img, 3.0);
        assert!(rgb_std(&out) < rgb_std(&img) / 3.0);

        let mut clear = RgbaImage::new(20, 20);
        for y in 5..15 {
            for x in 5..15 {
                clear.set_px(y, x, [255, 255, 255, 255]);
            }
        }
        let soft = gaussian_blur(&clear, 2.0);
        for y in 0..20 {
            for x in 0..20 {
                let p = soft.px(y, x);
                if p[3] > 30 {
                    // Không có viền tối do màu đen trong suốt bị trộn vào.
                    assert!(p[0] > 200 && p[1] > 200 && p[2] > 200, "viền tối tại ({y}, {x}): {p:?}");
                }
            }
        }
    }

    #[test]
    fn blur_sigma_and_block_scale_with_image() {
        assert_eq!(blur_sigma(0.0, 6000), 0.5); // sàn 0.5
        assert!(blur_sigma(100.0, 6000) > blur_sigma(100.0, 600));
        assert_eq!(pixel_block(0.0, 6000), 2); // sàn 2
        assert_eq!(pixel_block(100.0, 2500), 100);
    }

    #[test]
    fn photo_adjust_uses_the_batch_algorithms() {
        // Giống test_photo_adjust_uses_the_batch_algorithms.
        let src = noise(60, 80, 7);
        let mut rgba = src.clone();
        for i in 0..rgba.h * rgba.w {
            rgba.data[i * 4 + 3] = 200;
        }
        let adjust = PhotoAdjust {
            brightness: 20.0,
            contrast: 15.0,
            saturation: -30.0,
            temperature: 25.0,
            sharpness: 40.0,
        };
        let out = apply_photo_adjust(&rgba, &adjust, None);

        let n = rgba.h * rgba.w;
        let mut rgb = ImageU8 { h: rgba.h, w: rgba.w, data: vec![0; n * 3] };
        for i in 0..n {
            rgb.data[i * 3..i * 3 + 3].copy_from_slice(&rgba.data[i * 4..i * 4 + 3]);
        }
        let expected = pipeline::to_uint8(&pipeline::apply_adjustments_u8(&rgb, &adjust.to_settings()));
        for i in 0..n {
            for c in 0..3 {
                assert_eq!(out.data[i * 4 + c], expected.data[i * 3 + c], "pixel {i} kênh {c}");
            }
            assert_eq!(out.data[i * 4 + 3], 200);
        }
        // Mặc định → trả lại chính ảnh vào.
        assert_eq!(apply_photo_adjust(&rgba, &PhotoAdjust::default(), None), rgba);
    }

    #[test]
    fn photo_adjust_from_json_clamps() {
        // Giống test_photo_adjust_from_dict_clamps.
        let a = PhotoAdjust::from_json(&serde_json::json!({
            "brightness": 500, "sharpness": -3, "contrast": "x"
        }));
        assert_eq!(a.brightness, 100.0);
        assert_eq!(a.sharpness, 0.0);
        assert_eq!(a.contrast, 0.0);
        assert_eq!(PhotoAdjust::from_json(&serde_json::Value::Null), PhotoAdjust::default());
    }

    #[test]
    fn adjust_sliders_exist_in_the_batch_sliders() {
        // Adjust là tập con của 15 chỉnh sửa; "sharpness" map sang "sharpening_amount".
        for spec in ADJUST_SLIDERS {
            let key = if spec.key == "sharpness" { "sharpening_amount" } else { spec.key };
            assert!(crate::settings::slider_by_key(key).is_some(), "{} thiếu", spec.key);
        }
    }
}
