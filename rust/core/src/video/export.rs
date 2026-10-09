//! Dựng và chạy câu lệnh FFmpeg xuất một [`VideoProject`]. Port của `core/video/export.py`.
//!
//! Đồ thị filter (`-filter_complex`):
//!
//! * mỗi đoạn: `trim` / `atrim` + `setpts`, rồi `concat` theo thứ tự đoạn;
//! * `scale` về 1080p / 720p (cạnh ngắn, chỉ thu nhỏ), `setsar=1`;
//! * chữ: một PNG trong suốt cho mỗi dòng chữ (do app vẽ, nên giống hệt bản xem trước và
//!   dấu tiếng Việt đúng), `overlay` kèm `enable='between(t,a,b)'`;
//! * tiếng: tiếng gốc × âm lượng (hoặc không có khi tắt tiếng / không có tiếng) trộn với nhạc
//!   (`atrim` về độ dài video, `volume`, `afade` out nếu bật).
//!
//! Kết quả ghi ra `<tên>.tmp.<ext>` rồi đổi tên khi FFmpeg thành công, nên lần xuất thất bại
//! hoặc bị huỷ không để lại file hỏng; file nguồn không bao giờ bị ghi đè.

use std::path::{Path, PathBuf};

use crate::video::ffmpeg::{self, CancelFlag, FfmpegError};
use crate::video::project::VideoProject;

pub const EDITOR_FOLDER: &str = "editor";
/// Định dạng xuất → phần mở rộng.
pub const FORMATS: [(&str, &str); 2] = [("mp4", ".mp4"), ("mov", ".mov")];
/// Nhãn độ phân giải → cạnh ngắn mục tiêu (`None` = giữ nguyên).
pub const RESOLUTIONS: [(&str, Option<u32>); 3] =
    [("original", None), ("1080p", Some(1080)), ("720p", Some(720))];
pub const SAME_AS_SOURCE: &str = "The output file cannot be the original video";

/// Một PNG trong suốt (đúng độ phân giải đầu ra) chèn lên video trong khoảng thời gian cho trước.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayImage {
    pub path: String,
    pub x: i64, // góc trên trái, px đầu ra
    pub y: i64,
    pub start: f64, // giây timeline
    pub end: f64,
}

/// Lỗi khi xuất video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// Không có gì để xuất, hoặc đầu ra trùng file nguồn.
    Invalid(String),
    Ffmpeg(FfmpegError),
    Io(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Invalid(m) | ExportError::Io(m) => write!(f, "{m}"),
            ExportError::Ffmpeg(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for ExportError {}

impl From<FfmpegError> for ExportError {
    fn from(e: FfmpegError) -> Self {
        ExportError::Ffmpeg(e)
    }
}

fn even(v: f64) -> u32 {
    ((v / 2.0).round_ties_even() as i64 * 2).max(2) as u32
}

/// Cỡ khung đầu ra: cạnh ngắn về 1080 / 720 (không bao giờ phóng to); số chẵn cho H.264.
pub fn output_size(width: u32, height: u32, resolution: &str) -> (u32, u32) {
    let target = RESOLUTIONS
        .iter()
        .find(|(name, _)| *name == resolution)
        .and_then(|(_, t)| *t);
    let short = width.min(height);
    match target {
        Some(t) if short > t => {
            let k = t as f64 / short as f64;
            (even(width as f64 * k), even(height as f64 * k))
        }
        _ => (even(width as f64), even(height as f64)),
    }
}

/// Đường dẫn đầu ra mặc định: thư mục `editor` cạnh video gốc.
pub fn default_output_path(source: &Path, fmt: &str) -> PathBuf {
    let ext = FORMATS
        .iter()
        .find(|(name, _)| *name == fmt)
        .map(|(_, e)| *e)
        .unwrap_or(".mp4");
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "video".to_string());
    source
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default()
        .join(EDITOR_FOLDER)
        .join(format!("{stem}{ext}"))
}

/// Giây cho FFmpeg: 3 chữ số thập phân, không âm.
fn f(v: f64) -> String {
    format!("{:.3}", v.max(0.0))
}

/// Tham số FFmpeg (không gồm file thực thi / tuỳ chọn tiến độ).
pub fn build_args(
    p: &VideoProject,
    overlays: &[OverlayImage],
    out_path: &Path,
    fmt: &str,
    resolution: &str,
) -> Result<Vec<String>, ExportError> {
    if p.clips.is_empty() {
        return Err(ExportError::Invalid(
            "Nothing to export: the timeline is empty".to_string(),
        ));
    }
    let duration = p.duration();
    let use_orig_audio = p.info.has_audio && !p.mute && p.volume > 0.0;
    let mut args: Vec<String> = vec!["-i".into(), p.source.clone()];
    for ov in overlays {
        args.push("-i".into());
        args.push(ov.path.clone());
    }
    let mut music_index: Option<usize> = None;
    if let Some(m) = &p.music {
        if p.music_end() > 0.05 {
            music_index = Some(1 + overlays.len());
            if m.offset > 0.0 {
                args.push("-ss".into());
                args.push(f(m.offset));
            }
            args.push("-i".into());
            args.push(m.path.clone());
        }
    }

    let mut fc: Vec<String> = vec![];
    let n = p.clips.len();
    for (k, c) in p.clips.iter().enumerate() {
        fc.push(format!(
            "[0:v]trim=start={}:end={},setpts=PTS-STARTPTS[v{k}]",
            f(c.start),
            f(c.end)
        ));
        if use_orig_audio {
            fc.push(format!(
                "[0:a]atrim=start={}:end={},asetpts=PTS-STARTPTS[a{k}]",
                f(c.start),
                f(c.end)
            ));
        }
    }
    if n > 1 {
        let ins: String = (0..n)
            .map(|k| {
                if use_orig_audio {
                    format!("[v{k}][a{k}]")
                } else {
                    format!("[v{k}]")
                }
            })
            .collect();
        let a = if use_orig_audio { 1 } else { 0 };
        let tail = if use_orig_audio { "[ac]" } else { "" };
        fc.push(format!("{ins}concat=n={n}:v=1:a={a}[vc]{tail}"));
    } else {
        fc.push("[v0]null[vc]".into());
        if use_orig_audio {
            fc.push("[a0]anull[ac]".into());
        }
    }

    let (w, h) = output_size(p.info.width, p.info.height, resolution);
    if (w, h) != (p.info.width, p.info.height) {
        fc.push(format!("[vc]scale={w}:{h}:flags=lanczos,setsar=1[vs]"));
    } else {
        fc.push("[vc]setsar=1[vs]".into());
    }
    let mut cur = "vs".to_string();
    for (i, ov) in overlays.iter().enumerate() {
        let nxt = format!("vo{i}");
        fc.push(format!(
            "[{cur}][{}:v]overlay=x={}:y={}:enable='between(t,{},{})'[{nxt}]",
            i + 1,
            ov.x,
            ov.y,
            f(ov.start),
            f(ov.end)
        ));
        cur = nxt;
    }
    fc.push(format!("[{cur}]format=yuv420p[vout]"));

    let mut audio_label: Option<String> = None;
    if use_orig_audio {
        fc.push(format!("[ac]volume={:.3}[aorig]", p.volume));
        audio_label = Some("[aorig]".to_string());
    }
    if let (Some(mi), Some(m)) = (music_index, p.music.as_ref()) {
        let end = p.music_end();
        let mut chain = format!(
            "[{mi}:a]atrim=0:{},asetpts=PTS-STARTPTS,volume={:.3}",
            f(end),
            m.volume
        );
        if m.fade_out && m.fade_seconds > 0.0 {
            let fade = m.fade_seconds.min(end);
            chain += &format!(",afade=t=out:st={}:d={}", f(end - fade), f(fade));
        }
        fc.push(chain + "[amus]");
        audio_label = match audio_label {
            Some(label) => {
                fc.push(format!(
                    "{label}[amus]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[aout]"
                ));
                Some("[aout]".to_string())
            }
            None => Some("[amus]".to_string()),
        };
    }

    args.push("-filter_complex".into());
    args.push(fc.join(";"));
    args.push("-map".into());
    args.push("[vout]".into());
    match &audio_label {
        Some(label) => args.extend([
            "-map".into(),
            label.clone(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "192k".into(),
        ]),
        None => args.push("-an".into()),
    }
    args.extend([
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "fast".into(),
        "-crf".into(),
        "20".into(),
        "-t".into(),
        f(duration),
    ]);
    if p.info.fps > 0.0 {
        // tốc độ khung không đổi (concat có thể để lại khe timestamp nhỏ)
        args.extend([
            "-fps_mode".into(),
            "cfr".into(),
            "-r".into(),
            format!("{:.3}", p.info.fps.min(120.0)),
        ]);
    }
    if fmt == "mp4" {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.push(out_path.to_string_lossy().to_string());
    Ok(args)
}

/// `<tên>.tmp.<ext>` — file tạm trong lúc FFmpeg đang ghi.
pub fn tmp_output(out_path: &Path) -> PathBuf {
    let stem = out_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = out_path.extension().map(|s| s.to_string_lossy().to_string());
    out_path.with_file_name(match ext {
        Some(e) => format!("{stem}.tmp.{e}"),
        None => format!("{stem}.tmp"),
    })
}

/// Xuất dự án ra `out_path`. Không bao giờ ghi đè file nguồn.
pub fn export_video(
    p: &VideoProject,
    overlays: &[OverlayImage],
    out_path: &Path,
    fmt: &str,
    resolution: &str,
    on_progress: Option<&mut dyn FnMut(f64)>,
    cancel: Option<&CancelFlag>,
) -> Result<PathBuf, ExportError> {
    let same = match (out_path.canonicalize(), Path::new(&p.source).canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => out_path == Path::new(&p.source),
    };
    if same {
        return Err(ExportError::Invalid(SAME_AS_SOURCE.to_string()));
    }
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ExportError::Io(e.to_string()))?;
    }
    let tmp = tmp_output(out_path);
    let args = build_args(p, overlays, &tmp, fmt, resolution)?;
    let result = ffmpeg::run_ffmpeg(&args, p.duration(), on_progress, cancel)
        .map_err(ExportError::from)
        .and_then(|_| std::fs::rename(&tmp, out_path).map_err(|e| ExportError::Io(e.to_string())));
    if tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|_| out_path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::ffmpeg::MediaInfo;
    use crate::video::project::{delete_clip, split_at, MusicTrack};

    fn info(audio: bool) -> MediaInfo {
        MediaInfo {
            duration: 10.0,
            width: 1920,
            height: 1080,
            fps: 30.0,
            has_video: true,
            has_audio: audio,
            ..Default::default()
        }
    }

    fn project(audio: bool) -> VideoProject {
        VideoProject::new(Path::new("C:/v/in.mp4"), info(audio))
    }

    fn filter_complex(args: &[String]) -> String {
        let i = args.iter().position(|a| a == "-filter_complex").expect("có -filter_complex");
        args[i + 1].clone()
    }

    #[test]
    fn output_size_short_side_and_even() {
        // Giống test_output_size.
        assert_eq!(output_size(1920, 1080, "original"), (1920, 1080));
        assert_eq!(output_size(1920, 1080, "720p"), (1280, 720));
        assert_eq!(output_size(1080, 1920, "720p"), (720, 1280)); // dọc: theo cạnh ngắn
        assert_eq!(output_size(1280, 720, "1080p"), (1280, 720)); // không phóng to
        let (w, h) = output_size(1001, 563, "original");
        assert!(w % 2 == 0 && h % 2 == 0);
        assert!((w as i64 - 1001).abs() <= 1 && (h as i64 - 563).abs() <= 1);
    }

    #[test]
    fn default_output_path_in_editor_folder() {
        // Giống test_default_output_path_in_editor_folder.
        assert_eq!(
            default_output_path(Path::new("/clips/a b.mov"), "mp4"),
            PathBuf::from("/clips/editor/a b.mp4")
        );
    }

    #[test]
    fn build_args_segments_text_music() {
        // Giống test_build_args_segments_text_music.
        let mut p = project(true);
        split_at(&mut p, 4.0);
        delete_clip(&mut p, 0);
        split_at(&mut p, 3.0);
        let mut m = MusicTrack::new("C:/m/song.mp3", 100.0);
        m.offset = 12.0;
        m.volume = 0.5;
        m.fade_seconds = 2.0;
        p.music = Some(m);
        let overlays = [OverlayImage {
            path: "C:/t/0.png".into(),
            x: 10,
            y: 20,
            start: 1.0,
            end: 2.5,
        }];
        let args = build_args(&p, &overlays, Path::new("C:/out/x.mp4"), "mp4", "720p").unwrap();
        let fc = filter_complex(&args);
        assert!(fc.contains("trim=start=4.000:end=7.000"), "{fc}");
        assert!(fc.contains("trim=start=7.000:end=10.000"), "{fc}");
        assert!(fc.contains("concat=n=2:v=1:a=1"), "{fc}");
        assert!(fc.contains("scale=1280:720"), "{fc}");
        assert!(fc.contains("overlay=x=10:y=20:enable='between(t,1.000,2.500)'"), "{fc}");
        assert!(fc.contains("afade=t=out:st=4.000:d=2.000"), "{fc}");
        assert!(fc.contains("amix=inputs=2"), "{fc}");
        let ss = args.iter().position(|a| a == "-ss").unwrap();
        assert_eq!(args[ss + 1], "12.000");
        assert!(args.iter().any(|a| a == "-movflags"));
        assert_eq!(args.last().unwrap(), "C:/out/x.mp4");
    }

    #[test]
    fn build_args_mute_and_no_audio() {
        // Giống test_build_args_mute_and_no_audio.
        let mut p = project(true);
        p.mute = true;
        let args = build_args(&p, &[], Path::new("x.mov"), "mov", "original").unwrap();
        let fc = filter_complex(&args);
        assert!(!fc.contains("[0:a]"), "{fc}");
        assert!(args.iter().any(|a| a == "-an"));
        assert!(!args.iter().any(|a| a == "-movflags"));

        let mut q = project(false);
        q.music = Some(MusicTrack::new("m.wav", 5.0));
        let args = build_args(&q, &[], Path::new("x.mp4"), "mp4", "original").unwrap();
        assert!(args.iter().any(|a| a == "-map"));
        assert!(args.iter().any(|a| a == "[amus]"), "{args:?}");
    }

    #[test]
    fn export_refuses_the_source() {
        // Giống test_export_refuses_the_source.
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("in.mp4");
        std::fs::write(&src, b"").unwrap();
        let p = VideoProject::new(&src, info(true));
        let err = export_video(&p, &[], &src, "mp4", "original", None, None).unwrap_err();
        assert!(matches!(err, ExportError::Invalid(m) if m == SAME_AS_SOURCE));
    }

    #[test]
    fn empty_timeline_is_refused() {
        let mut p = project(true);
        p.clips.clear();
        let err = build_args(&p, &[], Path::new("x.mp4"), "mp4", "original").unwrap_err();
        assert!(matches!(err, ExportError::Invalid(m) if m.contains("empty")));
    }

    #[test]
    fn tmp_output_keeps_the_extension() {
        assert_eq!(tmp_output(Path::new("/a/b.mp4")), PathBuf::from("/a/b.tmp.mp4"));
    }
}
