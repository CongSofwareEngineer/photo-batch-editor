//! Trình sửa video: mở file, timeline (cắt / xoá / đổi chỗ đoạn), tắt tiếng, nhạc, xuất.
//! Port của `ui/video/video_editor_view.py`, `ui/video/timeline.py`, `ui/video/panels.py`,
//! `ui/video/player.py`.
//!
//! Khung xem trước lấy bằng FFmpeg (`-ss <t> -frames:v 1`) trên luồng nền rồi nạp vào
//! framebuffer, nên không cần một bộ giải mã video riêng trong GUI.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use egui::{Key, RichText, Ui};
use pbe_core::photo::image_io::load_rgba;
use pbe_core::photo::RgbaImage;
use pbe_core::video::export::{
    default_output_path, export_video, ExportError, OverlayImage, FORMATS, RESOLUTIONS,
};
use pbe_core::video::ffmpeg::{self, CancelFlag, FfmpegError, MediaInfo};
use pbe_core::video::project::{
    add_clip, delete_clip, delete_range, fmt_time, keep_range, move_clip, split_at, trim_clip,
    MusicTrack, TextOverlay, VideoProject,
};

use crate::fb::Canvas;
use crate::theme;
use crate::tr;
use crate::tr_args;
use crate::widgets;

/// Phần mở rộng hiện trong hộp thoại Mở video.
pub const VIDEO_EXTS: [&str; 6] = ["mp4", "mov", "m4v", "avi", "mkv", "webm"];
pub const AUDIO_EXTS: [&str; 5] = ["mp3", "wav", "m4a", "aac", "flac"];

struct ExportJob {
    progress: Arc<Mutex<f64>>,
    cancel: CancelFlag,
    rx: Receiver<Result<PathBuf, ExportError>>,
}

#[derive(Default)]
struct FramePeek {
    generation: Arc<AtomicU64>,
    rx: Option<Receiver<(u64, Option<RgbaImage>)>>,
    busy: bool,
    wanted: Option<f64>,
}

pub struct VideoEditorView {
    pub project: Option<VideoProject>,
    pub last_dir: Option<PathBuf>,
    pub playhead: f64,
    history: pbe_core::photo::history::History<VideoProject>,
    selected_clip: usize,
    selected_text: Option<u64>,
    mark_in: Option<f64>,
    mark_out: Option<f64>,
    export_fmt: String,
    export_resolution: String,
    job: Option<ExportJob>,
    peek: FramePeek,
    frame: Option<RgbaImage>,
    canvas: Canvas,
    error: Option<String>,
    info: Option<String>,
    pub modified: bool,
}

impl Default for VideoEditorView {
    fn default() -> Self {
        VideoEditorView {
            project: None,
            last_dir: None,
            playhead: 0.0,
            history: pbe_core::photo::history::History::default(),
            selected_clip: 0,
            selected_text: None,
            mark_in: None,
            mark_out: None,
            export_fmt: "mp4".to_string(),
            export_resolution: "original".to_string(),
            job: None,
            peek: FramePeek::default(),
            frame: None,
            canvas: Canvas::new("video_preview"),
            error: None,
            info: None,
            modified: false,
        }
    }
}

impl VideoEditorView {
    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    fn push_undo(&mut self, label: &str) {
        if let Some(p) = &self.project {
            self.history.push(label, p.clone(), None);
            self.modified = true;
        }
    }

    pub fn open_path(&mut self, path: &Path) {
        match ffmpeg::probe(path) {
            Ok(info) => {
                self.last_dir = path.parent().map(|p| p.to_path_buf());
                self.project = Some(VideoProject::new(path, info));
                self.history.clear();
                self.playhead = 0.0;
                self.selected_clip = 0;
                self.selected_text = None;
                self.mark_in = None;
                self.mark_out = None;
                self.frame = None;
                self.modified = false;
                self.error = None;
                self.info = None;
                self.peek.wanted = Some(0.0);
            }
            Err(e) => self.error = Some(describe(&e)),
        }
    }

    fn choose_and_open(&mut self) {
        let mut d = rfd::FileDialog::new()
            .set_title(tr("Open video"))
            .add_filter(tr("Videos"), &VIDEO_EXTS);
        if let Some(dir) = &self.last_dir {
            d = d.set_directory(dir);
        }
        if let Some(path) = d.pick_file() {
            self.open_path(&path);
        }
    }

    // --- Vẽ -------------------------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui) {
        self.pump(ui.ctx());
        self.ui_toolbar(ui);
        if self.project.is_some() {
            self.ui_right_panel(ui);
            self.ui_timeline(ui);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::WINDOW))
            .show(ui, |ui| {
                if let Some(e) = self.error.clone() {
                    widgets::error_banner(ui, &e);
                }
                if let Some(i) = self.info.clone() {
                    widgets::info_banner(ui, &i, theme::OK);
                }
                if self.project.is_none() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 2.0 - 40.0);
                        widgets::subtitle(ui, &tr("Open a video to start editing"));
                        ui.add_space(8.0);
                        if widgets::primary(ui, &tr("Open video"), true).clicked() {
                            self.choose_and_open();
                        }
                    });
                    return;
                }
                self.ui_preview(ui);
            });
        self.handle_shortcuts(ui);
    }

    /// Nhận khung xem trước và tiến độ xuất; gọi mỗi khung.
    fn pump(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.peek.rx {
            let mut latest = None;
            while let Ok((gen, img)) = rx.try_recv() {
                self.peek.busy = false;
                if gen >= self.peek.generation.load(Ordering::SeqCst) {
                    latest = Some(img);
                }
            }
            if let Some(img) = latest {
                self.frame = img;
            }
        }
        if !self.peek.busy {
            if let (Some(t), Some(p)) = (self.peek.wanted.take(), self.project.as_ref()) {
                let Some((_, src_t)) = p.locate(t) else { return };
                let source = p.source.clone();
                let generation = self.peek.generation.fetch_add(1, Ordering::SeqCst) + 1;
                let (tx, rx) = channel();
                self.peek.rx = Some(rx);
                self.peek.busy = true;
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let img = grab_frame(&source, src_t);
                    let _ = tx.send((generation, img));
                    ctx.request_repaint();
                });
            }
        }
        let done = match &self.job {
            Some(job) => job.rx.try_recv().ok(),
            None => None,
        };
        if let Some(result) = done {
            self.job = None;
            match result {
                Ok(path) => {
                    self.modified = false;
                    self.info = Some(format!("{} → {}", tr("Exported"), path.display()));
                    self.error = None;
                }
                Err(ExportError::Ffmpeg(FfmpegError::Cancelled)) => {
                    self.info = Some(tr("Export cancelled"));
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
    }

    fn ui_toolbar(&mut self, ui: &mut Ui) {
        egui::Panel::top(egui::Id::new("video_toolbar"))
            .frame(theme::bar())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let has = self.project.is_some();
                    let busy = self.is_busy();
                    if widgets::secondary(ui, &tr("Open video"), !busy).clicked() {
                        self.choose_and_open();
                    }
                    if widgets::primary(ui, &tr("Export"), has && !busy).clicked() {
                        self.start_export();
                    }
                    ui.separator();
                    if widgets::secondary(ui, &tr("Split"), has && !busy).clicked() {
                        self.split();
                    }
                    if widgets::secondary(ui, &tr("Delete clip"), has && !busy).clicked() {
                        self.delete_selected_clip();
                    }
                    if widgets::secondary(ui, "◀", has && !busy).clicked() {
                        self.move_selected_clip(-1);
                    }
                    if widgets::secondary(ui, "▶", has && !busy).clicked() {
                        self.move_selected_clip(1);
                    }
                    ui.separator();
                    if widgets::secondary(ui, &tr("Mark In"), has && !busy).clicked() {
                        self.mark_in = Some(self.playhead);
                    }
                    if widgets::secondary(ui, &tr("Mark Out"), has && !busy).clicked() {
                        self.mark_out = Some(self.playhead);
                    }
                    let marked = self.mark_in.is_some() && self.mark_out.is_some();
                    if widgets::secondary(ui, &tr("Keep range"), marked && !busy).clicked() {
                        self.apply_range(true);
                    }
                    if widgets::secondary(ui, &tr("Delete range"), marked && !busy).clicked() {
                        self.apply_range(false);
                    }
                    ui.separator();
                    let (can_undo, can_redo) = (self.history.can_undo(), self.history.can_redo());
                    if widgets::secondary(ui, "↶", can_undo && !busy).clicked() {
                        self.undo();
                    }
                    if widgets::secondary(ui, "↷", can_redo && !busy).clicked() {
                        self.redo();
                    }
                });
            });
    }

    fn ui_right_panel(&mut self, ui: &mut Ui) {
        egui::Panel::right(egui::Id::new("video_panels"))
            .default_size(320.0)
            .min_size(270.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(12)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.ui_clip_panel(ui);
                    ui.add_space(6.0);
                    self.ui_sound_panel(ui);
                    ui.add_space(6.0);
                    self.ui_music_panel(ui);
                    ui.add_space(6.0);
                    self.ui_text_panel(ui);
                    ui.add_space(6.0);
                    self.ui_export_panel(ui);
                });
            });
    }

    /// Đoạn đang chọn: điểm đầu / cuối (giây nguồn) và thêm lại một đoạn đã xoá.
    fn ui_clip_panel(&mut self, ui: &mut Ui) {
        let Some(p) = &self.project else { return };
        let source_duration = p.info.duration;
        let index = self.selected_clip.min(p.clips.len().saturating_sub(1));
        let Some(clip) = p.clips.get(index).copied() else { return };
        let mut trim: Option<(f64, f64)> = None;
        let mut restore = false;
        widgets::section(ui, &tr("Clip"), true, |ui| {
            widgets::subtitle(
                ui,
                &tr_args(
                    "Clip {n} of {total}",
                    &[
                        ("n", &(index + 1).to_string()),
                        ("total", &p.clips.len().to_string()),
                    ],
                ),
            );
            let mut start = clip.start;
            let mut end = clip.end;
            let mut changed = false;
            changed |= ui
                .add(egui::Slider::new(&mut start, 0.0..=source_duration).text(tr("From")))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut end, 0.0..=source_duration).text(tr("To")))
                .changed();
            if changed {
                trim = Some((start, end));
            }
            if widgets::secondary(ui, &tr("Add clip"), true)
                .on_hover_text(tr("Put the whole source video back on the timeline"))
                .clicked()
            {
                restore = true;
            }
        });
        if let Some((start, end)) = trim {
            self.selected_clip = index;
            self.trim_selected(start, end);
        }
        if restore {
            self.restore_clip(0.0, source_duration);
        }
    }

    fn ui_sound_panel(&mut self, ui: &mut Ui) {
        let Some(p) = &mut self.project else { return };
        let has_audio = p.info.has_audio;
        widgets::section(ui, &tr("Sound"), true, |ui| {
            if !has_audio {
                widgets::subtitle(ui, &tr("This video has no sound"));
                return;
            }
            let mut mute = p.mute;
            if ui.checkbox(&mut mute, tr("Mute")).changed() {
                let label = "Mute";
                self.history.push(label, p.clone(), None);
                self.modified = true;
                p.mute = mute;
            }
            let mut volume = p.volume;
            if ui
                .add_enabled(
                    !p.mute,
                    egui::Slider::new(&mut volume, 0.0..=2.0).text(tr("Volume")),
                )
                .changed()
            {
                self.history.push("Volume", p.clone(), Some("volume"));
                self.modified = true;
                p.volume = volume;
            }
        });
    }

    fn ui_music_panel(&mut self, ui: &mut Ui) {
        let mut add = false;
        let mut remove = false;
        {
            let Some(p) = &mut self.project else { return };
            let music = p.music.clone();
            widgets::section(ui, &tr("Music"), true, |ui| match music {
                None => {
                    widgets::subtitle(ui, &tr("No music added"));
                    if widgets::secondary(ui, &tr("Add music"), true).clicked() {
                        add = true;
                    }
                }
                Some(m) => {
                    ui.label(RichText::new(m.name()).size(12.0).color(theme::TEXT));
                    widgets::subtitle(
                        ui,
                        &format!("{} · {}", fmt_time(m.duration, 1), fmt_time(p.music_end(), 1)),
                    );
                    let mut volume = m.volume;
                    let mut offset = m.offset;
                    let mut fade = m.fade_out;
                    let mut fade_seconds = m.fade_seconds;
                    let mut changed = false;
                    changed |= ui
                        .add(egui::Slider::new(&mut volume, 0.0..=2.0).text(tr("Volume")))
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut offset, 0.0..=m.duration.max(0.1))
                                .text(tr("Start from")),
                        )
                        .changed();
                    changed |= ui.checkbox(&mut fade, tr("Fade out")).changed();
                    if fade {
                        changed |= ui
                            .add(egui::Slider::new(&mut fade_seconds, 0.2..=10.0).text(tr("Seconds")))
                            .changed();
                    }
                    if changed {
                        self.history.push("Music", p.clone(), Some("music"));
                        self.modified = true;
                        if let Some(mm) = p.music.as_mut() {
                            mm.volume = volume;
                            mm.offset = offset;
                            mm.fade_out = fade;
                            mm.fade_seconds = fade_seconds;
                        }
                    }
                    if widgets::danger(ui, &tr("Remove music"), true).clicked() {
                        remove = true;
                    }
                }
            });
        }
        if add {
            self.add_music();
        }
        if remove {
            self.push_undo("Music");
            if let Some(p) = &mut self.project {
                p.music = None;
            }
        }
    }

    fn add_music(&mut self) {
        let mut d = rfd::FileDialog::new()
            .set_title(tr("Add music"))
            .add_filter(tr("Audio"), &AUDIO_EXTS);
        if let Some(dir) = &self.last_dir {
            d = d.set_directory(dir);
        }
        let Some(path) = d.pick_file() else { return };
        match ffmpeg::probe(&path) {
            Ok(info) => {
                self.push_undo("Music");
                if let Some(p) = &mut self.project {
                    p.music = Some(MusicTrack::new(path.to_string_lossy().to_string(), info.duration));
                }
            }
            Err(e) => self.error = Some(describe(&e)),
        }
    }

    fn ui_text_panel(&mut self, ui: &mut Ui) {
        let mut add = false;
        let mut delete: Option<u64> = None;
        let mut edit: Option<TextOverlay> = None;
        {
            let Some(p) = &self.project else { return };
            let texts = p.texts.clone();
            let selected = self.selected_text;
            let duration = p.duration();
            widgets::section(ui, &tr("Text"), true, |ui| {
                widgets::info_banner(
                    ui,
                    &tr("Text is kept in the project; drawing it onto the video is not ported yet."),
                    theme::WARNING,
                );
                for t in &texts {
                    let label = if t.text.trim().is_empty() {
                        tr("(empty)")
                    } else {
                        t.text.clone()
                    };
                    let sub = format!("{} → {}", fmt_time(t.start, 1), fmt_time(t.end, 1));
                    if widgets::list_row(ui, &label, Some(&sub), selected == Some(t.id)).clicked() {
                        self.selected_text = Some(t.id);
                    }
                }
                if widgets::secondary(ui, &tr("Add text"), true).clicked() {
                    add = true;
                }
                if let Some(id) = self.selected_text {
                    if let Some(t) = texts.iter().find(|t| t.id == id) {
                        let mut e = t.clone();
                        let mut changed = false;
                        changed |= ui
                            .add(egui::TextEdit::singleline(&mut e.text).hint_text(tr("Text")))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(&mut e.start, 0.0..=duration).text(tr("From")))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(&mut e.end, 0.0..=duration).text(tr("To")))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(&mut e.font_size, 8.0..=256.0).text(tr("Size")))
                            .changed();
                        changed |= ui.add(egui::Slider::new(&mut e.x, 0.0..=1.0).text("X")).changed();
                        changed |= ui.add(egui::Slider::new(&mut e.y, 0.0..=1.0).text("Y")).changed();
                        if changed {
                            edit = Some(e);
                        }
                        if widgets::danger(ui, &tr("Delete text"), true).clicked() {
                            delete = Some(id);
                        }
                    }
                }
            });
        }
        if add {
            self.push_undo("Add text");
            if let Some(p) = &mut self.project {
                let t = TextOverlay {
                    start: self.playhead,
                    end: (self.playhead + 3.0).min(p.duration()),
                    ..Default::default()
                };
                self.selected_text = Some(t.id);
                p.texts.push(t);
            }
        }
        if let Some(e) = edit {
            self.history.break_merge();
            if let Some(p) = &mut self.project {
                self.modified = true;
                if let Some(t) = p.text_mut(e.id) {
                    *t = e;
                }
            }
        }
        if let Some(id) = delete {
            self.push_undo("Delete text");
            if let Some(p) = &mut self.project {
                p.texts.retain(|t| t.id != id);
            }
            self.selected_text = None;
        }
    }

    fn ui_export_panel(&mut self, ui: &mut Ui) {
        let busy = self.is_busy();
        let progress = self
            .job
            .as_ref()
            .map(|j| *j.progress.lock().unwrap())
            .unwrap_or(0.0);
        let mut cancel = false;
        widgets::section(ui, &tr("Export"), true, |ui| {
            ui.horizontal(|ui| {
                for (fmt, _) in FORMATS {
                    if ui
                        .selectable_label(self.export_fmt == fmt, fmt.to_uppercase())
                        .clicked()
                    {
                        self.export_fmt = fmt.to_string();
                    }
                }
            });
            ui.horizontal(|ui| {
                for (res, _) in RESOLUTIONS {
                    if ui
                        .selectable_label(self.export_resolution == res, tr(res))
                        .clicked()
                    {
                        self.export_resolution = res.to_string();
                    }
                }
            });
            if let Some(p) = &self.project {
                widgets::subtitle(ui, &output_size_label(&p.info, &self.export_resolution));
            }
            if busy {
                widgets::progress_bar(
                    ui,
                    progress as f32,
                    &format!("{}%", (progress * 100.0).round()),
                );
                if widgets::danger(ui, &tr("Cancel"), true).clicked() {
                    cancel = true;
                }
            } else if widgets::primary(ui, &tr("Export"), self.project.is_some()).clicked() {
                self.start_export();
            }
        });
        if cancel {
            if let Some(job) = &self.job {
                ffmpeg::cancel(&job.cancel);
            }
        }
    }

    fn ui_preview(&mut self, ui: &mut Ui) {
        let Some(p) = &self.project else { return };
        let duration = p.duration();
        let (w, h) = (p.info.width, p.info.height);
        theme::card().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} / {}",
                        fmt_time(self.playhead, 1),
                        fmt_time(duration, 1)
                    ))
                    .size(12.0)
                    .monospace()
                    .color(theme::CYAN),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{w} × {h}"))
                            .size(11.0)
                            .color(theme::TEXT_FAINT),
                    );
                    if self.peek.busy {
                        ui.spinner();
                    }
                });
            });
            ui.add_space(6.0);
            let avail = ui.available_size();
            let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
            let ppp = ui.ctx().pixels_per_point();
            let fb = self
                .canvas
                .begin(((rect.width() * ppp) as usize).max(1), ((rect.height() * ppp) as usize).max(1));
            fb.clear([0x08, 0x0c, 0x18, 255]);
            if let Some(img) = &self.frame {
                let k = (fb.w as f32 / img.w as f32).min(fb.h as f32 / img.h as f32);
                let (dw, dh) = (img.w as f32 * k, img.h as f32 * k);
                fb.draw_image(
                    img,
                    (fb.w as f32 - dw) / 2.0,
                    (fb.h as f32 - dh) / 2.0,
                    dw,
                    dh,
                    1.0,
                );
            }
            self.canvas.paint(ui, rect);
        });
    }

    fn ui_timeline(&mut self, ui: &mut Ui) {
        let Some(p) = &self.project else { return };
        let duration = p.duration().max(0.001);
        let clips: Vec<(f64, f64)> = {
            let mut acc = 0.0;
            p.clips
                .iter()
                .map(|c| {
                    let start = acc;
                    acc += c.duration();
                    (start, c.duration())
                })
                .collect()
        };
        let playhead = self.playhead;
        let (mark_in, mark_out) = (self.mark_in, self.mark_out);
        let selected = self.selected_clip;
        let mut seek: Option<f64> = None;
        let mut pick: Option<usize> = None;

        egui::Panel::bottom(egui::Id::new("video_timeline"))
            .exact_size(118.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(10)))
            .show(ui, |ui| {
                widgets::caption(ui, &tr("Timeline"));
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 56.0),
                    egui::Sense::click_and_drag(),
                );
                let p = ui.painter();
                p.rect_filled(rect, egui::CornerRadius::same(8), theme::CARD);
                let x_of = |t: f64| rect.min.x + (t / duration) as f32 * rect.width();
                // các đoạn
                for (i, (start, dur)) in clips.iter().enumerate() {
                    let x0 = x_of(*start);
                    let x1 = x_of(start + dur);
                    let r = egui::Rect::from_min_max(
                        egui::pos2(x0 + 1.0, rect.min.y + 6.0),
                        egui::pos2(x1 - 1.0, rect.max.y - 6.0),
                    );
                    let fill = if i == selected {
                        theme::ACCENT.linear_multiply(0.55)
                    } else {
                        theme::HOVER
                    };
                    p.rect_filled(r, egui::CornerRadius::same(6), fill);
                    p.rect_stroke(
                        r,
                        egui::CornerRadius::same(6),
                        egui::Stroke::new(1.0, theme::BORDER_STRONG),
                        egui::StrokeKind::Inside,
                    );
                    if r.width() > 34.0 {
                        p.text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            fmt_time(*dur, 1),
                            egui::FontId::monospace(10.0),
                            theme::TEXT,
                        );
                    }
                }
                // vùng đánh dấu In / Out
                if let (Some(a), Some(b)) = (mark_in, mark_out) {
                    let (lo, hi) = (a.min(b), a.max(b));
                    let r = egui::Rect::from_min_max(
                        egui::pos2(x_of(lo), rect.min.y),
                        egui::pos2(x_of(hi), rect.max.y),
                    );
                    p.rect_filled(r, egui::CornerRadius::ZERO, theme::CYAN.linear_multiply(0.18));
                }
                // đầu đọc
                let px = x_of(playhead);
                p.line_segment(
                    [egui::pos2(px, rect.min.y), egui::pos2(px, rect.max.y)],
                    egui::Stroke::new(2.0, theme::CYAN),
                );

                if let Some(pos) = resp.interact_pointer_pos() {
                    if resp.clicked() || resp.dragged() {
                        let t = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0) as f64
                            * duration;
                        seek = Some(t);
                        pick = clips
                            .iter()
                            .position(|(s, d)| t >= *s && t < s + d)
                            .or(Some(clips.len().saturating_sub(1)));
                    }
                }
                ui.horizontal(|ui| {
                    widgets::subtitle(
                        ui,
                        &tr_args(
                            "{n} clips · {total}",
                            &[
                                ("n", &clips.len().to_string()),
                                ("total", &fmt_time(duration, 1)),
                            ],
                        ),
                    );
                    if let (Some(a), Some(b)) = (mark_in, mark_out) {
                        ui.label(
                            RichText::new(format!(
                                "In {} · Out {}",
                                fmt_time(a.min(b), 1),
                                fmt_time(a.max(b), 1)
                            ))
                            .size(11.0)
                            .color(theme::CYAN),
                        );
                    }
                });
            });
        if let Some(t) = seek {
            self.seek(t);
        }
        if let Some(i) = pick {
            self.selected_clip = i;
        }
    }

    // --- Lệnh -----------------------------------------------------------------------------

    pub fn seek(&mut self, t: f64) {
        let Some(p) = &self.project else { return };
        self.playhead = t.clamp(0.0, p.duration());
        self.peek.wanted = Some(self.playhead);
    }

    pub fn split(&mut self) {
        self.push_undo("Split");
        let t = self.playhead;
        let ok = self.project.as_mut().is_some_and(|p| split_at(p, t));
        if !ok {
            self.history.undo(self.project.clone().unwrap());
            self.error = Some(tr("Too close to the edge of the clip to split."));
        }
    }

    fn delete_selected_clip(&mut self) {
        self.push_undo("Delete clip");
        let i = self.selected_clip;
        let ok = self.project.as_mut().is_some_and(|p| delete_clip(p, i));
        if ok {
            self.selected_clip = self.selected_clip.saturating_sub(1);
            let t = self.playhead;
            self.seek(t);
        } else if let Some(p) = self.project.clone() {
            self.history.undo(p);
            self.error = Some(tr("The last clip cannot be deleted."));
        }
    }

    fn move_selected_clip(&mut self, delta: i64) {
        self.push_undo("Reorder clips");
        let i = self.selected_clip;
        if self.project.as_mut().is_some_and(|p| move_clip(p, i, delta)) {
            self.selected_clip = (i as i64 + delta).max(0) as usize;
        } else if let Some(p) = self.project.clone() {
            self.history.undo(p);
        }
    }

    fn apply_range(&mut self, keep: bool) {
        let (Some(a), Some(b)) = (self.mark_in, self.mark_out) else {
            return;
        };
        self.push_undo(if keep { "Keep range" } else { "Delete range" });
        let ok = self.project.as_mut().is_some_and(|p| {
            if keep {
                keep_range(p, a, b)
            } else {
                delete_range(p, a, b)
            }
        });
        if ok {
            self.mark_in = None;
            self.mark_out = None;
            self.selected_clip = 0;
            self.seek(0.0);
        } else if let Some(p) = self.project.clone() {
            self.history.undo(p);
            self.error = Some(tr("That range cannot be removed."));
        }
    }

    /// Thêm lại một đoạn của file nguồn (dùng sau khi xoá quá tay).
    pub fn restore_clip(&mut self, start: f64, end: f64) {
        self.push_undo("Add clip");
        if let Some(p) = &mut self.project {
            add_clip(p, start, end);
        }
    }

    /// Đặt lại điểm đầu / cuối của đoạn đang chọn.
    pub fn trim_selected(&mut self, start: f64, end: f64) {
        self.push_undo("Trim clip");
        let i = self.selected_clip;
        if let Some(p) = &mut self.project {
            if i < p.clips.len() {
                trim_clip(p, i, start, end);
            }
        }
    }

    pub fn undo(&mut self) {
        if let Some(current) = self.project.clone() {
            if let Some(prev) = self.history.undo(current) {
                self.project = Some(prev);
                self.modified = true;
                let t = self.playhead;
                self.seek(t);
            }
        }
    }

    pub fn redo(&mut self) {
        if let Some(current) = self.project.clone() {
            if let Some(next) = self.history.redo(current) {
                self.project = Some(next);
                self.modified = true;
                let t = self.playhead;
                self.seek(t);
            }
        }
    }

    fn handle_shortcuts(&mut self, ui: &mut Ui) {
        if self.project.is_none() || self.is_busy() {
            return;
        }
        let (undo, redo, split, left, right) = ui.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && !i.modifiers.shift && i.key_pressed(Key::Z),
                cmd && i.modifiers.shift && i.key_pressed(Key::Z),
                i.key_pressed(Key::S) && !cmd,
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
            )
        });
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if split {
            self.split();
        }
        if left {
            self.seek(self.playhead - 0.5);
        }
        if right {
            self.seek(self.playhead + 0.5);
        }
    }

    fn start_export(&mut self) {
        let Some(p) = &self.project else { return };
        if self.is_busy() {
            return;
        }
        let default = default_output_path(Path::new(&p.source), &self.export_fmt);
        let mut dialog = rfd::FileDialog::new().set_title(tr("Export")).set_file_name(
            default
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
        );
        if let Some(dir) = default.parent() {
            dialog = dialog.set_directory(dir);
        }
        let Some(out) = dialog.save_file() else { return };

        let project = p.clone();
        let (fmt, resolution) = (self.export_fmt.clone(), self.export_resolution.clone());
        let progress = Arc::new(Mutex::new(0.0f64));
        let cancel = ffmpeg::new_cancel_flag();
        let (tx, rx) = channel();
        let shared = Arc::clone(&progress);
        let cancel_thread = Arc::clone(&cancel);
        // Chữ chưa được vẽ thành PNG (xem ui_text_panel): xuất không có overlay.
        let overlays: Vec<OverlayImage> = vec![];
        std::thread::spawn(move || {
            let mut cb = |v: f64| {
                *shared.lock().unwrap() = v;
            };
            let res = export_video(
                &project,
                &overlays,
                &out,
                &fmt,
                &resolution,
                Some(&mut cb),
                Some(&cancel_thread),
            );
            let _ = tx.send(res);
        });
        self.error = None;
        self.info = None;
        self.job = Some(ExportJob {
            progress,
            cancel,
            rx,
        });
    }

    /// Huỷ việc đang chạy khi đóng app.
    pub fn shutdown(&mut self) {
        if let Some(job) = &self.job {
            ffmpeg::cancel(&job.cancel);
        }
    }
}

/// Lấy một khung ở giây `t` của file nguồn bằng FFmpeg.
fn grab_frame(source: &str, t: f64) -> Option<RgbaImage> {
    let exe = ffmpeg::find_ffmpeg(false).ok()?;
    let dir = std::env::temp_dir().join(format!("pbe-frame-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let out = dir.join("frame.png");
    let status = std::process::Command::new(exe)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss"])
        .arg(format!("{t:.3}"))
        .args(["-i", source, "-frames:v", "1", "-vf", "scale=min(1280\\,iw):-2"])
        .arg(&out)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    load_rgba(&out).ok()
}

fn describe(e: &FfmpegError) -> String {
    pbe_core::i18n::tr_msg(&e.to_string())
}

/// Cỡ khung đầu ra sẽ dùng khi xuất (hiện trên UI).
pub fn output_size_label(info: &MediaInfo, resolution: &str) -> String {
    let (w, h) = pbe_core::video::export::output_size(info.width, info.height, resolution);
    format!("{w} × {h}")
}
