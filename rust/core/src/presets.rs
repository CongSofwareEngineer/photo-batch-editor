//! Preset có sẵn và của người dùng. Port của `core/presets.py`.

use std::path::{Path, PathBuf};

use regex::Regex;
use std::sync::LazyLock;

use crate::i18n::{tr, tr_args};
use crate::paths::resource_path;
use crate::settings::{
    load_settings_json, save_settings_json, AdjustmentSettings, IMAGE_SIZE_MODES, SLIDERS,
};

pub const BUILTIN_ORDER: [&str; 6] = [
    "default",
    "bright_clean",
    "warm",
    "high_contrast",
    "black_white",
    "web_export",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: String,
    pub settings: AdjustmentSettings,
    pub path: Option<PathBuf>,
    pub builtin: bool,
}

#[derive(Debug)]
pub struct PresetError(pub String);

impl std::fmt::Display for PresetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for PresetError {}

/// Tên hiển thị: mẫu có sẵn được dịch, thiết lập người dùng thì không.
pub fn display_name(p: &Preset) -> String {
    if p.builtin {
        tr(&p.name)
    } else {
        p.name.clone()
    }
}

pub fn builtin_dir() -> PathBuf {
    resource_path(["presets_builtin"])
}

pub fn load_preset(path: &Path, builtin: bool) -> Result<Preset, PresetError> {
    let (name, settings) = load_settings_json(path).map_err(|e| PresetError(e.to_string()))?;
    Ok(Preset {
        name,
        settings,
        path: Some(path.to_path_buf()),
        builtin,
    })
}

fn load_dir(folder: &Path, builtin: bool) -> Vec<Preset> {
    let mut out = vec![];
    let entries = match std::fs::read_dir(folder) {
        Ok(it) => it,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match load_preset(&path, builtin) {
            Ok(p) => out.push(p),
            Err(_) => continue, // preset hỏng không được làm sập app
        }
    }
    out
}

pub fn list_builtin_presets() -> Vec<Preset> {
    let mut presets = load_dir(&builtin_dir(), true);
    let rank = |p: &Preset| -> usize {
        let stem = p
            .path
            .as_ref()
            .and_then(|path| path.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("");
        BUILTIN_ORDER.iter().position(|k| *k == stem).unwrap_or(99)
    };
    presets.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    presets
}

pub fn list_user_presets(user_dir: &Path) -> Vec<Preset> {
    let mut presets = load_dir(user_dir, false);
    presets.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    presets
}

static INVALID_FILENAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[<>:"/\\|?*\x00-\x1f]"#).unwrap());

fn safe_filename(name: &str) -> String {
    let cleaned = INVALID_FILENAME.replace_all(name, "_");
    let cleaned = cleaned.trim_matches(|c| c == ' ' || c == '.');
    let cut: String = cleaned.chars().take(80).collect();
    if cut.is_empty() {
        "preset".to_string()
    } else {
        cut
    }
}

/// Lưu một preset người dùng. Không có `path` thì chọn tên file chưa dùng.
pub fn save_user_preset(
    user_dir: &Path,
    name: &str,
    settings: &AdjustmentSettings,
    path: Option<&Path>,
) -> Result<PathBuf, PresetError> {
    let target = match path {
        Some(p) => p.to_path_buf(),
        None => {
            let base = safe_filename(name);
            let mut p = user_dir.join(format!("{base}.json"));
            let mut n = 2;
            while p.exists() {
                p = user_dir.join(format!("{base} ({n}).json"));
                n += 1;
            }
            p
        }
    };
    save_settings_json(&target, settings, name).map_err(|e| PresetError(e.to_string()))?;
    Ok(target)
}

pub fn delete_user_preset(preset: &Preset) -> Result<(), PresetError> {
    if preset.builtin || preset.path.is_none() {
        return Err(PresetError("Built-in presets cannot be deleted".into()));
    }
    let path = preset.path.as_ref().unwrap();
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(PresetError(e.to_string())),
    }
}

/// Đổi tên preset người dùng tại chỗ (file giữ nguyên tên, chỉ đổi tên lưu bên trong).
pub fn rename_user_preset(preset: &Preset, new_name: &str) -> Result<Preset, PresetError> {
    let new_name = new_name.trim();
    if preset.builtin || preset.path.is_none() {
        return Err(PresetError("Built-in presets cannot be renamed".into()));
    }
    if new_name.is_empty() {
        return Err(PresetError("The name cannot be empty".into()));
    }
    let path = preset.path.as_ref().unwrap();
    save_settings_json(path, &preset.settings, new_name).map_err(|e| PresetError(e.to_string()))?;
    Ok(Preset {
        name: new_name.to_string(),
        settings: preset.settings.clone(),
        path: Some(path.clone()),
        builtin: false,
    })
}

static PAREN_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\(\d+\)$").unwrap());

/// `name`, hoặc `name (2)`, `name (3)`… để khác (không phân biệt hoa thường) với `existing`.
pub fn unique_name<'a, I>(name: &str, existing: I) -> String
where
    I: IntoIterator<Item = &'a str>,
{
    let name = name.trim();
    let name = if name.is_empty() { "Setting" } else { name };
    let taken: std::collections::HashSet<String> =
        existing.into_iter().map(|n| n.to_lowercase()).collect();
    if !taken.contains(&name.to_lowercase()) {
        return name.to_string();
    }
    let stripped = PAREN_SUFFIX.replace(name, "").to_string();
    let base = if stripped.is_empty() { name.to_string() } else { stripped };
    let mut n = 2;
    while taken.contains(&format!("{base} ({n})").to_lowercase()) {
        n += 1;
    }
    format!("{base} ({n})")
}

/// Nhãn ngắn cho phần tóm tắt: vài slider dùng chung nhãn Camera Raw "Amount".
fn short_label(key: &str) -> Option<&'static str> {
    match key {
        "sharpening_amount" => Some("Sharpening"),
        "vignette_amount" => Some("Vignette"),
        "noise_reduction" => Some("Noise Red."),
        _ => None,
    }
}

fn fmt_g(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn image_size_mode_label(mode: &str) -> &'static str {
    IMAGE_SIZE_MODES
        .iter()
        .find(|(k, _)| *k == mode)
        .map(|(_, l)| *l)
        .unwrap_or("Off")
}

/// Tóm tắt dễ đọc các giá trị khác mặc định, vd. `["Exposure +0.30", "Contrast +15"]`
/// (theo ngôn ngữ UI).
pub fn describe_settings(s: &AdjustmentSettings) -> Vec<String> {
    let mut out = vec![];
    for spec in SLIDERS.iter() {
        let v = s.get(spec.key).unwrap();
        if (v - spec.default).abs() < 1e-9 {
            continue;
        }
        let label = tr(short_label(spec.key).unwrap_or(spec.label));
        let sign = if v > 0.0 && spec.minimum < 0.0 { "+" } else { "" };
        out.push(format!("{} {}{:.*}", label, sign, spec.decimals, v));
    }
    if s.super_resolution != "off" {
        out.push(tr_args("Super Res {factor}", &[("factor", &s.super_resolution)]));
    }
    if !s.image_size.is_default() {
        let unit = if s.image_size.mode == "percent" { "%" } else { " px" };
        let value = format!("{}{}", fmt_g(s.image_size.value), unit);
        let mode = tr(image_size_mode_label(&s.image_size.mode));
        out.push(tr_args("Resize {mode} {value}", &[("mode", &mode), ("value", &value)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{set_language, TEST_LANG_LOCK};
    use crate::settings::ImageSizeSettings;
    use serde_json::json;

    #[test]
    fn builtin_presets_load() {
        let presets = list_builtin_presets();
        let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Default", "Bright & Clean", "Warm", "High Contrast", "Black & White", "Web Export"]
        );
        let by_name = |n: &str| presets.iter().find(|p| p.name == n).unwrap().settings.clone();
        assert!(by_name("Default").is_default());
        let b = by_name("Bright & Clean");
        assert_eq!((b.exposure, b.shadows, b.highlights, b.vibrance), (0.3, 30.0, -20.0, 15.0));
        let w = by_name("Warm");
        assert_eq!((w.temperature, w.tint, w.vibrance), (25.0, 5.0, 10.0));
        let hc = by_name("High Contrast");
        assert_eq!((hc.contrast, hc.clarity, hc.blacks, hc.whites), (30.0, 25.0, -15.0, 10.0));
        let bw = by_name("Black & White");
        assert_eq!((bw.saturation, bw.contrast, bw.clarity), (-100.0, 20.0, 15.0));
        let web = by_name("Web Export");
        assert_eq!(web.image_size.mode, "long_edge");
        assert_eq!(web.image_size.value, 2048.0);
        assert_eq!(web.image_size.resample, "bicubic_sharper");
        assert_eq!(web.sharpening_amount, 25.0);
        assert!(presets.iter().all(|p| p.builtin));
    }

    fn write(tmp: &Path, settings: serde_json::Value) -> PathBuf {
        let p = tmp.join("p.json");
        std::fs::write(&p, json!({"version": 2, "name": "Test", "settings": settings}).to_string())
            .unwrap();
        p
    }

    #[test]
    fn missing_keys_default() {
        let dir = tempfile::tempdir().unwrap();
        let s = load_preset(&write(dir.path(), json!({"exposure": 1.5})), false)
            .unwrap()
            .settings;
        assert_eq!(s.exposure, 1.5);
        assert_eq!(s.contrast, 0.0);
        assert_eq!(s.super_resolution, "off");
        assert_eq!(s.image_size.mode, "off");
    }

    #[test]
    fn user_preset_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = AdjustmentSettings {
            exposure: 0.35,
            super_resolution: "2x".to_string(),
            ..Default::default()
        };
        s.image_size.mode = "width".to_string();
        s.image_size.value = 1200.0;
        let path = save_user_preset(dir.path(), "My: Look", &s, None).unwrap();
        assert!(path.exists());
        assert!(!path.file_name().unwrap().to_string_lossy().contains(':'));
        let list = list_user_presets(dir.path());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "My: Look");
        assert_eq!(list[0].settings.to_dict(), s.to_dict());
        assert!(!list[0].builtin);
        let path2 = save_user_preset(dir.path(), "My: Look", &s, None).unwrap();
        assert_ne!(path2, path);
        delete_user_preset(&list[0]).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn rename_preset() {
        let dir = tempfile::tempdir().unwrap();
        let s = AdjustmentSettings { contrast: 12.0, ..Default::default() };
        let path = save_user_preset(dir.path(), "Old", &s, None).unwrap();
        let list = list_user_presets(dir.path());
        let q = rename_user_preset(&list[0], "  New name ").unwrap();
        assert_eq!(q.name, "New name");
        assert_eq!(q.path.as_deref(), Some(path.as_path()));
        let list = list_user_presets(dir.path());
        assert_eq!(list[0].name, "New name");
        assert_eq!(list[0].settings.contrast, 12.0);
        let builtin = &list_builtin_presets()[0];
        assert!(rename_user_preset(builtin, "x").is_err());
        assert!(rename_user_preset(&list[0], "   ").is_err());
    }

    #[test]
    fn unique_name_cases() {
        assert_eq!(unique_name("Warm", ["Default"]), "Warm");
        assert_eq!(unique_name("warm", ["Warm"]), "warm (2)");
        assert_eq!(unique_name("Warm", ["Warm", "Warm (2)"]), "Warm (3)");
        assert_eq!(unique_name("Warm (2)", ["Warm", "Warm (2)"]), "Warm (3)");
        assert_eq!(unique_name("  ", std::iter::empty()), "Setting");
    }

    #[test]
    fn describe() {
        let _guard = TEST_LANG_LOCK.lock().unwrap();
        set_language("en");
        assert!(describe_settings(&AdjustmentSettings::default()).is_empty());
        let mut s = AdjustmentSettings {
            exposure: 0.3,
            contrast: -15.0,
            sharpening_amount: 40.0,
            super_resolution: "2x".to_string(),
            ..Default::default()
        };
        s.image_size = ImageSizeSettings::new("long_edge", 2048.0, "automatic", false);
        assert_eq!(
            describe_settings(&s),
            vec![
                "Exposure +0.30",
                "Contrast -15",
                "Sharpening 40",
                "Super Res 2x",
                "Resize Long Edge 2048 px",
            ]
        );
    }

    #[test]
    fn describe_follows_language() {
        let _guard = TEST_LANG_LOCK.lock().unwrap();
        let s = AdjustmentSettings { exposure: 0.3, ..Default::default() };
        set_language("vi");
        assert_eq!(describe_settings(&s), vec!["Phơi sáng +0.30"]);
        set_language("en");
        assert_eq!(describe_settings(&s), vec!["Exposure +0.30"]);
    }
}
