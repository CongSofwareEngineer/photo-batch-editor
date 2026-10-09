//! Dự án video: các đoạn giữ lại (theo thứ tự), tắt tiếng / âm lượng, chữ, nhạc — và các
//! phép sửa trên timeline. Port của `core/video/project.py`.
//!
//! Thời gian: [`Clip`] dùng giây của **file nguồn**; chữ và đầu đọc dùng giây của
//! **timeline** (các đoạn phát nối tiếp nhau).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::system::DEFAULT_FONT_FAMILY;
use crate::video::ffmpeg::MediaInfo;

/// Đoạn ngắn nhất cho phép (giây).
pub const MIN_CLIP: f64 = 0.1;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Một đoạn giữ lại của file nguồn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clip {
    pub start: f64,
    pub end: f64,
}

impl Clip {
    pub fn new(start: f64, end: f64) -> Self {
        Clip { start, end }
    }

    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
}

/// Một dòng chữ chèn lên video.
#[derive(Debug, Clone, PartialEq)]
pub struct TextOverlay {
    pub id: u64,
    pub text: String,
    pub font_family: String,
    pub font_size: f64, // px ở độ phân giải nguồn
    pub color: String,
    pub bold: bool,
    pub italic: bool,
    pub outline_width: f64,
    pub outline_color: String,
    pub opacity: f64,
    pub x: f64, // tâm, theo phần trăm chiều rộng khung
    pub y: f64, // tâm, theo phần trăm chiều cao khung
    pub start: f64, // giây timeline
    pub end: f64,
}

impl Default for TextOverlay {
    fn default() -> Self {
        TextOverlay {
            id: next_id(),
            text: String::new(),
            font_family: DEFAULT_FONT_FAMILY.to_string(),
            font_size: 64.0,
            color: "#ffffff".to_string(),
            bold: true,
            italic: false,
            outline_width: 3.0,
            outline_color: "#000000".to_string(),
            opacity: 1.0,
            x: 0.5,
            y: 0.85,
            start: 0.0,
            end: 3.0,
        }
    }
}

impl TextOverlay {
    pub fn active_at(&self, t: f64) -> bool {
        self.start <= t && t < self.end
    }
}

/// Bản nhạc nền.
#[derive(Debug, Clone, PartialEq)]
pub struct MusicTrack {
    pub path: String,
    pub duration: f64, // độ dài file nhạc, giây
    pub offset: f64,   // nhạc bắt đầu từ giây này của file
    pub volume: f64,   // 0–2
    pub fade_out: bool,
    pub fade_seconds: f64,
}

impl MusicTrack {
    pub fn new(path: impl Into<String>, duration: f64) -> Self {
        MusicTrack {
            path: path.into(),
            duration,
            offset: 0.0,
            volume: 1.0,
            fade_out: true,
            fade_seconds: 2.0,
        }
    }

    /// Tên file nhạc (hiện trên UI).
    pub fn name(&self) -> String {
        Path::new(&self.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.clone())
    }
}

/// Toàn bộ trạng thái một dự án video (đơn vị undo / redo).
#[derive(Debug, Clone, PartialEq)]
pub struct VideoProject {
    pub source: String,
    pub info: MediaInfo,
    pub clips: Vec<Clip>,
    pub mute: bool,
    pub volume: f64, // tiếng gốc, 0–2
    pub texts: Vec<TextOverlay>,
    pub music: Option<MusicTrack>,
}

impl VideoProject {
    /// Dự án mới: giữ toàn bộ file nguồn làm một đoạn.
    pub fn new(source: &Path, info: MediaInfo) -> Self {
        let duration = info.duration;
        VideoProject {
            source: source.to_string_lossy().to_string(),
            info,
            clips: vec![Clip::new(0.0, duration)],
            mute: false,
            volume: 1.0,
            texts: vec![],
            music: None,
        }
    }

    /// Bản sao độc lập (ảnh chụp cho [`crate::photo::history::History`]).
    pub fn clone_state(&self) -> VideoProject {
        self.clone()
    }

    /// Độ dài timeline (giây).
    pub fn duration(&self) -> f64 {
        self.clips.iter().map(Clip::duration).sum()
    }

    /// Giây timeline nơi đoạn `index` bắt đầu.
    pub fn clip_start(&self, index: usize) -> f64 {
        self.clips[..index.min(self.clips.len())]
            .iter()
            .map(Clip::duration)
            .sum()
    }

    /// `(chỉ số đoạn, giây nguồn)` của giây timeline `t` (kẹp vào trong timeline).
    ///
    /// Trả `None` khi không còn đoạn nào (bản Python trả `(-1, 0.0)`).
    pub fn locate(&self, t: f64) -> Option<(usize, f64)> {
        if self.clips.is_empty() {
            return None;
        }
        let t = t.max(0.0);
        let mut acc = 0.0;
        let last = self.clips.len() - 1;
        for (i, c) in self.clips.iter().enumerate() {
            if t < acc + c.duration() || i == last {
                return Some((i, c.end.min(c.start + (t - acc))));
            }
            acc += c.duration();
        }
        None
    }

    /// Giây timeline của giây nguồn `source_t` trong đoạn `index`.
    pub fn timeline_time(&self, index: usize, source_t: f64) -> f64 {
        let c = self.clips[index];
        self.clip_start(index) + (source_t - c.start).clamp(0.0, c.duration())
    }

    pub fn text(&self, text_id: u64) -> Option<&TextOverlay> {
        self.texts.iter().find(|t| t.id == text_id)
    }

    pub fn text_mut(&mut self, text_id: u64) -> Option<&mut TextOverlay> {
        self.texts.iter_mut().find(|t| t.id == text_id)
    }

    /// Giây timeline nơi nhạc dừng (nhạc ngắn hơn, hoặc hết video).
    pub fn music_end(&self) -> f64 {
        match &self.music {
            None => 0.0,
            Some(m) => self.duration().min(m.duration - m.offset).max(0.0),
        }
    }
}

// --- Sửa timeline (sửa tại chỗ; người gọi ghi bước undo trước) ----------------------------

/// Cắt đoạn đang ở dưới giây timeline `t` thành hai.
pub fn split_at(p: &mut VideoProject, t: f64) -> bool {
    let Some((i, src)) = p.locate(t) else {
        return false;
    };
    let c = p.clips[i];
    if src - c.start < MIN_CLIP || c.end - src < MIN_CLIP {
        return false;
    }
    p.clips.splice(i..i + 1, [Clip::new(c.start, src), Clip::new(src, c.end)]);
    true
}

/// Xoá một đoạn — không bao giờ xoá đoạn cuối cùng còn lại.
pub fn delete_clip(p: &mut VideoProject, index: usize) -> bool {
    if index >= p.clips.len() || p.clips.len() == 1 {
        return false;
    }
    p.clips.remove(index);
    true
}

/// Đổi chỗ đoạn `index` lên / xuống `delta` bước.
pub fn move_clip(p: &mut VideoProject, index: usize, delta: i64) -> bool {
    let n = p.clips.len() as i64;
    let new = index as i64 + delta;
    if index as i64 >= n || new < 0 || new >= n {
        return false;
    }
    let c = p.clips.remove(index);
    p.clips.insert(new as usize, c);
    true
}

/// Đặt lại điểm đầu / cuối (giây nguồn) của một đoạn, kẹp trong file nguồn.
pub fn trim_clip(p: &mut VideoProject, index: usize, start: f64, end: f64) {
    let dur = p.info.duration;
    let start = start.max(0.0).min(dur - MIN_CLIP);
    let end = end.max(start + MIN_CLIP).min(dur);
    p.clips[index] = Clip::new(start, end);
}

/// Thêm lại một đoạn của file nguồn vào cuối timeline.
pub fn add_clip(p: &mut VideoProject, start: f64, end: f64) -> bool {
    let dur = p.info.duration;
    let (lo, hi) = (start.min(end), start.max(end));
    let (start, end) = (lo.max(0.0), hi.min(dur));
    if end - start < MIN_CLIP {
        return false;
    }
    p.clips.push(Clip::new(start, end));
    true
}

/// Các phần của đoạn nằm trong (hoặc ngoài) khoảng timeline `[t0, t1]`.
fn pieces(p: &VideoProject, t0: f64, t1: f64, inside: bool) -> Vec<Clip> {
    let mut out: Vec<Clip> = vec![];
    let mut acc = 0.0;
    for c in &p.clips {
        let (a, b) = (acc, acc + c.duration()); // đoạn này trên timeline
        acc = b;
        if inside {
            let (lo, hi) = (a.max(t0), b.min(t1));
            if hi - lo >= MIN_CLIP / 2.0 {
                out.push(Clip::new(c.start + lo - a, c.start + hi - a));
            }
        } else {
            if t0 - a >= MIN_CLIP / 2.0 {
                out.push(Clip::new(c.start, c.start + b.min(t0) - a));
            }
            if b - t1 >= MIN_CLIP / 2.0 {
                out.push(Clip::new(c.start + a.max(t1) - a, c.end));
            }
        }
    }
    out
}

/// Chỉ giữ khoảng timeline `[t0, t1]` (đánh dấu In / Out).
pub fn keep_range(p: &mut VideoProject, t0: f64, t1: f64) -> bool {
    let (t0, t1) = (t0.min(t1), t0.max(t1));
    let clips = pieces(p, t0, t1, true);
    if clips.is_empty() {
        return false;
    }
    p.clips = clips;
    true
}

/// Bỏ khoảng timeline `[t0, t1]` (vd. một đoạn ở giữa).
pub fn delete_range(p: &mut VideoProject, t0: f64, t1: f64) -> bool {
    let (t0, t1) = (t0.min(t1), t0.max(t1));
    let clips = pieces(p, t0, t1, false);
    if clips.is_empty() || t1 - t0 < MIN_CLIP / 2.0 {
        return false;
    }
    p.clips = clips;
    true
}

/// `m:ss.s` (hoặc `h:mm:ss.s`).
pub fn fmt_time(t: f64, decimals: usize) -> String {
    let t = t.max(0.0);
    let h = (t / 3600.0).floor();
    let rem = t - h * 3600.0;
    let m = (rem / 60.0).floor();
    let s = rem - m * 60.0;
    let width = if decimals > 0 { 3 + decimals } else { 2 };
    let sec = format!("{s:0width$.decimals$}");
    if h > 0.0 {
        format!("{}:{:02}:{sec}", h as i64, m as i64)
    } else {
        format!("{}:{sec}", m as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn info(duration: f64, audio: bool) -> MediaInfo {
        MediaInfo {
            duration,
            width: 1920,
            height: 1080,
            fps: 30.0,
            has_video: true,
            has_audio: audio,
            ..Default::default()
        }
    }

    fn project(duration: f64) -> VideoProject {
        VideoProject::new(&PathBuf::from("C:/v/in.mp4"), info(duration, true))
    }

    fn bounds(p: &VideoProject) -> Vec<(f64, f64)> {
        p.clips.iter().map(|c| (c.start, c.end)).collect()
    }

    #[test]
    fn split_locate_and_delete() {
        // Giống test_split_locate_and_delete.
        let mut p = project(10.0);
        assert!(split_at(&mut p, 4.0) && split_at(&mut p, 7.0));
        assert_eq!(bounds(&p), vec![(0.0, 4.0), (4.0, 7.0), (7.0, 10.0)]);
        assert!(!split_at(&mut p, 4.02)); // quá sát mép
        assert_eq!(p.locate(5.0), Some((1, 5.0)));
        assert!(delete_clip(&mut p, 1)); // mất đoạn giữa
        assert!((p.duration() - 7.0).abs() < 1e-9);
        assert_eq!(p.locate(5.0), Some((1, 8.0)));
        assert!((p.timeline_time(1, 8.0) - 5.0).abs() < 1e-9);
        assert!(!delete_clip(&mut project(10.0), 0)); // không bao giờ xoá đoạn cuối
    }

    #[test]
    fn reorder_trim_add() {
        // Giống test_reorder_trim_add.
        let mut p = project(10.0);
        split_at(&mut p, 3.0);
        assert!(move_clip(&mut p, 1, -1));
        assert_eq!(bounds(&p), vec![(3.0, 10.0), (0.0, 3.0)]);
        trim_clip(&mut p, 0, 4.0, 20.0);
        assert_eq!((p.clips[0].start, p.clips[0].end), (4.0, 10.0));
        trim_clip(&mut p, 0, 9.99, 9.0);
        assert!(p.clips[0].duration() >= MIN_CLIP - 1e-9);
        assert!(add_clip(&mut p, 1.0, 2.5));
        assert_eq!(
            (p.clips[p.clips.len() - 1].start, p.clips[p.clips.len() - 1].end),
            (1.0, 2.5)
        );
    }

    #[test]
    fn keep_and_delete_range_across_clips() {
        // Giống test_keep_and_delete_range_across_clips.
        let mut p = project(10.0);
        split_at(&mut p, 5.0);
        move_clip(&mut p, 1, -1); // timeline: nguồn 5–10, rồi 0–5
        assert!(delete_range(&mut p, 3.0, 7.0)); // timeline 3–7 = nguồn 8–10 + 0–2
        assert_eq!(bounds(&p), vec![(5.0, 8.0), (2.0, 5.0)]);

        let mut q = project(10.0);
        assert!(keep_range(&mut q, 2.0, 6.5));
        assert_eq!(bounds(&q), vec![(2.0, 6.5)]);
        assert!(!delete_range(&mut project(10.0), 0.0, 10.0));
    }

    #[test]
    fn music_end_and_fmt_time() {
        // Giống test_music_end_and_fmt_time.
        let mut p = project(10.0);
        let mut m = MusicTrack::new("m.mp3", 8.0);
        m.offset = 2.0;
        p.music = Some(m);
        assert!((p.music_end() - 6.0).abs() < 1e-9);
        if let Some(m) = p.music.as_mut() {
            m.offset = 0.0;
            m.duration = 60.0;
        }
        assert!((p.music_end() - 10.0).abs() < 1e-9); // cắt ở cuối video
        assert!(matches!(fmt_time(65.25, 1).as_str(), "1:05.2" | "1:05.3"));
        assert_eq!(fmt_time(3725.0, 0), "1:02:05");
        assert_eq!(fmt_time(-5.0, 1), "0:00.0");
    }

    #[test]
    fn text_ids_are_unique_and_active_at() {
        let a = TextOverlay::default();
        let b = TextOverlay::default();
        assert_ne!(a.id, b.id);
        let t = TextOverlay { start: 1.0, end: 2.0, ..Default::default() };
        assert!(!t.active_at(0.99) && t.active_at(1.0) && !t.active_at(2.0));
    }

    #[test]
    fn music_name_is_the_file_name() {
        assert_eq!(MusicTrack::new("/nhạc/bài hát.mp3", 1.0).name(), "bài hát.mp3");
    }
}
