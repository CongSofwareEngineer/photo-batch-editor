//! Chứng minh bản Rust (backend + adjustments + pipeline) khớp số học với bản Python.
//!
//! Fixture ở `tests/fixtures/` do `tools/gen_rust_fixtures.py` sinh từ `core/` Python (NumPy +
//! OpenCV). Test nạp cùng input, tự tính bằng Rust, rồi so với output Python trong sai số mà
//! các test số học của Python cho phép (xem `tests/test_adjustments.py`).

use std::path::PathBuf;

use pbe_core::adjustments::{
    self, apply_adjustments_f32, apply_adjustments_u8, ImageF32, ImageU8,
};
use pbe_core::backend::{self, Plane};
use pbe_core::pipeline;
use pbe_core::settings::AdjustmentSettings;
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

struct Fixtures {
    manifest: Value,
}

impl Fixtures {
    fn load() -> Self {
        let text = std::fs::read_to_string(fixtures_dir().join("manifest.json"))
            .expect("manifest.json — chạy: python3 tools/gen_rust_fixtures.py");
        Fixtures {
            manifest: serde_json::from_str(&text).unwrap(),
        }
    }

    fn entry(&self, name: &str) -> &Value {
        &self.manifest["arrays"][name]
    }

    fn shape(&self, name: &str) -> Vec<usize> {
        self.entry(name)["shape"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as usize)
            .collect()
    }

    fn bytes(&self, name: &str) -> Vec<u8> {
        let file = self.entry(name)["file"].as_str().unwrap();
        std::fs::read(fixtures_dir().join(file)).unwrap()
    }

    fn f32(&self, name: &str) -> (Vec<usize>, Vec<f32>) {
        assert_eq!(self.entry(name)["dtype"], "f32", "{name} không phải f32");
        let data = self
            .bytes(name)
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        (self.shape(name), data)
    }

    fn u8(&self, name: &str) -> (Vec<usize>, Vec<u8>) {
        assert_eq!(self.entry(name)["dtype"], "u8", "{name} không phải u8");
        (self.shape(name), self.bytes(name))
    }

    fn plane(&self, name: &str) -> Plane {
        let (shape, data) = self.f32(name);
        assert_eq!(shape.len(), 2, "{name} phải là 2D");
        Plane::from_vec(shape[0], shape[1], data)
    }

    fn image_f32(&self, name: &str) -> ImageF32 {
        let (shape, data) = self.f32(name);
        assert_eq!(shape.len(), 3);
        ImageF32 { h: shape[0], w: shape[1], data }
    }

    fn image_u8(&self, name: &str) -> ImageU8 {
        let (shape, data) = self.u8(name);
        assert_eq!(shape.len(), 3);
        ImageU8 { h: shape[0], w: shape[1], data }
    }
}

fn diff_stats(a: &[f32], b: &[f32]) -> (f32, f32) {
    assert_eq!(a.len(), b.len(), "độ dài lệch");
    let mut max = 0.0f32;
    let mut sum = 0.0f64;
    for (&x, &y) in a.iter().zip(b) {
        let d = (x - y).abs();
        if d > max {
            max = d;
        }
        sum += d as f64;
    }
    (max, (sum / a.len() as f64) as f32)
}

fn assert_close(name: &str, got: &[f32], expected: &[f32], max_tol: f32, mean_tol: f32) {
    let (mx, mean) = diff_stats(got, expected);
    assert!(
        mx <= max_tol && mean <= mean_tol,
        "{name}: max {mx} (<= {max_tol}), mean {mean} (<= {mean_tol})"
    );
}

const SETTING_CASES: &[&str] = &[
    "identity",
    "white_balance",
    "tone_pointwise",
    "tone_local",
    "color",
    "detail_clarity_sharpen",
    "detail_noise",
    "effects_vignette",
    "all",
];

fn settings_for(case: &str) -> AdjustmentSettings {
    let mut s = AdjustmentSettings::default();
    match case {
        "identity" => {}
        "white_balance" => {
            s.temperature = 20.0;
            s.tint = -10.0;
        }
        "tone_pointwise" => {
            s.exposure = 0.4;
            s.brightness = 10.0;
            s.contrast = 20.0;
        }
        "tone_local" => {
            s.highlights = -30.0;
            s.shadows = 30.0;
            s.whites = 10.0;
            s.blacks = -10.0;
        }
        "color" => {
            s.vibrance = 20.0;
            s.saturation = 10.0;
        }
        "detail_clarity_sharpen" => {
            s.clarity = 30.0;
            s.sharpening_amount = 40.0;
        }
        "detail_noise" => s.noise_reduction = 40.0,
        "effects_vignette" => s.vignette_amount = -30.0,
        "all" => {
            s.temperature = 20.0;
            s.tint = -10.0;
            s.exposure = 0.4;
            s.brightness = 10.0;
            s.contrast = 20.0;
            s.highlights = -30.0;
            s.shadows = 30.0;
            s.whites = 10.0;
            s.blacks = -10.0;
            s.clarity = 30.0;
            s.vibrance = 20.0;
            s.saturation = 10.0;
            s.sharpening_amount = 40.0;
            s.noise_reduction = 40.0;
            s.vignette_amount = -30.0;
        }
        _ => unreachable!(),
    }
    s
}

#[test]
fn backend_primitives_match_opencv() {
    let fx = Fixtures::load();
    let plane = fx.plane("plane");

    for (tag, sigma) in [("1_0", 1.0), ("2_5", 2.5), ("8_0", 8.0), ("20_0", 20.0)] {
        let got = backend::gaussian_blur(&plane, sigma);
        let (_, exp) = fx.f32(&format!("blur_sigma_{tag}"));
        assert_close(&format!("blur_sigma_{tag}"), &got.data, &exp, 2.0e-3, 3.0e-4);
    }
    for r in [1usize, 3, 7] {
        let got = backend::box_filter(&plane, r);
        let (_, exp) = fx.f32(&format!("box_r{r}"));
        assert_close(&format!("box_r{r}"), &got.data, &exp, 1.0e-4, 1.0e-5);
    }
    let got = backend::block_mean(&plane, 4, 0, 0);
    assert_close("block_mean_f4", &got.data, &fx.f32("block_mean_f4").1, 1.0e-5, 1.0e-6);
    let got = backend::block_mean(&plane, 5, 2, 1);
    assert_close("block_mean_f5_pad", &got.data, &fx.f32("block_mean_f5_pad").1, 1.0e-5, 1.0e-6);
    let got = backend::resize_linear(&plane, 97, 131);
    assert_close("resize_linear_97x131", &got.data, &fx.f32("resize_linear_97x131").1, 1.0e-4, 1.0e-5);
    let got = backend::resize_linear(&plane, 19, 23);
    assert_close("resize_linear_19x23", &got.data, &fx.f32("resize_linear_19x23").1, 1.0e-4, 1.0e-5);
    let got = backend::upscale(&backend::downscale(&plane, 3), 48, 64, 3);
    assert_close("upscale_f3", &got.data, &fx.f32("upscale_f3").1, 1.0e-4, 1.0e-5);
}

#[test]
fn guided_filter_matches() {
    let fx = Fixtures::load();
    let plane = fx.plane("plane");
    let out = adjustments::guided_filter(&plane, std::slice::from_ref(&plane), 4, 0.0009);
    assert_close("guided_r4_eps9e-4", &out[0].data, &fx.f32("guided_r4_eps9e-4").1, 1.0e-4, 1.0e-5);
}

#[test]
fn adjustments_match_python() {
    let fx = Fixtures::load();
    let img_f = fx.image_f32("img_f32");
    let img_u8 = fx.image_u8("img_u8");

    for case in SETTING_CASES {
        let s = settings_for(case);
        let got_f = apply_adjustments_f32(&img_f, &s);
        assert_close(&format!("adj_f32_{case}"), &got_f.data, &fx.f32(&format!("adj_f32_{case}")).1, 3.0e-3, 5.0e-4);

        let got_u8in = apply_adjustments_u8(&img_u8, &s);
        assert_close(&format!("adj_u8in_{case}"), &got_u8in.data, &fx.f32(&format!("adj_u8in_{case}")).1, 3.0e-3, 5.0e-4);

        let (out, warnings) = pipeline::process_u8(&img_u8, &s).unwrap();
        assert!(warnings.is_empty());
        let (_, exp) = fx.u8(&format!("proc_u8_{case}"));
        // u8: sai khác tối đa 1 mức (khác biệt làm tròn float) — như test_cpu_and_fake_gpu.
        let max = out
            .data
            .iter()
            .zip(&exp)
            .map(|(&a, &b)| (a as i32 - b as i32).abs())
            .max()
            .unwrap();
        assert!(max <= 1, "proc_u8_{case}: max diff {max} > 1");
    }
}

#[test]
fn full_default_pipeline_is_identity() {
    // Giống test_all_default_full_pipeline_is_identity: identity trả về đúng ảnh vào.
    let fx = Fixtures::load();
    let img_u8 = fx.image_u8("img_u8");
    let (out, warnings) = pipeline::process_u8(&img_u8, &AdjustmentSettings::default()).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(out.data, img_u8.data, "pipeline mặc định phải bằng ảnh vào");
}

#[test]
fn extremes_stay_in_range_and_finite() {
    // Giống test_extremes_are_finite_and_in_range cho một số ảnh biên.
    let fx = Fixtures::load();
    let img_f = fx.image_f32("img_f32");
    let (h, w) = (img_f.h, img_f.w);
    let zeros = ImageF32 { h, w, data: vec![0.0; h * w * 3] };
    let ones = ImageF32 { h, w, data: vec![1.0; h * w * 3] };
    let mut cases: Vec<(&str, f64, f64)> = vec![];
    for spec in pbe_core::settings::SLIDERS.iter() {
        cases.push((spec.key, spec.minimum, spec.maximum));
    }
    for (key, lo, hi) in cases {
        for value in [lo, hi] {
            let mut s = AdjustmentSettings::default();
            s.set(key, value);
            for img in [&img_f, &zeros, &ones] {
                let out = apply_adjustments_f32(img, &s);
                for &v in &out.data {
                    assert!(v.is_finite(), "{key}={value}: không hữu hạn");
                    assert!((0.0..=1.0).contains(&v), "{key}={value}: {v} ngoài [0,1]");
                }
            }
        }
    }
}
