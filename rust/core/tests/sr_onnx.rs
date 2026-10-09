//! Super Resolution (ONNX) khớp bản Python. Chỉ chạy khi bật feature `sr`:
//!   cargo test --features sr --test sr_onnx
//! Xem docs/instruction/rust-port.md (GĐ3).

#![cfg(feature = "sr")]

use std::path::PathBuf;

use pbe_core::adjustments::ImageF32;
use pbe_core::enhance::{default_model_path, SuperResolver};
use pbe_core::pipeline::Upscaler;
use serde_json::Value;

fn sr_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("sr")
}

fn load(key: &str) -> (Vec<usize>, Vec<f32>) {
    let m: Value = serde_json::from_str(&std::fs::read_to_string(sr_dir().join("sr_manifest.json")).unwrap()).unwrap();
    let shape: Vec<usize> = m["arrays"][key]["shape"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect();
    let file = m["arrays"][key]["file"].as_str().unwrap();
    let data = std::fs::read(sr_dir().join(file)).unwrap().chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    (shape, data)
}

#[test]
fn sr_matches_python() {
    if !default_model_path().is_file() {
        eprintln!("bỏ qua: thiếu model SR");
        return;
    }
    let (in_shape, in_data) = load("sr_in");
    let img = ImageF32 { h: in_shape[0], w: in_shape[1], data: in_data };
    let resolver = SuperResolver::new(None, None).expect("tạo resolver");
    for factor in [2i64, 4] {
        let (shape, expected) = load(&format!("sr_out_{factor}x"));
        let out = resolver.upscale(&img, factor).expect("upscale");
        assert_eq!(vec![out.h, out.w, 3], shape, "SR {factor}x shape");
        // Sai số do khác phiên bản ONNX runtime; test Python dùng 1–2/255.
        let n = out.data.len();
        let mut max = 0.0f32;
        let mut sum = 0.0f64;
        for i in 0..n {
            let d = (out.data[i] - expected[i]).abs();
            max = max.max(d);
            sum += d as f64;
        }
        let mean = (sum / n as f64) as f32;
        assert!(mean <= 2.0 / 255.0, "SR {factor}x mean {mean} > 2/255");
        assert!(max <= 12.0 / 255.0, "SR {factor}x max {max} > 12/255");
    }
}
