//! Trình chỉnh nhiều ảnh — 3 màn hình: Chuẩn bị → Đang chạy → Kết quả.
//! Port của `ui/prepare_view.py`, `ui/run_view.py`, `ui/results_view.py`, `ui/file_list.py`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use egui::{RichText, Ui};
use pbe_core::batch::{run_batch, BatchOptions, BatchReport, FileResult};
use pbe_core::photo::RgbaImage;
use pbe_core::presets::{self, Preset};
use pbe_core::scanner::{default_output_dir, next_free_output_dir, scan_folder};
use pbe_core::settings::AdjustmentSettings;

use crate::adjust::{self, ImageSizeState};
use crate::fb::Canvas;
use crate::preview::PreviewWorker;
use crate::theme;
use crate::tr;
use crate::tr_args;
use crate::widgets;

#[derive(PartialEq, Clone, Copy)]
pub enum Screen {
    Prepare,
    Run,
    Results,
}

/// Khi thư mục đầu ra đã tồn tại: hỏi ghi đè / tạo thư mục mới / huỷ.
#[derive(PartialEq)]
enum Ask {
    None,
    OutputExists(PathBuf),
}

#[derive(Default)]
struct RunProgress {
    done: usize,
    total: usize,
    current: String,
    results: Vec<FileResult>,
}

struct RunHandle {
    progress: Arc<Mutex<RunProgress>>,
    cancel: Arc<AtomicBool>,
    rx: Receiver<Result<BatchReport, String>>,
    cancelling: bool,
    started: std::time::Instant,
}

pub struct BatchView {
    pub screen: Screen,
    pub folder: Option<PathBuf>,
    pub files: Vec<PathBuf>,
    pub selected: usize,
    pub settings: AdjustmentSettings,
    size_state: ImageSizeState,
    pub error: Option<String>,

    preview: PreviewWorker,
    before: Option<RgbaImage>,
    after: Option<RgbaImage>,
    preview_path: Option<PathBuf>,
    source_size: (usize, usize),
    show_before: bool,
    canvas: Canvas,
    dirty_preview: bool,

    run: Option<RunHandle>,
    pub report: Option<BatchReport>,
    result_filter: String,

    user_preset_dir: PathBuf,
    pub presets: Vec<Preset>,
    pub current_preset: Option<String>,
    ask: Ask,
}

impl BatchView {
    pub fn new(user_preset_dir: PathBuf) -> Self {
        let mut v = BatchView {
            screen: Screen::Prepare,
            folder: None,
            files: vec![],
            selected: 0,
            settings: AdjustmentSettings::default(),
            size_state: ImageSizeState::default(),
            error: None,
            preview: PreviewWorker::default(),
            before: None,
            after: None,
            preview_path: None,
            source_size: (0, 0),
            show_before: false,
            canvas: Canvas::new("batch_preview"),
            dirty_preview: false,
            run: None,
            report: None,
            result_filter: "all".to_string(),
            user_preset_dir,
            presets: vec![],
            current_preset: None,
            ask: Ask::None,
        };
        v.reload_presets();
        v
    }

    pub fn reload_presets(&mut self) {
        self.presets = presets::list_builtin_presets();
        self.presets.extend(presets::list_user_presets(&self.user_preset_dir));
    }

    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    pub fn set_folder(&mut self, folder: Option<PathBuf>) {
        self.folder = folder.clone();
        self.files = folder.as_deref().map(scan_folder).unwrap_or_default();
        self.selected = 0;
        self.before = None;
        self.after = None;
        self.preview_path = None;
        self.error = if self.folder.is_some() && self.files.is_empty() {
            Some(tr("This folder has no supported photos."))
        } else {
            None
        };
        self.dirty_preview = true;
    }

    pub fn current_file(&self) -> Option<&PathBuf> {
        self.files.get(self.selected)
    }

    pub fn set_settings(&mut self, s: AdjustmentSettings) {
        self.size_state.remember(&s.image_size);
        self.settings = s;
        self.dirty_preview = true;
    }

    fn choose_folder(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().set_title(tr("Choose folder")).pick_folder() {
            self.set_folder(Some(dir));
        }
    }

    // --- Vẽ -------------------------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui) {
        self.pump();
        match self.screen {
            Screen::Prepare => self.ui_prepare(ui),
            Screen::Run => self.ui_run(ui),
            Screen::Results => self.ui_results(ui),
        }
        self.ui_ask(ui);
    }

    /// Nhận kết quả xem trước / tiến độ batch; gọi mỗi khung.
    fn pump(&mut self) {
        if let Some(res) = self.preview.poll() {
            match res.error {
                Some(e) => self.error = Some(e),
                None => {
                    self.before = Some(res.before);
                    self.after = Some(res.after);
                    self.preview_path = Some(res.path);
                    self.source_size = res.source_size;
                }
            }
        }
        if self.dirty_preview && !self.preview.busy {
            if let Some(path) = self.current_file().cloned() {
                self.preview.request(&path, &self.settings);
            }
            self.dirty_preview = false;
        }
        let finished = match &self.run {
            Some(run) => run.rx.try_recv().ok(),
            None => None,
        };
        if let Some(result) = finished {
            self.run = None;
            match result {
                Ok(report) => {
                    self.report = Some(report);
                    self.screen = Screen::Results;
                }
                Err(e) => {
                    self.error = Some(tr_args("The batch could not run: {error}", &[("error", &e)]));
                    self.screen = Screen::Prepare;
                }
            }
        }
    }

    fn ui_ask(&mut self, ui: &mut Ui) {
        let Ask::OutputExists(out) = &self.ask else {
            return;
        };
        let out = out.clone();
        let mut decision: Option<Option<PathBuf>> = None;
        egui::Modal::new(egui::Id::new("output_exists")).show(ui.ctx(), |ui| {
            ui.set_width(460.0);
            widgets::heading(ui, &tr("Output folder exists"));
            ui.add_space(6.0);
            ui.label(tr_args(
                "The output folder already exists:\n{path}",
                &[("path", &out.display().to_string())],
            ));
            ui.add_space(4.0);
            widgets::subtitle(
                ui,
                &tr("Overwrite replaces files with the same name and keeps the others."),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if widgets::primary(ui, &tr("Create New Folder"), true).clicked() {
                    let folder = self.folder.clone().unwrap_or_default();
                    decision = Some(next_free_output_dir(&folder).ok());
                }
                if widgets::secondary(ui, &tr("Overwrite"), true).clicked() {
                    decision = Some(Some(out.clone()));
                }
                if widgets::secondary(ui, &tr("Cancel"), true).clicked() {
                    decision = Some(None);
                }
            });
        });
        if let Some(choice) = decision {
            self.ask = Ask::None;
            if let Some(out) = choice {
                self.start(out);
            }
        }
    }

    // --- Màn hình 1: chuẩn bị -------------------------------------------------------------

    fn ui_prepare(&mut self, ui: &mut Ui) {
        egui::Panel::right(egui::Id::new("batch_adjust"))
            .default_size(320.0)
            .min_size(280.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(12)))
            .show(ui, |ui| {
                widgets::caption(ui, &tr("Adjustments"));
                ui.add_space(4.0);
                self.ui_presets(ui);
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut size_state = std::mem::take(&mut self.size_state);
                    if adjust::panel(ui, &mut self.settings, &mut size_state, true) {
                        self.dirty_preview = true;
                        self.current_preset = None;
                    }
                    self.size_state = size_state;
                    ui.add_space(8.0);
                });
            });

        egui::Panel::left(egui::Id::new("batch_files"))
            .default_size(260.0)
            .min_size(200.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(12)))
            .show(ui, |ui| self.ui_file_list(ui));

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::heading(ui, &tr("Batch edit"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = self.folder.is_some() && !self.files.is_empty();
                    if widgets::primary(ui, &tr("Start"), ready).clicked() {
                        self.request_start();
                    }
                    if widgets::secondary(ui, &tr("Choose folder"), true).clicked() {
                        self.choose_folder();
                    }
                });
            });
            widgets::subtitle(ui, &adjust::summary(&self.settings));
            ui.add_space(6.0);
            if let Some(e) = self.error.clone() {
                widgets::error_banner(ui, &e);
                ui.add_space(6.0);
            }
            self.ui_preview(ui);
        });
    }

    fn ui_presets(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let selected = self
                .current_preset
                .clone()
                .unwrap_or_else(|| tr("Custom"));
            egui::ComboBox::from_id_salt("preset_combo")
                .selected_text(selected)
                .width(190.0)
                .show_ui(ui, |ui| {
                    let chosen = self
                        .presets
                        .iter()
                        .find(|p| {
                            ui.selectable_label(
                                self.current_preset.as_deref() == Some(p.name.as_str()),
                                presets::display_name(p),
                            )
                            .clicked()
                        })
                        .cloned();
                    if let Some(p) = chosen {
                        self.current_preset = Some(p.name.clone());
                        self.set_settings(p.settings.clone());
                    }
                });
            if widgets::secondary(ui, &tr("Save"), true).clicked() {
                self.save_preset();
            }
        });
    }

    fn save_preset(&mut self) {
        let base = tr("My setting");
        let existing: Vec<String> = self.presets.iter().map(|p| p.name.clone()).collect();
        let name = presets::unique_name(&base, existing.iter().map(String::as_str));
        match presets::save_user_preset(&self.user_preset_dir, &name, &self.settings, None) {
            Ok(_) => {
                self.reload_presets();
                self.current_preset = Some(name);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn ui_file_list(&mut self, ui: &mut Ui) {
        widgets::caption(ui, &tr("Photos"));
        match &self.folder {
            Some(f) => widgets::subtitle(ui, &f.display().to_string()),
            None => widgets::subtitle(ui, &tr("No folder chosen")),
        }
        ui.label(
            RichText::new(pbe_core::i18n::trn(
                "{n} photo",
                "{n} photos",
                self.files.len() as i64,
            ))
            .size(11.5)
            .color(theme::CYAN),
        );
        ui.separator();
        let mut pick: Option<usize> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, path) in self.files.iter().enumerate() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if widgets::list_row(ui, &name, None, i == self.selected).clicked() {
                    pick = Some(i);
                }
            }
        });
        if let Some(i) = pick {
            self.selected = i;
            self.dirty_preview = true;
        }
    }

    fn ui_preview(&mut self, ui: &mut Ui) {
        let frame = theme::card();
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                let label = if self.show_before {
                    tr("Before")
                } else {
                    tr("After")
                };
                if widgets::secondary(ui, &label, self.after.is_some()).clicked() {
                    self.show_before = !self.show_before;
                }
                if self.preview.busy {
                    ui.spinner();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (w, h) = self.source_size;
                    if w > 0 {
                        let (ow, oh, _) = pbe_core::image_size::predict_output_size(
                            w as i64,
                            h as i64,
                            self.settings.sr_factor(),
                            &self.settings.image_size,
                        );
                        ui.label(
                            RichText::new(format!("{w} × {h} px  →  {ow} × {oh} px"))
                                .size(11.0)
                                .color(theme::TEXT_FAINT),
                        );
                    }
                });
            });
            ui.add_space(6.0);
            let avail = ui.available_size();
            let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
            let ppp = ui.ctx().pixels_per_point();
            let (pw, ph) = (
                (rect.width() * ppp) as usize,
                (rect.height() * ppp) as usize,
            );
            let img = if self.show_before {
                self.before.as_ref()
            } else {
                self.after.as_ref()
            };
            let fb = self.canvas.begin(pw.max(1), ph.max(1));
            fb.checkerboard(12, [0x18, 0x20, 0x36, 255], [0x12, 0x19, 0x2c, 255]);
            if let Some(img) = img {
                if !img.is_empty() {
                    let k = (fb.w as f32 / img.w as f32).min(fb.h as f32 / img.h as f32);
                    let (dw, dh) = (img.w as f32 * k, img.h as f32 * k);
                    let dx = (fb.w as f32 - dw) / 2.0;
                    let dy = (fb.h as f32 - dh) / 2.0;
                    fb.draw_image(img, dx, dy, dw, dh, 1.0);
                }
            }
            self.canvas.paint(ui, rect);
            if img.is_none() {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    tr("Choose a folder to see a preview"),
                    egui::FontId::proportional(13.0),
                    theme::TEXT_FAINT,
                );
            }
        });
    }

    // --- Chạy ------------------------------------------------------------------------------

    fn request_start(&mut self) {
        self.error = None;
        let Some(folder) = self.folder.clone() else { return };
        if self.files.is_empty() {
            self.error = Some(tr("This folder has no supported photos."));
            return;
        }
        match default_output_dir(&folder) {
            Err(e) => self.error = Some(pbe_core::i18n::tr_msg(&e.to_string())),
            Ok(out) if out.exists() => self.ask = Ask::OutputExists(out),
            Ok(out) => self.start(out),
        }
    }

    fn start(&mut self, out: PathBuf) {
        let Some(folder) = self.folder.clone() else { return };
        let files = self.files.clone();
        let settings = self.settings.clone();
        let progress = Arc::new(Mutex::new(RunProgress {
            total: files.len(),
            ..Default::default()
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let shared = Arc::clone(&progress);
        let cancel_thread = Arc::clone(&cancel);
        std::thread::spawn(move || {
            let on_progress = move |done: usize,
                                    total: usize,
                                    current: &str,
                                    res: Option<&FileResult>| {
                let mut g = shared.lock().unwrap();
                g.done = done;
                g.total = total;
                g.current = current.to_string();
                if let Some(r) = res {
                    g.results.push(r.clone());
                }
            };
            let opts = BatchOptions {
                device: Some("cpu".to_string()),
                output_dir: Some(out),
                workers: None,
                cancel: Some(cancel_thread),
                on_progress: Some(&on_progress),
                files: Some(files),
            };
            let _ = tx.send(run_batch(&folder, &settings, opts));
        });
        self.run = Some(RunHandle {
            progress,
            cancel,
            rx,
            cancelling: false,
            started: std::time::Instant::now(),
        });
        self.screen = Screen::Run;
    }

    /// Yêu cầu huỷ (người dùng nhấn Cancel hoặc đóng app).
    pub fn cancel(&mut self) {
        if let Some(run) = &mut self.run {
            run.cancel.store(true, Ordering::SeqCst);
            run.cancelling = true;
        }
    }

    fn ui_run(&mut self, ui: &mut Ui) {
        let Some(run) = &self.run else {
            self.screen = Screen::Prepare;
            return;
        };
        let (done, total, current, results, cancelling, elapsed) = {
            let g = run.progress.lock().unwrap();
            (
                g.done,
                g.total.max(1),
                g.current.clone(),
                g.results.clone(),
                run.cancelling,
                run.started.elapsed().as_secs_f64(),
            )
        };
        let mut cancel_clicked = false;
        egui::CentralPanel::default().show(ui, |ui| {
            widgets::heading(ui, &tr("Editing photos…"));
            widgets::subtitle(
                ui,
                &tr_args(
                    "{done} of {total}",
                    &[("done", &done.to_string()), ("total", &total.to_string())],
                ),
            );
            ui.add_space(10.0);
            let fraction = done as f32 / total as f32;
            widgets::progress_bar(ui, fraction, &format!("{}%", (fraction * 100.0).round()));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if cancelling {
                        tr("Cancelling…")
                    } else {
                        current.clone()
                    })
                    .size(11.5)
                    .color(theme::TEXT_DIM),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::danger(ui, &tr("Cancel"), !cancelling).clicked() {
                        cancel_clicked = true;
                    }
                    ui.label(
                        RichText::new(format!("{elapsed:.0} s"))
                            .size(11.0)
                            .color(theme::TEXT_FAINT),
                    );
                });
            });
            ui.add_space(10.0);
            theme::card().show(ui, |ui| {
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for r in results.iter().rev().take(200) {
                        file_result_row(ui, r);
                    }
                });
            });
        });
        if cancel_clicked {
            self.cancel();
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
    }

    // --- Màn hình 3: kết quả ---------------------------------------------------------------

    fn ui_results(&mut self, ui: &mut Ui) {
        let Some(report) = &self.report else {
            self.screen = Screen::Prepare;
            return;
        };
        let ok = report.count(&["ok"]);
        let warn = report.count(&["warning"]);
        let err = report.count(&["error"]);
        let out_dir = report.output_dir.clone();
        let cancelled = report.cancelled;
        let seconds = report.total_seconds;
        let files: Vec<FileResult> = report
            .files
            .iter()
            .filter(|f| self.result_filter == "all" || f.status == self.result_filter)
            .cloned()
            .collect();

        let mut new_folder = false;
        let mut again = false;
        let mut filter: Option<String> = None;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::heading(
                    ui,
                    &if cancelled { tr("Cancelled") } else { tr("Finished") },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary(ui, &tr("Choose folder"), true).clicked() {
                        new_folder = true;
                    }
                    if widgets::secondary(ui, &tr("Adjust and run again"), true).clicked() {
                        again = true;
                    }
                    if widgets::secondary(ui, &tr("Open output folder"), true).clicked() {
                        open_in_file_manager(&out_dir);
                    }
                });
            });
            widgets::subtitle(
                ui,
                &format!("{}  ·  {:.1} s", out_dir.display(), seconds),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (key, label, count, color) in [
                    ("all", tr("All"), ok + warn + err, theme::TEXT),
                    ("ok", tr("Done"), ok, theme::OK),
                    ("warning", tr("Warnings"), warn, theme::WARNING),
                    ("error", tr("Errors"), err, theme::ERROR),
                ] {
                    let selected = self.result_filter == key;
                    let text = RichText::new(format!("{label}: {count}")).color(color);
                    if ui.selectable_label(selected, text).clicked() {
                        filter = Some(key.to_string());
                    }
                }
            });
            ui.add_space(8.0);
            theme::card().show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for r in &files {
                        file_result_row(ui, r);
                    }
                });
            });
        });
        if let Some(f) = filter {
            self.result_filter = f;
        }
        if again {
            self.screen = Screen::Prepare;
        }
        if new_folder {
            self.screen = Screen::Prepare;
            self.choose_folder();
        }
    }
}

fn file_result_row(ui: &mut Ui, r: &FileResult) {
    ui.horizontal(|ui| {
        let name = r
            .input_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        ui.label(
            RichText::new("●")
                .size(11.0)
                .color(theme::status_color(&r.status)),
        );
        ui.label(RichText::new(name).size(12.0).color(theme::TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let (Some(w), Some(h)) = (r.width, r.height) {
                ui.label(
                    RichText::new(format!("{w}×{h}"))
                        .size(10.5)
                        .color(theme::TEXT_FAINT),
                );
            }
            if !r.message.is_empty() {
                ui.label(
                    RichText::new(pbe_core::i18n::tr_msg(&r.message))
                        .size(10.5)
                        .color(theme::status_color(&r.status)),
                );
            }
        });
    });
}

/// Mở một thư mục trong trình quản lý file của hệ điều hành.
pub fn open_in_file_manager(path: &Path) {
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(path).spawn();
}
