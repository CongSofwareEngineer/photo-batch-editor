//! Đường dẫn tài nguyên và dữ liệu, chạy được cả khi chạy từ mã nguồn lẫn bản build.
//! Port của `core/paths.py` (phần không liên quan CUDA DLL, vốn chỉ dành cho Windows + bản build).

use std::env;
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "PhotoBatchEditor";

/// Thư mục chứa tài nguyên bundle. Khi chạy từ mã nguồn Rust: gốc dự án (thư mục chứa
/// `rust/`), suy ra từ `CARGO_MANIFEST_DIR` (= `<root>/rust/core`).
pub fn app_root() -> PathBuf {
    if let Ok(p) = env::var("PBE_APP_ROOT") {
        return PathBuf::from(p);
    }
    // <root>/rust/core -> <root>
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent() // <root>/rust
        .and_then(|p| p.parent()) // <root>
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest)
}

/// Đường dẫn tuyệt đối của một tài nguyên bundle, vd. `resource_path(["models", "x.onnx"])`.
pub fn resource_path<I, S>(parts: I) -> PathBuf
where
    I: IntoIterator<Item = S>,
    S: AsRef<Path>,
{
    let mut p = app_root();
    for part in parts {
        p.push(part);
    }
    p
}

fn home_dir() -> PathBuf {
    if let Ok(h) = env::var("HOME") {
        return PathBuf::from(h);
    }
    if let Ok(up) = env::var("USERPROFILE") {
        return PathBuf::from(up);
    }
    PathBuf::from(".")
}

/// `%LOCALAPPDATA%\PhotoBatchEditor` trên Windows, `~/Library/Caches/PhotoBatchEditor`
/// trên macOS, `~/.cache/PhotoBatchEditor` nơi khác.
pub fn local_appdata_dir() -> PathBuf {
    let root = if let Ok(base) = env::var("LOCALAPPDATA") {
        PathBuf::from(base)
    } else if cfg!(target_os = "macos") {
        home_dir().join("Library").join("Caches")
    } else {
        home_dir().join(".cache")
    };
    root.join(APP_NAME)
}

/// Thư mục dữ liệu người dùng (auth.json, state.json, presets) — **cùng chỗ với bản Python**
/// (`QStandardPaths.AppDataLocation`), để hai bản dùng chung tài khoản và thiết lập đã lưu:
///
/// * Windows: `%APPDATA%\PhotoBatchEditor`
/// * macOS: `~/Library/Application Support/PhotoBatchEditor`
/// * nơi khác: `$XDG_DATA_HOME/PhotoBatchEditor` hoặc `~/.local/share/PhotoBatchEditor`
///
/// Khác [`local_appdata_dir`] (chỗ đệm, có thể xoá mất).
pub fn app_data_dir() -> PathBuf {
    if let Ok(p) = env::var("PBE_APP_DATA") {
        return PathBuf::from(p);
    }
    let root = if cfg!(target_os = "windows") {
        match env::var("APPDATA") {
            Ok(base) => PathBuf::from(base),
            Err(_) => home_dir().join("AppData").join("Roaming"),
        }
    } else if cfg!(target_os = "macos") {
        home_dir().join("Library").join("Application Support")
    } else {
        match env::var("XDG_DATA_HOME") {
            Ok(base) if !base.is_empty() => PathBuf::from(base),
            _ => home_dir().join(".local").join("share"),
        }
    };
    root.join(APP_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_path_joins_under_root() {
        let p = resource_path(["models", "x.onnx"]);
        assert!(p.ends_with("models/x.onnx"));
        assert_eq!(p, app_root().join("models").join("x.onnx"));
    }

    #[test]
    fn app_data_dir_is_overridable() {
        let prev = env::var("PBE_APP_DATA").ok();
        env::set_var("PBE_APP_DATA", "/tmp/pbe-data");
        assert_eq!(app_data_dir(), PathBuf::from("/tmp/pbe-data"));
        match prev {
            Some(v) => env::set_var("PBE_APP_DATA", v),
            None => env::remove_var("PBE_APP_DATA"),
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn mac_app_data_dir() {
        // Phải trùng `QStandardPaths.AppDataLocation` của bản Python.
        let prev_data = env::var("PBE_APP_DATA").ok();
        let prev_home = env::var("HOME").ok();
        env::remove_var("PBE_APP_DATA");
        env::set_var("HOME", "/tmp/fakehome");
        assert_eq!(
            app_data_dir(),
            PathBuf::from("/tmp/fakehome/Library/Application Support/PhotoBatchEditor")
        );
        if let Some(v) = prev_data {
            env::set_var("PBE_APP_DATA", v);
        }
        if let Some(v) = prev_home {
            env::set_var("HOME", v);
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn mac_cache_dir() {
        // Giống test_mac_cache_dir: không có LOCALAPPDATA -> ~/Library/Caches/PhotoBatchEditor.
        let prev_local = env::var("LOCALAPPDATA").ok();
        let prev_home = env::var("HOME").ok();
        env::remove_var("LOCALAPPDATA");
        env::set_var("HOME", "/tmp/fakehome");
        assert_eq!(
            local_appdata_dir(),
            PathBuf::from("/tmp/fakehome/Library/Caches/PhotoBatchEditor")
        );
        if let Some(v) = prev_local {
            env::set_var("LOCALAPPDATA", v);
        }
        if let Some(v) = prev_home {
            env::set_var("HOME", v);
        }
    }
}
