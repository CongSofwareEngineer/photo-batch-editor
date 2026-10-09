//! Xuất video thật bằng FFmpeg — phản chiếu phần "Real FFmpeg" của `tests/test_video.py`.
//!
//! Bỏ qua (pass im lặng) khi máy không có FFmpeg; bản build kèm `ffmpeg/` nên thường có.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use pbe_core::photo::{image_io, RgbaImage};
use pbe_core::video::export::{export_video, ExportError, OverlayImage};
use pbe_core::video::ffmpeg::{self, FfmpegError};
use pbe_core::video::project::{delete_clip, move_clip, split_at, Clip, MusicTrack, VideoProject};

/// `(video 4 s 320×240 có tiếng, nhạc wav 10 s)` — dựng một lần cho cả file test.
fn media() -> Option<&'static (PathBuf, PathBuf)> {
    static MEDIA: OnceLock<Option<(PathBuf, PathBuf)>> = OnceLock::new();
    MEDIA
        .get_or_init(|| {
            let exe = ffmpeg::find_ffmpeg(false).ok()?;
            // Thư mục tồn tại suốt tiến trình test (không dùng tempdir vì phải sống qua nhiều test).
            let dir = std::env::temp_dir().join(format!("pbe-video-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).ok()?;
            let video = dir.join("clip thử.mp4");
            let music = dir.join("nhạc.wav");
            let ok = Command::new(&exe)
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc=size=320x240:rate=25:duration=4",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=4",
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                    "-c:a",
                    "aac",
                    "-shortest",
                ])
                .arg(&video)
                .status()
                .ok()?
                .success();
            let ok2 = Command::new(&exe)
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=880:duration=10",
                ])
                .arg(&music)
                .status()
                .ok()?
                .success();
            (ok && ok2).then_some((video, music))
        })
        .as_ref()
}

fn tmp_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.to_string_lossy().contains(".tmp."))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn probe_real_file() {
    // Giống test_probe_real_file.
    let Some((video, _)) = media() else {
        eprintln!("bỏ qua: không có FFmpeg");
        return;
    };
    let i = ffmpeg::probe(video).expect("probe được");
    assert!(i.has_video && i.has_audio);
    assert_eq!((i.width, i.height), (320, 240));
    assert!((i.duration - 4.0).abs() < 0.1, "duration = {}", i.duration);
}

#[test]
fn export_cut_text_music_mov() {
    // Giống test_export_cut_text_music_mov.
    let Some((video, music)) = media() else {
        eprintln!("bỏ qua: không có FFmpeg");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut p = VideoProject::new(video, ffmpeg::probe(video).unwrap());
    split_at(&mut p, 1.0);
    split_at(&mut p, 2.0);
    delete_clip(&mut p, 1); // bỏ 1–2 s → còn 3 s
    move_clip(&mut p, 1, -1);
    let mut m = MusicTrack::new(music.to_string_lossy().to_string(), ffmpeg::probe(music).unwrap().duration);
    m.offset = 1.0;
    m.volume = 0.8;
    p.music = Some(m);

    let png = dir.path().join("t.png");
    image_io::save_image(&png, &RgbaImage::filled(30, 100, [255, 0, 0, 200]), "png", 92).unwrap();
    let overlays = [OverlayImage {
        path: png.to_string_lossy().to_string(),
        x: 10,
        y: 10,
        start: 0.5,
        end: 2.0,
    }];

    let mut progress: Vec<f64> = vec![];
    let out_dir = dir.path().join("editor");
    let out = {
        let mut cb = |v: f64| progress.push(v);
        export_video(
            &p,
            &overlays,
            &out_dir.join("out.mov"),
            "mov",
            "original",
            Some(&mut cb),
            None,
        )
        .expect("xuất được")
    };
    let r = ffmpeg::probe(&out).unwrap();
    assert!((r.duration - 3.0).abs() < 0.15, "duration = {}", r.duration);
    assert!(r.has_audio);
    assert_eq!((r.width, r.height), (320, 240));
    assert_eq!(progress.last(), Some(&1.0));
    assert!(tmp_files(&out_dir).is_empty(), "còn file .tmp");
    assert!(video.exists(), "file nguồn phải còn nguyên");
}

#[test]
fn export_muted_mp4_without_sound() {
    // Giống test_export_muted_mp4_without_sound.
    let Some((video, _)) = media() else {
        eprintln!("bỏ qua: không có FFmpeg");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut p = VideoProject::new(video, ffmpeg::probe(video).unwrap());
    p.mute = true;
    let out = export_video(&p, &[], &dir.path().join("m.mp4"), "mp4", "original", None, None).unwrap();
    assert!(!ffmpeg::probe(&out).unwrap().has_audio);
}

#[test]
fn export_cancel_leaves_no_file() {
    // Giống test_export_cancel_leaves_no_file.
    let Some((video, _)) = media() else {
        eprintln!("bỏ qua: không có FFmpeg");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut p = VideoProject::new(video, ffmpeg::probe(video).unwrap());
    p.clips = vec![Clip::new(0.0, 4.0); 30]; // đủ dài để huỷ kịp
    let flag = ffmpeg::new_cancel_flag();
    ffmpeg::cancel(&flag);
    let out = dir.path().join("c.mp4");
    let err = export_video(&p, &[], &out, "mp4", "original", None, Some(&flag)).unwrap_err();
    assert_eq!(err, ExportError::Ffmpeg(FfmpegError::Cancelled));
    assert!(!out.exists(), "huỷ không được để lại file đầu ra");
    assert!(tmp_files(dir.path()).is_empty(), "huỷ không được để lại file .tmp");
}
