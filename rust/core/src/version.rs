//! Phiên bản app: `version.json` ở gốc dự án. Port của `core/version.py`.

use std::path::Path;

use crate::paths::resource_path;

pub const VERSION_FILE: &str = "version.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppVersion {
    pub version: String,
    pub build: i64,
}

impl Default for AppVersion {
    fn default() -> Self {
        AppVersion {
            version: "0.0.0".to_string(),
            build: 0,
        }
    }
}

impl AppVersion {
    pub fn new(version: impl Into<String>, build: i64) -> Self {
        AppVersion {
            version: version.into(),
            build,
        }
    }

    /// `(X, Y, Z, build)` cho tài nguyên file-version của Windows.
    pub fn numbers(&self) -> (i64, i64, i64, i64) {
        let mut parts: Vec<i64> = self
            .version
            .split('.')
            .take(3)
            .map(|p| if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() {
                p.parse::<i64>().unwrap_or(0)
            } else {
                0
            })
            .collect();
        while parts.len() < 3 {
            parts.push(0);
        }
        (parts[0], parts[1], parts[2], self.build)
    }

    /// `1.0.0.12`
    pub fn full(&self) -> String {
        let (a, b, c, d) = self.numbers();
        format!("{a}.{b}.{c}.{d}")
    }
}

/// Phiên bản app đang chạy; `0.0.0` nếu file thiếu / hỏng.
pub fn read_version(path: Option<&Path>) -> AppVersion {
    let p = match path {
        Some(p) => p.to_path_buf(),
        None => resource_path([VERSION_FILE]),
    };
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(_) => return AppVersion::default(),
    };
    let data: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return AppVersion::default(),
    };
    let obj = match data.as_object() {
        Some(o) => o,
        None => return AppVersion::default(),
    };
    let version = match obj.get("version") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => "0.0.0".to_string(),
    };
    let build = match obj.get("build") {
        Some(serde_json::Value::Number(n)) => match n.as_i64() {
            Some(i) => i,
            None => return AppVersion::default(),
        },
        None => 0,
        Some(_) => return AppVersion::default(),
    };
    AppVersion { version, build }
}

pub fn write_version(path: &Path, ver: &AppVersion) -> std::io::Result<()> {
    let data = serde_json::json!({ "version": ver.version, "build": ver.build });
    let mut text = serde_json::to_string_pretty(&data).expect("serialize version");
    text.push('\n');
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_full() {
        assert_eq!(AppVersion::new("1.2.3", 45).numbers(), (1, 2, 3, 45));
        assert_eq!(AppVersion::new("2.0", 7).full(), "2.0.0.7");
    }

    #[test]
    fn repo_version_file_is_valid() {
        let ver = read_version(Some(&resource_path([VERSION_FILE])));
        assert_eq!(ver.version.matches('.').count(), 2);
        assert!(ver.build >= 0);
        assert_eq!(ver.full().matches('.').count(), 3);
    }

    #[test]
    fn missing_or_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read_version(Some(&dir.path().join("nope.json"))),
            AppVersion::default()
        );
        let bad = dir.path().join("v.json");
        std::fs::write(&bad, "{oops").unwrap();
        assert_eq!(read_version(Some(&bad)), AppVersion::default());
    }

    #[test]
    fn write_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("v.json");
        write_version(&f, &AppVersion::new("1.4.0", 9)).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"version": "1.4.0", "build": 9}));
        assert_eq!(read_version(Some(&f)), AppVersion::new("1.4.0", 9));
    }
}
