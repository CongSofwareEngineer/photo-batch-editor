//! Image Size: tính kích thước đích + giới hạn. Port phần tính toán của `core/image_size.py`.
//!
//! Phần resample pixel (cv2.resize/unsharp) để ở giai đoạn 2 (cần crate xử lý ảnh).

use crate::settings::{ImageSizeSettings, MAX_OUTPUT_LONG_EDGE, MAX_OUTPUT_MEGAPIXELS};

pub const CLAMP_WARNING: &str = "Image Size clamped to 20,000 px / 200 MP";
pub const SR_SKIP_WARNING: &str =
    "Super Resolution skipped: output would exceed 20,000 px / 200 MP";

pub fn exceeds_limits(width: i64, height: i64) -> bool {
    width.max(height) > MAX_OUTPUT_LONG_EDGE
        || width * height > MAX_OUTPUT_MEGAPIXELS * 1_000_000
}

/// Kích thước lớn nhất cùng tỉ lệ mà vừa với giới hạn xuất.
pub fn clamp_to_limits(width: i64, height: i64) -> (i64, i64) {
    if !exceeds_limits(width, height) {
        return (width, height);
    }
    let mut scale = (MAX_OUTPUT_LONG_EDGE as f64 / width.max(height) as f64).min(
        (MAX_OUTPUT_MEGAPIXELS as f64 * 1_000_000.0 / (width as f64 * height as f64)).sqrt(),
    );
    loop {
        let w = ((width as f64 * scale) as i64).max(1);
        let h = ((height as f64 * scale) as i64).max(1);
        if !exceeds_limits(w, h) {
            return (w, h);
        }
        scale *= 0.9999;
    }
}

fn round_half_away(x: f64) -> i64 {
    x.round() as i64
}

/// `(width, height, warnings)` mới cho ảnh `width × height`.
pub fn target_size(width: i64, height: i64, s: &ImageSizeSettings) -> (i64, i64, Vec<String>) {
    if s.mode == "off" {
        return (width, height, vec![]);
    }
    let v = s.value;
    let (wf, hf) = (width as f64, height as f64);
    let (tw, th) = match s.mode.as_str() {
        "percent" => (wf * v / 100.0, hf * v / 100.0),
        "long_edge" => {
            let k = v / wf.max(hf);
            (wf * k, hf * k)
        }
        "width" => (v, hf * v / wf),
        "height" => (wf * v / hf, v),
        _ => return (width, height, vec![]),
    };
    let mut tw_i = round_half_away(tw).max(1);
    let mut th_i = round_half_away(th).max(1);
    if s.dont_enlarge && (tw_i > width || th_i > height) {
        return (width, height, vec![]);
    }
    let mut warnings = vec![];
    if exceeds_limits(tw_i, th_i) {
        let (w, h) = clamp_to_limits(tw_i, th_i);
        tw_i = w;
        th_i = h;
        warnings.push(CLAMP_WARNING.to_string());
    }
    (tw_i, th_i, warnings)
}

pub fn sr_exceeds_limits(width: i64, height: i64, factor: i64) -> bool {
    factor > 1 && exceeds_limits(width * factor, height * factor)
}

/// Kích thước đầu ra sau Super Resolution và Image Size + các cảnh báo.
/// Port của `pipeline.predict_output_size`.
pub fn predict_output_size(
    width: i64,
    height: i64,
    sr_factor: i64,
    image_size: &ImageSizeSettings,
) -> (i64, i64, Vec<String>) {
    let mut warnings = vec![];
    let (mut w, mut h) = (width, height);
    if sr_factor > 1 {
        if sr_exceeds_limits(width, height, sr_factor) {
            warnings.push(SR_SKIP_WARNING.to_string());
        } else {
            w *= sr_factor;
            h *= sr_factor;
        }
    }
    let (fw, fh, more) = target_size(w, h, image_size);
    warnings.extend(more);
    (fw, fh, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sz(mode: &str, value: f64) -> ImageSizeSettings {
        ImageSizeSettings::new(mode, value, "automatic", false)
    }

    #[test]
    fn modes_give_expected_size() {
        let cases = [
            ("percent", 50.0, (300, 200)),
            ("percent", 200.0, (1200, 800)),
            ("long_edge", 900.0, (900, 600)),
            ("width", 300.0, (300, 200)),
            ("height", 100.0, (150, 100)),
        ];
        for (mode, value, expected) in cases {
            let (w, h, warn) = target_size(600, 400, &sz(mode, value));
            assert_eq!((w, h), expected);
            assert!(warn.is_empty());
        }
    }

    #[test]
    fn portrait_long_edge() {
        let (w, h, _) = target_size(400, 600, &sz("long_edge", 300.0));
        assert_eq!((w, h), (200, 300));
    }

    #[test]
    fn dont_enlarge() {
        let big = ImageSizeSettings::new("long_edge", 2000.0, "automatic", true);
        assert_eq!(target_size(600, 400, &big).0, 600);
        let pct = ImageSizeSettings::new("percent", 150.0, "automatic", true);
        let (w, h, _) = target_size(600, 400, &pct);
        assert_eq!((w, h), (600, 400));
        let small = ImageSizeSettings::new("long_edge", 300.0, "automatic", true);
        let (w, h, _) = target_size(600, 400, &small);
        assert_eq!((w, h), (300, 200));
    }

    #[test]
    fn clamped_to_limits() {
        let (w, h, warn) = target_size(6000, 4000, &sz("percent", 400.0));
        assert_eq!(warn, vec![CLAMP_WARNING.to_string()]);
        assert!(w.max(h) <= MAX_OUTPUT_LONG_EDGE && w * h <= 200_000_000);
        assert!((w as f64 / h as f64 - 1.5).abs() < 0.01);
        let (w, h, warn) = target_size(10000, 9000, &sz("percent", 190.0));
        assert!(!warn.is_empty());
        assert!(w * h <= 200_000_000 && w.max(h) <= MAX_OUTPUT_LONG_EDGE);
    }

    #[test]
    fn aspect_ratio_preserved() {
        for (mode, value) in [("percent", 37.0), ("long_edge", 777.0), ("width", 333.0), ("height", 251.0)] {
            let (w, h, _) = target_size(4032, 3024, &sz(mode, value));
            let err = (w as f64 / h as f64 * 3024.0 - 4032.0).abs();
            assert!(err <= 4032.0 / h as f64 + 1.0, "{mode} {value}: err {err}");
        }
    }

    #[test]
    fn predict_with_sr() {
        let is = sz("long_edge", 1000.0);
        assert_eq!(predict_output_size(600, 400, 2, &is).0, 1000);
        assert_eq!(predict_output_size(600, 400, 2, &is).1, 667);
        let off = ImageSizeSettings::default();
        let (w, h, warn) = predict_output_size(6000, 4000, 4, &off);
        assert_eq!((w, h), (6000, 4000));
        assert!(!warn.is_empty());
    }
}
