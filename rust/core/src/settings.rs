//! Thông số chỉnh sửa, giới hạn tham số và đọc/ghi JSON. Port của `core/settings.py`.

use std::path::Path;

use serde_json::{json, Map, Value};

pub const MAX_OUTPUT_LONG_EDGE: i64 = 20_000; // px
pub const MAX_OUTPUT_MEGAPIXELS: i64 = 200; // MP
pub const PRESET_FORMAT_VERSION: i64 = 2;

/// Phạm vi và metadata UI cho một thông số số (đặt tên kiểu Camera Raw).
#[derive(Debug, Clone, Copy)]
pub struct SliderSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub panel: &'static str,
    pub group: &'static str,
    pub minimum: f64,
    pub maximum: f64,
    pub step: f64,
    pub default: f64,
    pub decimals: usize,
}

const fn s(
    key: &'static str,
    label: &'static str,
    panel: &'static str,
    group: &'static str,
    minimum: f64,
    maximum: f64,
    step: f64,
    decimals: usize,
) -> SliderSpec {
    SliderSpec {
        key,
        label,
        panel,
        group,
        minimum,
        maximum,
        step,
        default: 0.0,
        decimals,
    }
}

pub const SLIDERS: [SliderSpec; 15] = [
    s("temperature", "Temperature", "Basic", "White Balance", -100.0, 100.0, 1.0, 0),
    s("tint", "Tint", "Basic", "White Balance", -100.0, 100.0, 1.0, 0),
    s("exposure", "Exposure", "Basic", "Tone", -5.0, 5.0, 0.05, 2),
    s("brightness", "Brightness", "Basic", "Tone", -100.0, 100.0, 1.0, 0),
    s("contrast", "Contrast", "Basic", "Tone", -100.0, 100.0, 1.0, 0),
    s("highlights", "Highlights", "Basic", "Tone", -100.0, 100.0, 1.0, 0),
    s("shadows", "Shadows", "Basic", "Tone", -100.0, 100.0, 1.0, 0),
    s("whites", "Whites", "Basic", "Tone", -100.0, 100.0, 1.0, 0),
    s("blacks", "Blacks", "Basic", "Tone", -100.0, 100.0, 1.0, 0),
    s("clarity", "Clarity", "Basic", "Presence", -100.0, 100.0, 1.0, 0),
    s("vibrance", "Vibrance", "Basic", "Presence", -100.0, 100.0, 1.0, 0),
    s("saturation", "Saturation", "Basic", "Presence", -100.0, 100.0, 1.0, 0),
    s("sharpening_amount", "Amount", "Detail", "Sharpening", 0.0, 150.0, 1.0, 0),
    s("noise_reduction", "Noise Reduction", "Detail", "Noise Reduction", 0.0, 100.0, 1.0, 0),
    s("vignette_amount", "Amount", "Effects", "Post-Crop Vignetting", -100.0, 100.0, 1.0, 0),
];

pub fn slider_by_key(key: &str) -> Option<&'static SliderSpec> {
    SLIDERS.iter().find(|s| s.key == key)
}

pub const SUPER_RESOLUTION_OPTIONS: [&str; 3] = ["off", "2x", "4x"];

pub fn super_resolution_factor(option: &str) -> i64 {
    match option {
        "2x" => 2,
        "4x" => 4,
        _ => 1,
    }
}

/// Các mode Image Size theo thứ tự, kèm nhãn tiếng Anh.
pub const IMAGE_SIZE_MODES: [(&str, &str); 5] = [
    ("off", "Off"),
    ("percent", "Percent"),
    ("long_edge", "Long Edge"),
    ("width", "Width"),
    ("height", "Height"),
];

pub const RESAMPLE_OPTIONS: [(&str, &str); 7] = [
    ("automatic", "Automatic"),
    ("preserve_details", "Preserve Details (enlargement)"),
    ("bicubic_smoother", "Bicubic Smoother (enlargement)"),
    ("bicubic_sharper", "Bicubic Sharper (reduction)"),
    ("bicubic", "Bicubic (smooth gradients)"),
    ("bilinear", "Bilinear"),
    ("nearest_neighbor", "Nearest Neighbor (hard edges)"),
];

pub const PERCENT_RANGE: (f64, f64) = (1.0, 400.0);
pub const PIXEL_RANGE: (f64, f64) = (16.0, 20_000.0);

fn is_mode(m: &str) -> bool {
    IMAGE_SIZE_MODES.iter().any(|(k, _)| *k == m)
}

fn is_resample(r: &str) -> bool {
    RESAMPLE_OPTIONS.iter().any(|(k, _)| *k == r)
}

fn clamp(value: f64, lo: f64, hi: f64) -> f64 {
    value.max(lo).min(hi)
}

/// Chuyển JSON về f64 hữu hạn, nếu không được thì dùng `default`.
/// Giống `_as_float`: bool -> default; chuỗi parse được -> số; không hữu hạn -> default.
fn as_float(value: Option<&Value>, default: f64) -> f64 {
    let v = match value {
        Some(v) => v,
        None => return default,
    };
    let f = match v {
        Value::Bool(_) => return default,
        Value::Number(n) => match n.as_f64() {
            Some(f) => f,
            None => return default,
        },
        Value::String(sstr) => match sstr.parse::<f64>() {
            Ok(f) => f,
            Err(_) => return default,
        },
        _ => return default,
    };
    if f.is_finite() {
        f
    } else {
        default
    }
}

/// Số JSON: số nguyên khi giá trị là số nguyên, còn lại làm tròn 4 chữ số.
fn number_rounded(v: f64) -> Value {
    if v.fract() == 0.0 && v.is_finite() {
        json!(v as i64)
    } else {
        let r = (v * 10_000.0).round() / 10_000.0;
        json!(r)
    }
}

/// Số JSON: số nguyên khi là số nguyên, còn lại giữ nguyên float (không làm tròn).
fn number_int_or_float(v: f64) -> Value {
    if v.fract() == 0.0 && v.is_finite() {
        json!(v as i64)
    } else {
        json!(v)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageSizeSettings {
    pub mode: String,
    pub value: f64,
    pub resample: String,
    pub dont_enlarge: bool,
}

impl Default for ImageSizeSettings {
    fn default() -> Self {
        ImageSizeSettings {
            mode: "off".to_string(),
            value: 100.0,
            resample: "automatic".to_string(),
            dont_enlarge: false,
        }
    }
}

impl ImageSizeSettings {
    pub fn new(mode: &str, value: f64, resample: &str, dont_enlarge: bool) -> Self {
        ImageSizeSettings {
            mode: mode.to_string(),
            value,
            resample: resample.to_string(),
            dont_enlarge,
        }
    }

    pub fn value_range(&self) -> (f64, f64) {
        if self.mode == "percent" {
            PERCENT_RANGE
        } else {
            PIXEL_RANGE
        }
    }

    pub fn is_default(&self) -> bool {
        self.mode == "off"
    }

    pub fn to_dict(&self) -> Value {
        json!({
            "mode": self.mode,
            "value": number_int_or_float(self.value),
            "resample": self.resample,
            "dont_enlarge": self.dont_enlarge,
        })
    }

    /// Dựng từ dữ liệu không tin cậy: enum sai -> mặc định, value clamp theo mode.
    pub fn from_dict(data: Option<&Value>) -> Self {
        let mut out = ImageSizeSettings::default();
        let obj = match data.and_then(|d| d.as_object()) {
            Some(o) => o,
            None => return out,
        };
        let mode = obj.get("mode").and_then(|v| v.as_str()).unwrap_or(&out.mode);
        out.mode = if is_mode(mode) { mode.to_string() } else { "off".to_string() };
        let resample = obj
            .get("resample")
            .and_then(|v| v.as_str())
            .unwrap_or(&out.resample);
        out.resample = if is_resample(resample) {
            resample.to_string()
        } else {
            "automatic".to_string()
        };
        out.dont_enlarge = obj.get("dont_enlarge").and_then(|v| v.as_bool()).unwrap_or(false);
        let (lo, hi) = out.value_range();
        let default_value = if out.mode == "off" || out.mode == "percent" {
            100.0
        } else {
            2048.0
        };
        out.value = clamp(as_float(obj.get("value"), default_value), lo, hi);
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdjustmentSettings {
    pub temperature: f64,
    pub tint: f64,
    pub exposure: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub highlights: f64,
    pub shadows: f64,
    pub whites: f64,
    pub blacks: f64,
    pub clarity: f64,
    pub vibrance: f64,
    pub saturation: f64,
    pub sharpening_amount: f64,
    pub noise_reduction: f64,
    pub vignette_amount: f64,
    pub super_resolution: String,
    pub image_size: ImageSizeSettings,
}

impl Default for AdjustmentSettings {
    fn default() -> Self {
        AdjustmentSettings {
            temperature: 0.0,
            tint: 0.0,
            exposure: 0.0,
            brightness: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            clarity: 0.0,
            vibrance: 0.0,
            saturation: 0.0,
            sharpening_amount: 0.0,
            noise_reduction: 0.0,
            vignette_amount: 0.0,
            super_resolution: "off".to_string(),
            image_size: ImageSizeSettings::default(),
        }
    }
}

impl AdjustmentSettings {
    pub fn get(&self, key: &str) -> Option<f64> {
        Some(match key {
            "temperature" => self.temperature,
            "tint" => self.tint,
            "exposure" => self.exposure,
            "brightness" => self.brightness,
            "contrast" => self.contrast,
            "highlights" => self.highlights,
            "shadows" => self.shadows,
            "whites" => self.whites,
            "blacks" => self.blacks,
            "clarity" => self.clarity,
            "vibrance" => self.vibrance,
            "saturation" => self.saturation,
            "sharpening_amount" => self.sharpening_amount,
            "noise_reduction" => self.noise_reduction,
            "vignette_amount" => self.vignette_amount,
            _ => return None,
        })
    }

    pub fn set(&mut self, key: &str, v: f64) {
        match key {
            "temperature" => self.temperature = v,
            "tint" => self.tint = v,
            "exposure" => self.exposure = v,
            "brightness" => self.brightness = v,
            "contrast" => self.contrast = v,
            "highlights" => self.highlights = v,
            "shadows" => self.shadows = v,
            "whites" => self.whites = v,
            "blacks" => self.blacks = v,
            "clarity" => self.clarity = v,
            "vibrance" => self.vibrance = v,
            "saturation" => self.saturation = v,
            "sharpening_amount" => self.sharpening_amount = v,
            "noise_reduction" => self.noise_reduction = v,
            "vignette_amount" => self.vignette_amount = v,
            _ => {}
        }
    }

    pub fn sr_factor(&self) -> i64 {
        super_resolution_factor(&self.super_resolution)
    }

    pub fn is_default(&self) -> bool {
        SLIDERS
            .iter()
            .all(|spec| self.get(spec.key).unwrap() == spec.default)
            && self.super_resolution == "off"
            && self.image_size.is_default()
    }

    pub fn to_dict(&self) -> Value {
        let mut out = Map::new();
        for spec in SLIDERS.iter() {
            out.insert(spec.key.to_string(), number_rounded(self.get(spec.key).unwrap()));
        }
        out.insert("super_resolution".to_string(), json!(self.super_resolution));
        out.insert("image_size".to_string(), self.image_size.to_dict());
        Value::Object(out)
    }

    /// Dựng từ dữ liệu không tin cậy: thiếu key -> mặc định, key lạ -> bỏ qua,
    /// ngoài phạm vi -> clamp, enum sai -> mặc định.
    pub fn from_dict(data: &Value) -> Self {
        let mut out = AdjustmentSettings::default();
        let obj = match data.as_object() {
            Some(o) => o,
            None => return out,
        };
        for spec in SLIDERS.iter() {
            if obj.contains_key(spec.key) {
                let v = as_float(obj.get(spec.key), spec.default);
                out.set(spec.key, clamp(v, spec.minimum, spec.maximum));
            }
        }
        let sr = obj.get("super_resolution").and_then(|v| v.as_str()).unwrap_or("off");
        out.super_resolution = if SUPER_RESOLUTION_OPTIONS.contains(&sr) {
            sr.to_string()
        } else {
            "off".to_string()
        };
        out.image_size = ImageSizeSettings::from_dict(obj.get("image_size"));
        out
    }
}

pub fn setting_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = SLIDERS.iter().map(|s| s.key).collect();
    keys.push("super_resolution");
    keys.push("image_size");
    keys
}

/// Ghi settings theo định dạng file preset.
pub fn save_settings_json(path: &Path, settings: &AdjustmentSettings, name: &str) -> std::io::Result<()> {
    let payload = json!({
        "version": PRESET_FORMAT_VERSION,
        "name": name,
        "settings": settings.to_dict(),
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&payload).expect("serialize settings");
    let tmp = path.with_file_name(format!(
        "{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("preset.json")
    ));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[derive(Debug)]
pub struct SettingsFileError(pub String);

impl std::fmt::Display for SettingsFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for SettingsFileError {}

/// Đọc file preset/settings. Trả về `(name, settings)`.
pub fn load_settings_json(path: &Path) -> Result<(String, AdjustmentSettings), SettingsFileError> {
    let text = std::fs::read_to_string(path).map_err(|e| SettingsFileError(e.to_string()))?;
    let data: Value = serde_json::from_str(&text).map_err(|e| SettingsFileError(e.to_string()))?;
    let obj = data
        .as_object()
        .ok_or_else(|| SettingsFileError(format!("Invalid settings file: {}", path.display())))?;
    let raw = obj.get("settings").cloned().unwrap_or_else(|| data.clone());
    let name = match obj.get("name") {
        Some(Value::String(s)) => s.clone(),
        _ => path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(),
    };
    Ok((name, AdjustmentSettings::from_dict(&raw)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_default() {
        assert!(AdjustmentSettings::default().is_default());
        assert!(ImageSizeSettings::default().is_default());
    }

    #[test]
    fn to_dict_roundtrips() {
        let mut s = AdjustmentSettings {
            exposure: 0.35,
            super_resolution: "2x".to_string(),
            ..Default::default()
        };
        s.image_size.mode = "width".to_string();
        s.image_size.value = 1200.0;
        let d = s.to_dict();
        let back = AdjustmentSettings::from_dict(&d);
        assert_eq!(back.to_dict(), d);
        assert_eq!(back, s);
    }

    #[test]
    fn missing_keys_default() {
        let s = AdjustmentSettings::from_dict(&json!({"exposure": 1.5}));
        assert_eq!(s.exposure, 1.5);
        assert_eq!(s.contrast, 0.0);
        assert_eq!(s.super_resolution, "off");
        assert_eq!(s.image_size.mode, "off");
    }

    #[test]
    fn out_of_range_clamped() {
        let s = AdjustmentSettings::from_dict(&json!({
            "exposure": 9, "contrast": -500, "sharpening_amount": -3,
            "image_size": {"mode": "percent", "value": 1000}
        }));
        assert_eq!(s.exposure, 5.0);
        assert_eq!(s.contrast, -100.0);
        assert_eq!(s.sharpening_amount, 0.0);
        assert_eq!(s.image_size.value, 400.0);
    }

    #[test]
    fn invalid_enums_default() {
        let s = AdjustmentSettings::from_dict(&json!({
            "super_resolution": "8x", "contrast": "abc",
            "image_size": {"mode": "huge", "resample": "magic", "value": 50}
        }));
        assert_eq!(s.super_resolution, "off");
        assert_eq!(s.contrast, 0.0);
        assert_eq!(s.image_size.mode, "off");
        assert_eq!(s.image_size.resample, "automatic");
        let s = AdjustmentSettings::from_dict(&json!({
            "image_size": {"mode": "width", "resample": "lanczos"}
        }));
        assert_eq!(s.image_size.mode, "width");
        assert_eq!(s.image_size.resample, "automatic");
    }

    #[test]
    fn as_float_rejects_bool() {
        assert_eq!(as_float(Some(&json!(true)), 7.0), 7.0);
        assert_eq!(as_float(Some(&json!("3.5")), 0.0), 3.5);
    }
}
