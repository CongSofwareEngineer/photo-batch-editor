//! Khác biệt hệ điều hành (Windows 10/11, macOS, Linux) gom về một chỗ. Không Qt.
//! Port của `core/system.py`.

pub const IS_WINDOWS: bool = cfg!(target_os = "windows");
pub const IS_MAC: bool = cfg!(target_os = "macos");

/// Font mặc định của text layer mới (photo editor) và chữ video: có sẵn trên mọi máy
/// của hệ điều hành đó và có dấu tiếng Việt.
pub const DEFAULT_FONT_FAMILY: &str = if cfg!(target_os = "windows") {
    "Segoe UI"
} else if cfg!(target_os = "macos") {
    "Helvetica Neue"
} else {
    "DejaVu Sans"
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_font_matches_os() {
        let expected = if cfg!(target_os = "windows") {
            "Segoe UI"
        } else if cfg!(target_os = "macos") {
            "Helvetica Neue"
        } else {
            "DejaVu Sans"
        };
        assert_eq!(DEFAULT_FONT_FAMILY, expected);
    }
}
