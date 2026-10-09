//! Ngôn ngữ UI (Anh / Việt). Port của `core/i18n.py`.
//!
//! Bản thân văn bản tiếng Anh là khoá: `tr("Choose Folder")` trả về nguyên văn khi tiếng Anh,
//! và tra trong [`crate::i18n_vi`] khi tiếng Việt (thiếu mục thì fallback về tiếng Anh).

use std::collections::HashSet;
use std::path::Path;
use std::sync::RwLock;

use serde_json::{json, Value};

pub const LANGUAGES: [(&str, &str); 2] = [("en", "English"), ("vi", "Tiếng Việt")];
pub const DEFAULT_LANGUAGE: &str = "en";
pub const PREFS_FILE: &str = "preferences.json";

static LANG: RwLock<String> = RwLock::new(String::new());

/// Khoá dùng chung cho các test chạm trạng thái ngôn ngữ toàn cục, tránh race khi chạy song song.
#[cfg(test)]
pub static TEST_LANG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn is_language(code: &str) -> bool {
    LANGUAGES.iter().any(|(k, _)| *k == code)
}

pub fn language() -> String {
    let g = LANG.read().unwrap();
    if g.is_empty() {
        DEFAULT_LANGUAGE.to_string()
    } else {
        g.clone()
    }
}

pub fn set_language(code: &str) {
    let lang = if is_language(code) { code } else { DEFAULT_LANGUAGE };
    *LANG.write().unwrap() = lang.to_string();
}

fn catalog_lookup(text: &str) -> String {
    if language() == "vi" {
        crate::i18n_vi::text()
            .get(text)
            .cloned()
            .unwrap_or_else(|| text.to_string())
    } else {
        text.to_string()
    }
}

/// `tr` không tham số.
pub fn tr(text: &str) -> String {
    catalog_lookup(text)
}

/// `tr` có tham số `{name}`.
pub fn tr_args(text: &str, args: &[(&str, &str)]) -> String {
    let s = catalog_lookup(text);
    format_with(&s, args)
}

/// `tr` nhận biết số nhiều; `{n}` được điền. Catalog keyed theo dạng số nhiều.
pub fn trn(singular: &str, plural: &str, n: i64) -> String {
    let key = if n != 1 { plural } else { singular };
    let s = if language() == "vi" {
        crate::i18n_vi::text()
            .get(plural)
            .cloned()
            .unwrap_or_else(|| key.to_string())
    } else {
        key.to_string()
    };
    let n_str = n.to_string();
    format_with(&s, &[("n", n_str.as_str())])
}

/// Dịch một message đến từ `core` (các cảnh báo nối bằng `"; "`).
pub fn tr_msg(message: &str) -> String {
    if message.is_empty() || language() != "vi" {
        return message.to_string();
    }
    message
        .split("; ")
        .map(tr_one)
        .collect::<Vec<_>>()
        .join("; ")
}

fn tr_one(part: &str) -> String {
    if let Some(v) = crate::i18n_vi::text().get(part) {
        return v.clone();
    }
    for (re, template) in crate::i18n_vi::message_patterns() {
        if let Some(caps) = re.captures(part) {
            let mut out = template.clone();
            for i in 1..caps.len() {
                let g = caps.get(i).map(|m| m.as_str()).unwrap_or("");
                let translated = if g.is_empty() { g.to_string() } else { tr_one(g) };
                out = out.replace(&format!("{{{}}}", i - 1), &translated);
            }
            return out;
        }
    }
    part.to_string()
}

/// Thay `{name}` bằng giá trị tương ứng (bỏ qua phần format spec sau `:` / `!`).
fn format_with(s: &str, args: &[(&str, &str)]) -> String {
    if args.is_empty() {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '{' {
            if let Some((_, '{')) = chars.peek() {
                chars.next();
                out.push('{');
                continue;
            }
            // đọc tới '}'
            let mut token = String::new();
            let mut closed = false;
            for (_, tc) in chars.by_ref() {
                if tc == '}' {
                    closed = true;
                    break;
                }
                token.push(tc);
            }
            if !closed {
                out.push('{');
                out.push_str(&token);
                continue;
            }
            let name = token.split([':', '!']).next().unwrap_or("");
            if let Some((_, val)) = args.iter().find(|(k, _)| *k == name) {
                out.push_str(val);
            } else {
                out.push('{');
                out.push_str(&token);
                out.push('}');
            }
        } else if c == '}' {
            if let Some((_, '}')) = chars.peek() {
                chars.next();
            }
            out.push('}');
        } else {
            out.push(c);
        }
    }
    out
}

/// Tên các placeholder `{name}` trong chuỗi (bỏ format spec) — dùng cho test khớp placeholder.
pub fn format_fields(s: &str) -> HashSet<String> {
    let mut fields = HashSet::new();
    let mut chars = s.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '{' {
            if let Some((_, '{')) = chars.peek() {
                chars.next();
                continue;
            }
            let mut token = String::new();
            let mut closed = false;
            for (_, tc) in chars.by_ref() {
                if tc == '}' {
                    closed = true;
                    break;
                }
                token.push(tc);
            }
            if closed {
                let name = token.split([':', '!']).next().unwrap_or("").to_string();
                fields.insert(name);
            }
        } else if c == '}' {
            if let Some((_, '}')) = chars.peek() {
                chars.next();
            }
        }
    }
    fields
}

pub fn system_language() -> String {
    // Không có Qt trong core; mặc định tiếng Anh (như core/i18n.py khi không có PySide).
    DEFAULT_LANGUAGE.to_string()
}

/// Ngôn ngữ đã lưu, hoặc ngôn ngữ hệ thống lần đầu.
pub fn load_language(data_dir: &Path) -> String {
    let code = std::fs::read_to_string(data_dir.join(PREFS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("language").and_then(|l| l.as_str()).map(|s| s.to_string()));
    match code {
        Some(c) if is_language(&c) => c,
        _ => system_language(),
    }
}

pub fn save_language(data_dir: &Path, code: &str) {
    let path = data_dir.join(PREFS_FILE);
    let mut prefs = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    prefs.insert("language".to_string(), json!(code));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&Value::Object(prefs)) {
        let tmp = path.with_file_name(format!(
            "{}.tmp",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("preferences.json")
        ));
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        set_language("en");
    }

    #[test]
    fn english_is_identity() {
        let _guard = TEST_LANG_LOCK.lock().unwrap();
        reset();
        assert_eq!(tr("Choose Folder"), "Choose Folder");
        assert_eq!(tr_args("Folder not found: {path}", &[("path", "X")]), "Folder not found: X");
        assert_eq!(trn("{n} photo", "{n} photos", 1), "1 photo");
        assert_eq!(trn("{n} photo", "{n} photos", 3), "3 photos");
        assert_eq!(tr_msg("Processing failed: boom"), "Processing failed: boom");
    }

    #[test]
    fn vietnamese() {
        let _guard = TEST_LANG_LOCK.lock().unwrap();
        set_language("vi");
        assert_eq!(tr("Choose Folder"), "Chọn thư mục");
        assert_eq!(
            tr_args("Folder not found: {path}", &[("path", "X")]),
            "Không tìm thấy thư mục: X"
        );
        assert_eq!(trn("{n} photo", "{n} photos", 1), "1 ảnh");
        assert_eq!(tr("not in the catalog"), "not in the catalog");
        assert_eq!(
            tr_msg("Photo too large for VRAM, processed on CPU; EXIF not kept"),
            "Ảnh quá lớn so với VRAM, đã xử lý bằng CPU; Không giữ được EXIF"
        );
        assert_eq!(tr_msg("Processing failed: boom"), "Xử lý thất bại: boom");
        assert_eq!(
            tr_msg("Saved as a (2).jpg (name collision)"),
            "Đã lưu thành a (2).jpg (trùng tên)"
        );
        assert_eq!(
            tr_msg("Super Resolution skipped: Super Resolution model not found: m.onnx"),
            "Bỏ qua Super Resolution: Không tìm thấy model Super Resolution: m.onnx"
        );
        assert_eq!(tr_msg("something unexpected"), "something unexpected");
        reset();
    }

    #[test]
    fn language_preference_roundtrip() {
        let _guard = TEST_LANG_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        save_language(dir.path(), "vi");
        assert_eq!(load_language(dir.path()), "vi");
        save_language(dir.path(), "en");
        assert_eq!(load_language(dir.path()), "en");
        std::fs::write(dir.path().join(PREFS_FILE), "{broken").unwrap();
        assert!(is_language(&load_language(dir.path())));
        set_language("xx");
        assert_eq!(language(), "en");
        reset();
    }
}
