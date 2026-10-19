//! Làm mịn da (xem `docs/instruction/skin-smoothing.md`).
//!
//! Lọc giữ cạnh (guided filter tự dẫn, từng kênh) xoá mụn / vết thâm / da sần nhưng giữ mắt, tóc,
//! viền; chỉ áp trong vùng có màu da (YCbCr) và cộng lại một phần chi tiết rất nhỏ (lỗ chân lông)
//! để da không bị "nhựa".

use crate::backend::{self, Plane};
use crate::photo::matting::{box_mean, guided_filter};
use crate::photo::RgbaImage;
use crate::settings::SliderSpec;

const fn ss(
    key: &'static str,
    label: &'static str,
    minimum: f64,
    maximum: f64,
    step: f64,
    default: f64,
    decimals: usize,
) -> SliderSpec {
    SliderSpec {
        key,
        label,
        panel: "Skin",
        group: "Skin",
        minimum,
        maximum,
        step,
        default,
        decimals,
    }
}

/// 3 thanh trượt: Độ mịn (%), Bán kính (px ảnh gốc), Giữ kết cấu da (%).
pub const SKIN_SLIDERS: [SliderSpec; 3] = [
    ss("amount", "Smoothness", 0.0, 100.0, 1.0, 0.0, 0),
    ss("radius", "Radius", 1.0, 30.0, 0.5, 8.0, 1),
    ss("detail", "Keep texture", 0.0, 100.0, 1.0, 30.0, 0),
];

/// Thông số làm mịn da; `amount == 0` = tắt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinSmoothSettings {
    pub amount: f64,
    pub radius: f64,
    pub detail: f64,
    /// true = chỉ vùng có màu da; false = cả ảnh.
    pub skin_only: bool,
}

impl Default for SkinSmoothSettings {
    fn default() -> Self {
        SkinSmoothSettings {
            amount: 0.0,
            radius: 8.0,
            detail: 30.0,
            skin_only: true,
        }
    }
}

impl SkinSmoothSettings {
    pub fn is_default(&self) -> bool {
        self.amount <= 0.0
    }

    pub fn get(&self, key: &str) -> Option<f64> {
        Some(match key {
            "amount" => self.amount,
            "radius" => self.radius,
            "detail" => self.detail,
            _ => return None,
        })
    }

    /// Đặt theo khoá, kẹp vào khoảng của [`SKIN_SLIDERS`].
    pub fn set(&mut self, key: &str, value: f64) -> bool {
        let Some(spec) = SKIN_SLIDERS.iter().find(|s| s.key == key) else {
            return false;
        };
        let v = value.clamp(spec.minimum, spec.maximum);
        match key {
            "amount" => self.amount = v,
            "radius" => self.radius = v,
            "detail" => self.detail = v,
            _ => return false,
        }
        true
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "amount": self.amount,
            "radius": self.radius,
            "detail": self.detail,
            "skin_only": self.skin_only,
        })
    }

    /// Khoá thiếu → mặc định, ngoài khoảng → kẹp.
    pub fn from_json(data: &serde_json::Value) -> SkinSmoothSettings {
        let mut out = SkinSmoothSettings::default();
        let Some(map) = data.as_object() else {
            return out;
        };
        for spec in SKIN_SLIDERS {
            if let Some(v) = map.get(spec.key).and_then(|v| v.as_f64()) {
                out.set(spec.key, v);
            }
        }
        if let Some(b) = map.get("skin_only").and_then(|v| v.as_bool()) {
            out.skin_only = b;
        }
        out
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Xác suất 0…1 pixel là da: Cb ∈ [77, 127], Cr ∈ [133, 173] (ngưỡng YCbCr kinh điển, mép mềm
/// ±8), bỏ vùng quá tối (tóc, bóng đổ).
pub fn skin_probability(r: u8, g: u8, b: u8) -> f32 {
    let (r, g, b) = (r as f32, g as f32, b as f32);
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
    let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
    let band = |v: f32, lo: f32, hi: f32| {
        smoothstep(lo - 8.0, lo + 8.0, v) * (1.0 - smoothstep(hi - 8.0, hi + 8.0, v))
    };
    band(cb, 77.0, 127.0) * band(cr, 133.0, 173.0) * smoothstep(30.0, 60.0, y)
}

/// Mặt nạ da `h × w` (chưa làm mềm).
pub fn skin_mask(img: &RgbaImage) -> Vec<f32> {
    img.data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| skin_probability(p[0], p[1], p[2]))
        .collect()
}

/// Làm mịn da. `scale` = tỉ lệ của `img` so với ảnh gốc (bản xem trước thu nhỏ → bán kính thu
/// theo) để canvas và file xuất trông như nhau. Alpha giữ nguyên.
pub fn smooth_skin(img: &RgbaImage, s: &SkinSmoothSettings, scale: f64) -> RgbaImage {
    if s.is_default() || img.is_empty() {
        return img.clone();
    }
    let (h, w) = (img.h, img.w);
    let n = h * w;
    let r = ((s.radius * scale).round() as usize).max(1);
    let a = (s.amount / 100.0) as f32;
    let detail = (s.detail / 100.0) as f32;
    // Ngưỡng phương sai của guided filter (thang 0…1): vùng có độ lệch chuẩn nhỏ hơn ~eps^½
    // (mụn, da sần) bị làm phẳng, cạnh mạnh hơn (mắt, môi, tóc) được giữ. Mạnh tay hơn khi amount lớn.
    let sd = 0.03 + 0.07 * a;
    let eps = sd * sd;

    let mask: Vec<f32> = if s.skin_only {
        // làm mềm mặt nạ để không lộ ranh giới vùng da
        box_mean(&skin_mask(img), h, w, (r * 2).max(1))
    } else {
        vec![1.0; n]
    };

    // Chi tiết rất nhỏ (lỗ chân lông) lấy trên độ sáng: L − blur(L).
    let lum = Plane::from_vec(
        h,
        w,
        img.data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0)
            .collect(),
    );
    let fine_sigma = (r as f64 / 5.0).max(0.6);
    let lum_blur = backend::gaussian_blur_exact(&lum, fine_sigma);

    // 3 kênh lọc song song (mỗi kênh độc lập).
    let filtered: Vec<(Vec<f32>, Vec<f32>)> = std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..3)
            .map(|c| {
                scope.spawn(move || {
                    let p: Vec<f32> = img
                        .data
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|px| px[c] as f32 / 255.0)
                        .collect();
                    let q = guided_filter(&p, &p, h, w, r, eps);
                    (p, q)
                })
            })
            .collect();
        jobs.into_iter()
            .map(|j| j.join().expect("luồng làm mịn da"))
            .collect()
    });
    let mut out = img.clone();
    for (c, (p, q)) in filtered.iter().enumerate() {
        for i in 0..n {
            let k = a * mask[i];
            if k <= 0.0 {
                continue;
            }
            let fine = lum.data[i] - lum_blur.data[i];
            let target = q[i] + detail * fine;
            let v = p[i] + k * (target - p[i]);
            out.data[i * 4 + c] = (v * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SKIN: [u8; 3] = [224, 172, 140];

    /// Mảng da có chấm "mụn" tối ngẫu nhiên + một cạnh mạnh (nửa dưới là màu tóc tối).
    fn face(h: usize, w: usize) -> RgbaImage {
        let mut img = RgbaImage::new(h, w);
        let mut state = 12345u64;
        for y in 0..h {
            for x in 0..w {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let jitter = ((state >> 33) % 21) as i32 - 10;
                let p = if y >= h / 2 {
                    [40, 30, 25, 255]
                } else {
                    let v = |c: u8| (c as i32 + jitter).clamp(0, 255) as u8;
                    [v(SKIN[0]), v(SKIN[1]), v(SKIN[2]), 255]
                };
                img.set_px(y, x, p);
            }
        }
        img
    }

    fn std_red(img: &RgbaImage, y0: usize, y1: usize) -> f64 {
        let vals: Vec<f64> = (y0..y1)
            .flat_map(|y| (4..img.w - 4).map(move |x| (y, x)))
            .map(|(y, x)| img.px(y, x)[0] as f64)
            .collect();
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64).sqrt()
    }

    #[test]
    fn skin_colour_is_detected_and_others_are_not() {
        assert!(skin_probability(SKIN[0], SKIN[1], SKIN[2]) > 0.9);
        assert!(skin_probability(30, 90, 200) < 0.05); // xanh dương
        assert!(skin_probability(40, 160, 60) < 0.05); // xanh lá
        assert!(skin_probability(15, 12, 10) < 0.05); // quá tối
    }

    #[test]
    fn smoothing_flattens_skin_and_keeps_the_edge() {
        let img = face(40, 60);
        let s = SkinSmoothSettings {
            amount: 100.0,
            radius: 4.0,
            detail: 0.0,
            skin_only: true,
        };
        let out = smooth_skin(&img, &s, 1.0);
        let before = std_red(&img, 4, 14);
        let after = std_red(&out, 4, 14);
        assert!(
            after < before * 0.5,
            "da chưa mịn: {before:.2} → {after:.2}"
        );
        // cạnh da / tóc vẫn sắc, vùng tóc (không phải da) gần như không đổi
        assert!(out.px(10, 30)[0] > 180);
        assert_eq!(out.px(35, 30), img.px(35, 30));
        assert_eq!(out.px(5, 5)[3], 255);
    }

    #[test]
    fn keep_texture_and_default() {
        let img = face(40, 60);
        let mut s = SkinSmoothSettings {
            amount: 100.0,
            radius: 4.0,
            detail: 0.0,
            skin_only: true,
        };
        let flat = std_red(&smooth_skin(&img, &s, 1.0), 4, 14);
        s.detail = 100.0;
        let textured = std_red(&smooth_skin(&img, &s, 1.0), 4, 14);
        assert!(
            textured > flat,
            "giữ kết cấu phải còn chi tiết: {flat:.2} vs {textured:.2}"
        );
        assert_eq!(smooth_skin(&img, &SkinSmoothSettings::default(), 1.0), img);
    }

    #[test]
    fn settings_json_clamps() {
        let s = SkinSmoothSettings::from_json(
            &serde_json::json!({"amount": 500, "radius": 0, "skin_only": false}),
        );
        assert_eq!(s.amount, 100.0);
        assert_eq!(s.radius, 1.0);
        assert_eq!(s.detail, 30.0);
        assert!(!s.skin_only);
        assert_eq!(SkinSmoothSettings::from_json(&s.to_json()), s);
    }
}
