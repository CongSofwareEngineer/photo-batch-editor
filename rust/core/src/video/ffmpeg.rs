//! FFmpeg: tìm file thực thi, đọc thông tin media, chạy kèm tiến độ / huỷ.
//! Port của `core/video/ffmpeg.py`.
//!
//! Thứ tự tìm `ffmpeg`:
//!
//! 1. biến môi trường `PBE_FFMPEG` (đường dẫn đầy đủ),
//! 2. `ffmpeg/ffmpeg(.exe)` đi kèm app (bản build copy vào đó),
//! 3. `ffmpeg` trên `PATH`.
//!
//! (Bản Python còn thử binary của gói pip `imageio-ffmpeg`; bản Rust không có gói đó nên
//! bỏ bước này — bản build vẫn kèm `ffmpeg/` như cũ.)

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use regex::Regex;

pub const FFMPEG_NOT_FOUND: &str = "FFmpeg was not found. It should be in the \"ffmpeg\" folder next to the app; reinstall the app or put ffmpeg.exe on the PATH.";

/// Tên file thực thi theo hệ điều hành.
pub const EXE: &str = if cfg!(target_os = "windows") {
    "ffmpeg.exe"
} else {
    "ffmpeg"
};

/// Lỗi khi dùng FFmpeg.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfmpegError {
    /// Không tìm thấy file thực thi (tương ứng `FFmpegNotFound` của Python).
    NotFound(String),
    /// FFmpeg chạy nhưng thất bại; thông điệp là phần cuối log của nó.
    Failed(String),
    /// Người dùng huỷ (tương ứng `Cancelled` của Python).
    Cancelled,
}

impl std::fmt::Display for FfmpegError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FfmpegError::NotFound(m) | FfmpegError::Failed(m) => write!(f, "{m}"),
            FfmpegError::Cancelled => write!(f, "Cancelled"),
        }
    }
}
impl std::error::Error for FfmpegError {}

/// Cờ huỷ dùng chung giữa UI và luồng chạy FFmpeg (tương ứng `threading.Event`).
pub type CancelFlag = Arc<AtomicBool>;

pub fn new_cancel_flag() -> CancelFlag {
    Arc::new(AtomicBool::new(false))
}

static CACHED: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<PathBuf>> {
    CACHED.get_or_init(|| Mutex::new(None))
}

/// Các vị trí sẽ thử, theo thứ tự ưu tiên.
pub fn candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    if let Ok(env) = std::env::var("PBE_FFMPEG") {
        if !env.is_empty() {
            out.push(PathBuf::from(env));
        }
    }
    out.push(crate::paths::app_root().join("ffmpeg").join(EXE));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("ffmpeg").join(EXE));
        }
    }
    out.extend(on_path(EXE));
    out
}

fn on_path(name: &str) -> Vec<PathBuf> {
    let Some(paths) = std::env::var_os("PATH") else {
        return vec![];
    };
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .filter(|p| p.is_file())
        .collect()
}

/// Đường dẫn FFmpeg đang dùng (lưu lại sau lần tìm đầu). `refresh` buộc tìm lại.
pub fn find_ffmpeg(refresh: bool) -> Result<PathBuf, FfmpegError> {
    let mut cached = cache().lock().unwrap();
    if let Some(p) = cached.as_ref() {
        if !refresh && p.is_file() {
            return Ok(p.clone());
        }
    }
    for c in candidates() {
        if c.is_file() {
            *cached = Some(c.clone());
            return Ok(c);
        }
    }
    *cached = None;
    Err(FfmpegError::NotFound(FFMPEG_NOT_FOUND.to_string()))
}

/// Xoá đường dẫn đã lưu (dùng trong test).
pub fn reset_cache() {
    *cache().lock().unwrap() = None;
}

/// Không nhá cửa sổ console trên Windows (app GUI).
fn hide_console(cmd: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

// --- Thông tin media ----------------------------------------------------------------------

/// Thông tin một file video / audio, kích thước là cỡ **hiển thị** (đã tính xoay).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaInfo {
    pub duration: f64, // giây
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_video: bool,
    pub has_audio: bool,
    pub rotation: i32,
    pub video_codec: String,
    pub audio_codec: String,
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("regex trong ffmpeg.rs phải hợp lệ")
}

/// Thông tin media đọc từ log của `ffmpeg -i <file>`.
pub fn parse_probe(text: &str) -> Result<MediaInfo, FfmpegError> {
    let r_duration = re(r"Duration:\s*(\d+):(\d+):(\d+(?:\.\d+)?)");
    let r_video = re(r"Stream #\d+:\d+.*?: Video: (\w+).*?[, ](\d{2,5})x(\d{2,5})");
    let r_fps = re(r"(\d+(?:\.\d+)?) fps");
    let r_tbr = re(r"(\d+(?:\.\d+)?) tbr");
    let r_audio = re(r"Stream #\d+:\d+.*?: Audio: (\w+)");
    let r_rotation = re(r"rotation of (-?\d+(?:\.\d+)?) degrees|rotate\s*:\s*(-?\d+)");

    let Some(m) = r_duration.captures(text) else {
        return Err(FfmpegError::Failed(
            "Cannot read the duration of this file (is it a video or audio file?)".to_string(),
        ));
    };
    let duration = m[1].parse::<f64>().unwrap_or(0.0) * 3600.0
        + m[2].parse::<f64>().unwrap_or(0.0) * 60.0
        + m[3].parse::<f64>().unwrap_or(0.0);
    let mut info = MediaInfo {
        duration,
        ..Default::default()
    };
    for line in text.lines() {
        if line.contains("Video:") && !info.has_video && !line.contains("attached pic") {
            if let Some(v) = r_video.captures(line) {
                info.has_video = true;
                info.video_codec = v[1].to_string();
                info.width = v[2].parse().unwrap_or(0);
                info.height = v[3].parse().unwrap_or(0);
                let f = r_fps.captures(line).or_else(|| r_tbr.captures(line));
                info.fps = f.and_then(|c| c[1].parse().ok()).unwrap_or(0.0);
            }
        } else if line.contains("Audio:") && !info.has_audio {
            if let Some(a) = r_audio.captures(line) {
                info.has_audio = true;
                info.audio_codec = a[1].to_string();
            }
        }
    }
    if let Some(r) = r_rotation.captures(text) {
        let raw = r
            .get(1)
            .or_else(|| r.get(2))
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        info.rotation = (raw.round_ties_even() as i32).rem_euclid(360);
        if info.rotation == 90 || info.rotation == 270 {
            // FFmpeg tự xoay: khung ra đã quay
            std::mem::swap(&mut info.width, &mut info.height);
        }
    }
    Ok(info)
}

/// Đọc thông tin của một file media.
pub fn probe(path: &Path) -> Result<MediaInfo, FfmpegError> {
    let exe = find_ffmpeg(false)?;
    let mut cmd = Command::new(&exe);
    cmd.args(["-hide_banner", "-i"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| FfmpegError::Failed(format!("Cannot run FFmpeg: {e}")))?;
    parse_probe(&String::from_utf8_lossy(&out.stderr))
}

// --- Chạy ---------------------------------------------------------------------------------

/// Chạy `ffmpeg <args>`; `on_progress(fraction)` theo đầu ra `-progress` của FFmpeg.
///
/// Trả [`FfmpegError::Cancelled`] khi `cancel` được bật (tiến trình bị dừng) và
/// [`FfmpegError::Failed`] kèm phần cuối log khi FFmpeg thất bại.
pub fn run_ffmpeg(
    args: &[String],
    duration: f64,
    mut on_progress: Option<&mut dyn FnMut(f64)>,
    cancel: Option<&CancelFlag>,
) -> Result<(), FfmpegError> {
    let exe = find_ffmpeg(false)?;
    let mut cmd = Command::new(&exe);
    cmd.args(["-hide_banner", "-nostdin", "-y"])
        .args(args)
        .args(["-progress", "pipe:1", "-nostats"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| FfmpegError::Failed(format!("Cannot run FFmpeg: {e}")))?;

    let stdout = child.stdout.take().expect("stdout đã piped");
    let stderr = child.stderr.take().expect("stderr đã piped");

    // 25 dòng log cuối, để báo lỗi cho người dùng.
    let tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let tail_writer = Arc::clone(&tail);
    let err_thread = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let mut t = tail_writer.lock().unwrap();
            t.push(line.trim_end().to_string());
            if t.len() > 25 {
                t.remove(0);
            }
        }
    });

    let child = Arc::new(Mutex::new(child));
    let cancelled = Arc::new(AtomicBool::new(false));
    let watcher = cancel.map(|flag| {
        let (flag, child, cancelled) = (Arc::clone(flag), Arc::clone(&child), Arc::clone(&cancelled));
        std::thread::spawn(move || {
            loop {
                if child.lock().unwrap().try_wait().ok().flatten().is_some() {
                    return;
                }
                if flag.load(Ordering::SeqCst) {
                    cancelled.store(true, Ordering::SeqCst);
                    let _ = child.lock().unwrap().kill();
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        })
    });

    let r_out_time = re(r"^out_time_(?:us|ms)=(\d+)");
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let line = line.trim();
        if let (Some(c), Some(cb)) = (r_out_time.captures(line), on_progress.as_mut()) {
            if duration > 0.0 {
                let us: f64 = c[1].parse().unwrap_or(0.0);
                cb((us / 1e6 / duration).min(1.0));
            }
        }
    }
    let status = child
        .lock()
        .unwrap()
        .wait()
        .map_err(|e| FfmpegError::Failed(format!("Cannot wait for FFmpeg: {e}")))?;
    let _ = err_thread.join();
    if let Some(w) = watcher {
        let _ = w.join();
    }

    let was_cancelled = cancelled.load(Ordering::SeqCst)
        || cancel.is_some_and(|f| f.load(Ordering::SeqCst));
    if was_cancelled {
        return Err(FfmpegError::Cancelled);
    }
    if !status.success() {
        let t = tail.lock().unwrap();
        let lines: Vec<&str> = t.iter().map(String::as_str).filter(|l| !l.trim().is_empty()).collect();
        let msg = lines[lines.len().saturating_sub(8)..].join("\n");
        return Err(FfmpegError::Failed(if msg.is_empty() {
            format!("FFmpeg exited with code {}", status.code().unwrap_or(-1))
        } else {
            msg
        }));
    }
    if let Some(cb) = on_progress.as_mut() {
        cb(1.0);
    }
    Ok(())
}

/// Tiện ích: huỷ một lần chạy đang diễn ra.
pub fn cancel(flag: &CancelFlag) {
    flag.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE_TEXT: &str = r#"
Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'C:\v\phone.mp4':
  Duration: 00:01:02.50, start: 0.000000, bitrate: 17000 kb/s
  Stream #0:0[0x1](eng): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709), 1920x1080, 16000 kb/s, 29.97 fps, 29.97 tbr, 90k tbn (default)
      Side data:
        displaymatrix: rotation of -90.00 degrees
  Stream #0:1[0x2](eng): Audio: aac (LC) (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 256 kb/s (default)
"#;

    #[test]
    fn parse_probe_rotated_phone_video() {
        // Giống test_parse_probe_rotated_phone_video.
        let i = parse_probe(PROBE_TEXT).unwrap();
        assert!((i.duration - 62.5).abs() < 1e-9);
        assert_eq!((i.width, i.height), (1080, 1920)); // đã tính xoay
        assert!((i.fps - 29.97).abs() < 1e-9);
        assert!(i.has_audio);
        assert_eq!(i.video_codec, "h264");
        assert_eq!(i.audio_codec, "aac");
        assert_eq!(i.rotation, 270);
    }

    #[test]
    fn parse_probe_audio_only_and_errors() {
        // Giống test_parse_probe_audio_only_and_errors.
        let i = parse_probe(
            "  Duration: 00:00:20.04, start: 0.025057, bitrate: 128 kb/s\n\
               Stream #0:0: Audio: mp3 (mp3float), 44100 Hz, mono, fltp, 128 kb/s\n",
        )
        .unwrap();
        assert!(i.has_audio && !i.has_video);
        assert!((i.duration - 20.04).abs() < 1e-9);
        match parse_probe("garbage") {
            Err(FfmpegError::Failed(m)) => assert!(m.contains("duration"), "{m}"),
            other => panic!("phải là lỗi duration, nhận {other:?}"),
        }
    }

    #[test]
    fn not_found_message_is_clear() {
        // Giống test_find_ffmpeg_reports_clearly.
        assert!(FFMPEG_NOT_FOUND.contains("ffmpeg"));
        let err = FfmpegError::NotFound(FFMPEG_NOT_FOUND.to_string());
        assert!(err.to_string().starts_with("FFmpeg was not found"));
    }

    #[test]
    fn env_override_comes_first() {
        // Giống test_find_ffmpeg_env_override: PBE_FFMPEG là ứng viên đầu tiên.
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join(EXE);
        std::fs::write(&fake, b"").unwrap();
        std::env::set_var("PBE_FFMPEG", &fake);
        let first = candidates().into_iter().next().unwrap();
        std::env::remove_var("PBE_FFMPEG");
        assert_eq!(first, fake);
    }
}
