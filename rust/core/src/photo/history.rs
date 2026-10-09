//! Undo / redo bằng ảnh chụp trạng thái (photo editor và video editor).
//! Port của `core/photo/history.py`.
//!
//! Hai trình sửa giữ toàn bộ trạng thái tài liệu trong các struct nhỏ; trước mỗi thay đổi,
//! trạng thái **hiện tại** được đẩy vào đây. Các thay đổi liên tiếp cùng `key` trong
//! [`MERGE_SECONDS`] (một lần kéo thanh trượt, một lần gõ vào ô chữ) gộp thành một bước.

use std::time::Instant;

pub const DEFAULT_LIMIT: usize = 50;
pub const MERGE_SECONDS: f64 = 1.0;

/// Ngăn xếp undo / redo cho trạng thái kiểu `T`.
#[derive(Debug)]
pub struct History<T> {
    limit: usize,
    undo: Vec<(String, T)>,
    redo: Vec<(String, T)>,
    last_key: Option<String>,
    last_time: f64,
    origin: Instant,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        History::new(DEFAULT_LIMIT)
    }
}

impl<T> History<T> {
    pub fn new(limit: usize) -> Self {
        History {
            limit: limit.max(1),
            undo: vec![],
            redo: vec![],
            last_key: None,
            last_time: 0.0,
            origin: Instant::now(),
        }
    }

    /// Đồng hồ đơn điệu (giây) — tương ứng `time.monotonic()`.
    fn now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64()
    }

    /// Ghi `state` (trạng thái **trước** thay đổi). Trả về `false` khi bị gộp vào bước trước.
    ///
    /// `now` chỉ dùng cho test (bản Python cũng vậy); `None` = đồng hồ thật.
    pub fn push_at(&mut self, label: &str, state: T, key: Option<&str>, now: Option<f64>) -> bool {
        let now = now.unwrap_or_else(|| self.now());
        let merged = match (key, &self.last_key) {
            (Some(k), Some(last)) => {
                k == last.as_str() && now - self.last_time < MERGE_SECONDS && !self.undo.is_empty()
            }
            _ => false,
        };
        self.last_key = key.map(str::to_string);
        self.last_time = now;
        self.redo.clear();
        if merged {
            return false;
        }
        self.undo.push((label.to_string(), state));
        if self.undo.len() > self.limit {
            let excess = self.undo.len() - self.limit;
            self.undo.drain(..excess);
        }
        true
    }

    /// [`push_at`](Self::push_at) với đồng hồ thật.
    pub fn push(&mut self, label: &str, state: T, key: Option<&str>) -> bool {
        self.push_at(label, state, key, None)
    }

    /// Chặn việc gộp với thay đổi kế tiếp (vd. khi nhả chuột khỏi thanh trượt).
    pub fn break_merge(&mut self) {
        self.last_key = None;
    }

    /// Lấy trạng thái trước đó; `current` được đẩy sang ngăn redo.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let (label, state) = self.undo.pop()?;
        self.redo.push((label, current));
        self.last_key = None;
        Some(state)
    }

    pub fn redo(&mut self, current: T) -> Option<T> {
        let (label, state) = self.redo.pop()?;
        self.undo.push((label, current));
        self.last_key = None;
        Some(state)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Nhãn của bước undo kế tiếp (chuỗi rỗng khi không có) — dùng cho menu "Undo <việc gì>".
    pub fn undo_label(&self) -> &str {
        self.undo.last().map(|(l, _)| l.as_str()).unwrap_or("")
    }

    pub fn redo_label(&self) -> &str {
        self.redo.last().map(|(l, _)| l.as_str()).unwrap_or("")
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last_key = None;
    }

    /// Số bước undo đang giữ.
    pub fn len(&self) -> usize {
        self.undo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_and_limit() {
        // Giống test_history_undo_redo_and_limit.
        let mut h: History<i32> = History::new(50);
        let mut state = 0;
        for i in 1..=40 {
            h.push(&format!("step {i}"), state, None);
            state = i;
        }
        assert_eq!(h.len(), 40);
        for expected in (0..40).rev() {
            let undone = h.undo(state).expect("còn bước undo");
            assert_eq!(undone, expected);
            state = expected;
        }
        assert!(h.undo(state).is_none());
        let redone = h.redo(state).expect("còn bước redo");
        assert_eq!(redone, 1);
        assert!(h.can_redo());
        state = redone;
        h.push("new", state, None); // thay đổi mới xoá ngăn redo
        assert!(!h.can_redo());
    }

    #[test]
    fn limit_drops_oldest() {
        // Giống test_history_limit_drops_oldest.
        let mut h: History<i32> = History::new(3);
        for i in 0..5 {
            h.push("s", i, None);
        }
        assert_eq!(h.len(), 3);
        assert_eq!(h.undo(99), Some(4));
    }

    #[test]
    fn merges_same_key() {
        // Giống test_history_merges_same_key.
        let mut h: History<i32> = History::default();
        assert!(h.push_at("slider", 0, Some("a"), Some(10.0)));
        assert!(!h.push_at("slider", 1, Some("a"), Some(10.5))); // cùng lần kéo: gộp
        assert!(h.push_at("slider", 2, Some("a"), Some(12.0))); // nghỉ > 1 s: bước mới
        assert!(h.push_at("other", 3, Some("b"), Some(12.1)));
        assert_eq!(h.len(), 3);
        assert_eq!(h.undo(4), Some(3));
        assert_eq!(h.undo(3), Some(2));
        assert_eq!(h.undo(2), Some(0));
    }

    #[test]
    fn break_merge_starts_a_new_step() {
        let mut h: History<i32> = History::default();
        h.push_at("slider", 0, Some("a"), Some(1.0));
        h.break_merge();
        assert!(h.push_at("slider", 1, Some("a"), Some(1.1)));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn labels_and_clear() {
        let mut h: History<i32> = History::default();
        assert_eq!(h.undo_label(), "");
        h.push("Xoay ảnh", 0, None);
        assert_eq!(h.undo_label(), "Xoay ảnh");
        h.undo(1);
        assert_eq!(h.redo_label(), "Xoay ảnh");
        h.clear();
        assert!(!h.can_undo() && !h.can_redo() && h.is_empty());
    }
}
