//! Văn bản UI tiếng Việt. Khoá = văn bản tiếng Anh dùng trong code (xem [`crate::i18n`]).
//!
//! Dữ liệu được sinh từ `core/i18n_vi.py` (nguồn chuẩn) và nhúng dưới dạng JSON để đảm bảo
//! khớp 100% với bản Python. Chạy lại khi sửa `core/i18n_vi.py`:
//! ```text
//! python3 -c "import json; from core import i18n_vi; \
//!   json.dump({'text': i18n_vi.TEXT, 'patterns': [list(p) for p in i18n_vi.MESSAGE_PATTERNS]}, \
//!   open('rust/core/src/i18n_vi_data.json','w',encoding='utf-8'), ensure_ascii=False, indent=0)"
//! ```

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

const DATA: &str = include_str!("i18n_vi_data.json");

#[derive(Deserialize)]
struct RawData {
    text: HashMap<String, String>,
    patterns: Vec<Vec<String>>,
}

struct Catalog {
    text: HashMap<String, String>,
    patterns: Vec<(Regex, String)>,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    let raw: RawData = serde_json::from_str(DATA).expect("valid i18n_vi_data.json");
    let patterns = raw
        .patterns
        .iter()
        .map(|pair| {
            let re = Regex::new(&pair[0]).expect("valid i18n pattern");
            (re, pair[1].clone())
        })
        .collect();
    Catalog {
        text: raw.text,
        patterns,
    }
});

/// Bảng dịch EN -> VI.
pub fn text() -> &'static HashMap<String, String> {
    &CATALOG.text
}

/// Danh sách (regex trên văn bản tiếng Anh, template tiếng Việt với {0}, {1}…).
pub fn message_patterns() -> &'static [(Regex, String)] {
    &CATALOG.patterns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_loads() {
        assert!(!text().is_empty());
        assert_eq!(text().get("Choose Folder").map(|s| s.as_str()), Some("Chọn thư mục"));
        assert!(!message_patterns().is_empty());
    }

    #[test]
    fn placeholders_match() {
        // Giống test_placeholders_match: tập {placeholder} ở key và value phải bằng nhau.
        let bad: Vec<&String> = text()
            .iter()
            .filter(|(k, v)| crate::i18n::format_fields(k) != crate::i18n::format_fields(v))
            .map(|(k, _)| k)
            .collect();
        assert!(bad.is_empty(), "{bad:?}");
    }

    #[test]
    fn patterns_compile_and_fill() {
        for (re, template) in message_patterns() {
            let n = re.captures_len() - 1; // số nhóm bắt
            let expected: std::collections::HashSet<String> =
                (0..n).map(|i| i.to_string()).collect();
            assert_eq!(crate::i18n::format_fields(template), expected);
        }
    }
}
