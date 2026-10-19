//! Trình sửa một ảnh — bố cục kiểu Photoshop: thanh menu + thanh tuỳ chọn công cụ ở trên,
//! thanh công cụ dọc bên trái, canvas ở giữa, panel (Properties / Adjust / Skin / Sharpen /
//! Layers) bên phải, thanh trạng thái ở dưới. Port của `ui/photo/photo_editor_view.py`,
//! `ui/photo/canvas.py`, `ui/photo/tools.py`, `ui/photo/panels.py`.

pub mod collage;
pub mod doc;
pub mod mask_edit;
pub mod render;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use egui::{Color32, Key, Pos2, RichText, Stroke, Ui};
use pbe_core::bg_remove::BackgroundRemover;
use pbe_core::enhance::SuperResolver;
use pbe_core::photo::effects::{
    ai_sharpen, PhotoAdjust, SharpenSettings, ADJUST_SLIDERS, BLUR_MODES, SHARPEN_SLIDERS,
};
use pbe_core::photo::geometry::{
    clamp_zoom, crop_rect, crop_ratio, fit_scale, next_zoom, round_rect, zoom_at, CROP_RATIOS,
    FIT_MARGIN,
};
use pbe_core::photo::image_io::{default_export_path, load_rgba, save_image, OPEN_FILTER_EXTS};
use pbe_core::photo::matting::{Backdrop, Cutout, Matte, BACKDROP_COLORS, CUTOUT_SLIDERS};
use pbe_core::photo::skin::{SkinSmoothSettings, SKIN_SLIDERS};
use pbe_core::photo::RgbaImage;
use pbe_core::settings::SliderSpec;

use crate::fb::Canvas;
use crate::preview::PREVIEW_LONG_SIDE;
use crate::theme;
use crate::tr;
use crate::widgets;
use doc::{CloneStroke, Document, Layer, BACKGROUND_ID};
use mask_edit::{MaskBrush, MaskEdit, MaskView};
use render::BaseCache;

#[derive(PartialEq, Clone, Copy, Debug)]
enum Tool {
    Move,
    Crop,
    Blur,
    Clone,
    Hand,
    Zoom,
}

/// Một nút của thanh công cụ dọc.
#[derive(PartialEq, Clone, Copy)]
enum ToolButton {
    Tool(Tool),
    InsertImage,
    Collage,
}

/// Thanh công cụ dọc: (nút, nhãn, phím tắt). `None` = vạch ngăn nhóm.
const TOOLBAR: [Option<(ToolButton, &str, Key)>; 10] = [
    Some((ToolButton::Tool(Tool::Move), "Move tool", Key::V)),
    Some((ToolButton::Tool(Tool::Crop), "Crop tool", Key::C)),
    None,
    Some((ToolButton::Tool(Tool::Blur), "Blur tool", Key::B)),
    Some((ToolButton::Tool(Tool::Clone), "Clone stamp tool", Key::S)),
    Some((ToolButton::InsertImage, "Insert image", Key::I)),
    Some((ToolButton::Collage, "Collage", Key::L)),
    None,
    Some((ToolButton::Tool(Tool::Hand), "Hand tool", Key::H)),
    Some((ToolButton::Tool(Tool::Zoom), "Zoom tool", Key::Z)),
];

/// Hình chữ nhật `(x, y, w, h)` px ảnh.
type RectF = (f64, f64, f64, f64);

/// Bán kính bắt nút góc (px màn hình).
const HANDLE_HIT: f32 = 8.0;

enum Drag {
    Pan { last: Pos2 },
    MoveLayer { id: u64, last: (f64, f64) },
    Resize { id: u64, corner: usize, start: RectF },
    Rect { anchor: (f64, f64) },
}

const STRENGTH_SPEC: SliderSpec = SliderSpec {
    key: "strength",
    label: "Strength",
    panel: "Blur",
    group: "Blur",
    minimum: 1.0,
    maximum: 100.0,
    step: 1.0,
    default: 40.0,
    decimals: 0,
};

const STRAIGHTEN_SPEC: SliderSpec = SliderSpec {
    key: "angle",
    label: "Straighten",
    panel: "Adjust",
    group: "Adjust",
    minimum: -45.0,
    maximum: 45.0,
    step: 0.1,
    default: 0.0,
    decimals: 1,
};

/// Tách nền đang chạy ở thread nền (model mất ~0.2–1 s, lần đầu thêm thời gian nạp model).
struct BgJob {
    rx: mpsc::Receiver<Result<Matte, String>>,
    /// Tài liệu + cỡ ảnh lúc bắt đầu: đổi ảnh / crop / xoay trong lúc chạy thì bỏ kết quả.
    doc_serial: u64,
    size: (usize, usize),
}

/// Làm nét bằng AI đang chạy ở thread nền (Real-ESRGAN, vài giây tới vài chục giây).
struct SrJob {
    rx: mpsc::Receiver<SrMsg>,
    progress: f32,
    cancel: Arc<AtomicBool>,
    doc_serial: u64,
    size: (usize, usize),
}

enum SrMsg {
    Progress(f32),
    Done(Result<RgbaImage, String>),
}

/// Model Super Resolution nạp một lần (dùng cho "Làm nét bằng AI").
static RESOLVER: Mutex<Option<Arc<SuperResolver>>> = Mutex::new(None);

fn super_resolver() -> Result<Arc<SuperResolver>, String> {
    let mut slot = RESOLVER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = slot.as_ref() {
        return Ok(r.clone());
    }
    // tile 512: mỗi ô của ai_sharpen (480 + lề) chạy model một lần
    let r = Arc::new(SuperResolver::new(None, Some(512))?);
    *slot = Some(r.clone());
    Ok(r)
}

/// Model tách nền nạp một lần, dùng chung cho mọi lần chạy.
static REMOVER: Mutex<Option<Arc<BackgroundRemover>>> = Mutex::new(None);

fn background_remover() -> Result<Arc<BackgroundRemover>, String> {
    let mut slot = REMOVER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = slot.as_ref() {
        return Ok(r.clone());
    }
    let r = Arc::new(BackgroundRemover::new(None)?);
    *slot = Some(r.clone());
    Ok(r)
}

pub struct PhotoEditorView {
    pub doc: Option<Document>,
    scale: f64,
    offset: (f64, f64),
    tool: Tool,
    crop_ratio: String,
    crop_pending: Option<(f64, f64, f64, f64)>,
    drag: Option<Drag>,
    canvas: Canvas,
    composite: Option<RgbaImage>,
    composite_rev: u64,
    preview_scale: f64,
    blur_mode: String,
    blur_strength: f64,
    blur_shape: String,
    pub last_dir: Option<PathBuf>,
    export_fmt: String,
    export_quality: u8,
    error: Option<String>,
    info: Option<String>,
    fit_requested: bool,
    /// Toạ độ ảnh dưới con trỏ (thanh trạng thái).
    cursor: Option<(f64, f64)>,
    collage: collage::CollageView,
    bg_job: Option<BgJob>,
    /// Đang chỉnh vùng chọn tách nền (sau khi AI chạy xong, trước khi áp dụng).
    mask_edit: Option<MaskEdit>,
    /// Ảnh nền (chưa tách, chưa xoay, không layer) cỡ xem trước cho chế độ chỉnh vùng + revision.
    mask_base: Option<RgbaImage>,
    mask_base_rev: u64,
    /// Tăng mỗi lần mở tài liệu khác (để nhận ra kết quả tách nền của ảnh cũ).
    doc_serial: u64,
    /// Ảnh nền đã chỉnh (Adjust / mịn da / Unsharp) cỡ xem trước — không tính lại khi chỉ đổi layer.
    base_cache: BaseCache,
    /// Tem nhân bản: điểm mẫu (Alt + bấm), độ lệch nguồn − đích, layer đang tô, tuỳ chọn cọ.
    clone_source: Option<(f64, f64)>,
    clone_offset: Option<(f64, f64)>,
    clone_painting: Option<u64>,
    clone_size: f64,
    clone_hardness: f64,
    clone_opacity: f32,
    clone_aligned: bool,
    sr_job: Option<SrJob>,
    /// Độ mạnh "Làm nét bằng AI" 0…100 %.
    ai_strength: f64,
    /// Ảnh thu nhỏ trong bảng Layer: id → (con trỏ `Arc` ảnh nguồn, texture).
    thumbs: HashMap<u64, (usize, egui::TextureHandle)>,
    /// Đang đổi tên layer: (id, tên đang gõ).
    renaming: Option<(u64, String)>,
}

impl Default for PhotoEditorView {
    fn default() -> Self {
        PhotoEditorView {
            doc: None,
            scale: 1.0,
            offset: (0.0, 0.0),
            tool: Tool::Move,
            crop_ratio: "free".to_string(),
            crop_pending: None,
            drag: None,
            canvas: Canvas::new("photo_canvas"),
            composite: None,
            composite_rev: 0,
            preview_scale: 1.0,
            blur_mode: "blur".to_string(),
            blur_strength: 40.0,
            blur_shape: "rect".to_string(),
            last_dir: None,
            export_fmt: "jpeg".to_string(),
            export_quality: 92,
            error: None,
            info: None,
            fit_requested: false,
            cursor: None,
            collage: collage::CollageView::default(),
            bg_job: None,
            mask_edit: None,
            mask_base: None,
            mask_base_rev: 0,
            doc_serial: 0,
            base_cache: BaseCache::default(),
            clone_source: None,
            clone_offset: None,
            clone_painting: None,
            clone_size: 60.0,
            clone_hardness: 0.5,
            clone_opacity: 1.0,
            clone_aligned: true,
            sr_job: None,
            ai_strength: 100.0,
            thumbs: HashMap::new(),
            renaming: None,
        }
    }
}

impl PhotoEditorView {
    pub fn modified(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.modified)
    }

    pub fn open_path(&mut self, path: &Path) {
        match load_rgba(path) {
            Ok(img) => {
                self.last_dir = path.parent().map(|p| p.to_path_buf());
                self.set_document(Document::new(img, Some(path.to_path_buf())));
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn set_document(&mut self, doc: Document) {
        let long = doc.state.background.img.long_side().max(1);
        self.preview_scale = (PREVIEW_LONG_SIDE as f64 / long as f64).min(1.0);
        self.doc = Some(doc);
        self.doc_serial += 1;
        self.mask_edit = None;
        self.mask_base = None;
        self.composite = None;
        self.composite_rev = 0;
        self.fit_requested = true;
        self.crop_pending = None;
        self.drag = None;
        self.tool = Tool::Move;
        self.error = None;
        self.info = None;
        self.base_cache = BaseCache::default();
        self.clone_source = None;
        self.clone_offset = None;
        self.clone_painting = None;
        self.thumbs.clear();
        self.renaming = None;
    }

    fn choose_and_open(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title(tr("Open photo")).add_filter(
            tr("Photos"),
            &OPEN_FILTER_EXTS.map(|e| e.trim_start_matches('.')),
        );
        if let Some(dir) = &self.last_dir {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_path(&path);
        }
    }

    fn select_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.crop_pending = None;
        self.drag = None;
        self.clone_painting = None;
        if tool == Tool::Clone {
            if let Some(d) = &self.doc {
                // cọ mặc định ≈ 1/20 cạnh dài ảnh
                let long = d.size().0.max(d.size().1) as f64;
                if self.clone_source.is_none() {
                    self.clone_size = (long / 20.0).clamp(10.0, 400.0).round();
                }
            }
        }
    }

    fn press_tool_button(&mut self, button: ToolButton) {
        match button {
            ToolButton::Tool(t) => self.select_tool(t),
            ToolButton::InsertImage => self.insert_image(),
            ToolButton::Collage => self.collage.show(self.last_dir.clone()),
        }
    }

    // --- Vẽ -------------------------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui) {
        self.poll_bg_job(ui.ctx());
        self.poll_sr_job(ui.ctx());
        self.check_mask_edit();
        self.ui_menu_bar(ui);
        self.ui_options_bar(ui);
        self.ui_status_bar(ui);
        self.ui_toolbox(ui);
        if self.doc.is_some() {
            self.ui_right_panel(ui);
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
                if self.doc.is_none() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 2.0 - 60.0);
                        widgets::subtitle(ui, &tr("Open a photo to start editing"));
                        ui.add_space(8.0);
                        if widgets::primary(ui, &tr("Open photo"), true).clicked() {
                            self.choose_and_open();
                        }
                        ui.add_space(4.0);
                        if widgets::secondary(ui, &tr("New collage…"), true).clicked() {
                            self.collage.show(self.last_dir.clone());
                        }
                    });
                    return;
                }
                self.ui_canvas(ui);
            });
        if let Some(img) = self.collage.ui(ui.ctx()) {
            if self.collage.last_dir.is_some() {
                self.last_dir = self.collage.last_dir.clone();
            }
            self.set_document(Document::new_untitled(img, "collage"));
        }
        self.handle_dropped_files(ui);
        self.handle_shortcuts(ui);
    }

    /// Kéo thả file ảnh từ Finder / Explorer: chưa mở ảnh → file đầu mở làm nền, các file còn lại
    /// chèn thành layer; đã mở ảnh → tất cả chèn thành layer (như Photoshop "Place").
    fn handle_dropped_files(&mut self, ui: &mut Ui) {
        if self.collage.open || self.mask_edit.is_some() {
            return;
        }
        let (hovering, dropped) = ui.input(|i| {
            (
                !i.raw.hovered_files.is_empty(),
                i.raw
                    .dropped_files
                    .iter()
                    .map(|f| f.path().to_path_buf())
                    .filter(|p| !p.as_os_str().is_empty())
                    .collect::<Vec<PathBuf>>(),
            )
        });
        if hovering {
            let rect = ui.ctx().content_rect();
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("photo_drop_hint"),
            ));
            painter.rect_filled(rect, 0.0, Color32::from_black_alpha(140));
            let hint = if self.doc.is_some() {
                tr("Drop to insert as layers")
            } else {
                tr("Drop to open")
            };
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                hint,
                egui::FontId::proportional(22.0),
                Color32::WHITE,
            );
        }
        if dropped.is_empty() {
            return;
        }
        let mut paths = dropped.as_slice();
        if self.doc.is_none() {
            self.open_path(&paths[0]);
            paths = &paths[1..];
        }
        self.insert_paths(paths);
    }

    /// Thanh menu kiểu Photoshop: File / Edit / Image / View + Xuất, Hoàn tác ở bên phải.
    fn ui_menu_bar(&mut self, ui: &mut Ui) {
        let has = self.doc.is_some();
        let editing = self.mask_edit.is_some();
        let (can_undo, can_redo, has_layer) = match &self.doc {
            Some(d) if !editing => (d.history.can_undo(), d.history.can_redo(), d.selected.is_some()),
            _ => (false, false, false),
        };
        // Đang chỉnh vùng tách nền: không cho đổi hình học ảnh (matte phải khớp từng pixel).
        let geo = has && !editing;
        egui::Panel::top(egui::Id::new("photo_menu"))
            .frame(theme::bar())
            .show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button(tr("File"), |ui| {
                        if menu_item(ui, &tr("Open photo"), "Ctrl+O", true) {
                            self.choose_and_open();
                        }
                        if menu_item(ui, &tr("New collage…"), "L", true) {
                            self.collage.show(self.last_dir.clone());
                        }
                        if menu_item(ui, &tr("Insert image"), "I", has) {
                            self.insert_image();
                        }
                        ui.separator();
                        if menu_item(ui, &tr("Export"), "Ctrl+E", has) {
                            self.export();
                        }
                        if menu_item(ui, &tr("Close photo"), "", has) {
                            self.doc = None;
                            self.composite = None;
                        }
                    });
                    ui.menu_button(tr("Edit"), |ui| {
                        if menu_item(ui, &tr("Undo"), "Ctrl+Z", can_undo) {
                            self.undo();
                        }
                        if menu_item(ui, &tr("Redo"), "Ctrl+Shift+Z", can_redo) {
                            self.redo();
                        }
                        ui.separator();
                        if menu_item(ui, &tr("Duplicate layer"), "Ctrl+J", has_layer) {
                            if let Some(d) = &mut self.doc {
                                d.duplicate_selected();
                            }
                        }
                        if menu_item(ui, &tr("Delete layer"), "Del", has_layer) {
                            if let Some(d) = &mut self.doc {
                                d.delete_selected();
                            }
                        }
                        ui.separator();
                        if let Some(d) = &mut self.doc {
                            if !editing {
                                arrange_menu(ui, d);
                            }
                        }
                    });
                    ui.menu_button(tr("Image"), |ui| {
                        if menu_item(ui, &tr("Rotate 90° left"), "", geo) {
                            if let Some(d) = &mut self.doc {
                                d.rotate90(false);
                            }
                        }
                        if menu_item(ui, &tr("Rotate 90° right"), "", geo) {
                            if let Some(d) = &mut self.doc {
                                d.rotate90(true);
                            }
                        }
                        if menu_item(ui, &tr("Flip horizontal"), "", geo) {
                            if let Some(d) = &mut self.doc {
                                d.flip(true);
                            }
                        }
                        if menu_item(ui, &tr("Flip vertical"), "", geo) {
                            if let Some(d) = &mut self.doc {
                                d.flip(false);
                            }
                        }
                        ui.separator();
                        if menu_item(ui, &tr("Crop"), "C", geo) {
                            self.select_tool(Tool::Crop);
                        }
                        if menu_item(ui, &tr("Remove background"), "", geo && self.bg_job.is_none()) {
                            self.start_remove_background(ui.ctx());
                        }
                        if menu_item(
                            ui,
                            &tr("AI sharpen"),
                            "",
                            geo && self.sr_job.is_none(),
                        ) {
                            self.start_ai_sharpen(ui.ctx());
                        }
                        if menu_item(ui, &tr("Reset adjustments"), "", has) {
                            self.reset_adjustments();
                        }
                    });
                    ui.menu_button(tr("View"), |ui| {
                        if menu_item(ui, &tr("Zoom in"), "Ctrl++", has) {
                            self.zoom_step(1);
                        }
                        if menu_item(ui, &tr("Zoom out"), "Ctrl+-", has) {
                            self.zoom_step(-1);
                        }
                        if menu_item(ui, &tr("Fit on screen"), "Ctrl+0", has) {
                            self.fit_requested = true;
                        }
                        if menu_item(ui, "100%", "Ctrl+1", has) {
                            self.set_zoom(1.0);
                        }
                    });
                    if let Some(d) = &self.doc {
                        ui.add_space(12.0);
                        let mark = if d.modified { " •" } else { "" };
                        ui.label(
                            RichText::new(format!("{}{mark}", d.name))
                                .size(12.0)
                                .color(theme::TEXT_DIM),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::primary(ui, &tr("Export"), has).clicked() {
                            self.export();
                        }
                        if widgets::secondary(ui, "↷", can_redo)
                            .on_hover_text(tr("Redo"))
                            .clicked()
                        {
                            self.redo();
                        }
                        if widgets::secondary(ui, "↶", can_undo)
                            .on_hover_text(tr("Undo"))
                            .clicked()
                        {
                            self.undo();
                        }
                    });
                });
            });
    }

    /// Thanh tuỳ chọn của công cụ đang chọn (như "options bar" của Photoshop).
    fn ui_options_bar(&mut self, ui: &mut Ui) {
        let has = self.doc.is_some();
        egui::Panel::top(egui::Id::new("photo_options"))
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(28.0);
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                    paint_icon(ui.painter(), icon_rect, ToolButton::Tool(self.tool), theme::CYAN);
                    ui.separator();
                    if let Some(edit) = &mut self.mask_edit {
                        let action = mask_controls(ui, edit, true);
                        self.run_mask_action(action);
                        return;
                    }
                    match self.tool {
                        Tool::Move => self.ui_move_options(ui, has),
                        Tool::Crop => self.ui_crop_options(ui),
                        Tool::Blur => self.ui_blur_options(ui),
                        Tool::Clone => self.ui_clone_options(ui),
                        Tool::Hand | Tool::Zoom => {
                            if self.tool == Tool::Zoom {
                                if widgets::secondary(ui, "+", has).clicked() {
                                    self.zoom_step(1);
                                }
                                if widgets::secondary(ui, "−", has).clicked() {
                                    self.zoom_step(-1);
                                }
                            }
                            if widgets::secondary(ui, "100%", has).clicked() {
                                self.set_zoom(1.0);
                            }
                            if widgets::secondary(ui, &tr("Fit on screen"), has).clicked() {
                                self.fit_requested = true;
                            }
                            let hint = if self.tool == Tool::Zoom {
                                tr("Click to zoom in, Alt + click to zoom out")
                            } else {
                                tr("Drag to move around the photo")
                            };
                            widgets::subtitle(ui, &hint);
                        }
                    }
                });
            });
    }

    fn ui_move_options(&mut self, ui: &mut Ui, has: bool) {
        if widgets::secondary(ui, &tr("Insert image"), has).clicked() {
            self.insert_image();
        }
        let Some(doc) = &self.doc else { return };
        match doc.selected_layer() {
            Some(l) => {
                let (_, _, w, h) = l.bounds();
                widgets::subtitle(
                    ui,
                    &format!("{} × {} px", w.round() as i64, h.round() as i64),
                );
                widgets::subtitle(ui, &tr("Drag a corner to resize (Shift = keep ratio)"));
            }
            None => widgets::subtitle(ui, &tr("Click a layer to select it, drag to move")),
        }
    }

    fn ui_crop_options(&mut self, ui: &mut Ui) {
        for (label, _) in CROP_RATIOS {
            if ui
                .selectable_label(self.crop_ratio == label, tr(label))
                .clicked()
            {
                self.crop_ratio = label.to_string();
            }
        }
        ui.separator();
        let pending = self.crop_pending;
        if widgets::primary(ui, &tr("Apply"), pending.is_some()).clicked() {
            self.apply_crop();
        }
        if widgets::secondary(ui, &tr("Cancel"), pending.is_some()).clicked() {
            self.crop_pending = None;
        }
        match pending {
            Some(r) => widgets::subtitle(
                ui,
                &format!("{} × {} px", r.2.round() as i64, r.3.round() as i64),
            ),
            None => widgets::subtitle(ui, &tr("Drag on the photo to choose the crop")),
        }
    }

    fn ui_blur_options(&mut self, ui: &mut Ui) {
        for mode in BLUR_MODES {
            if ui
                .selectable_label(self.blur_mode == mode, tr(mode))
                .clicked()
            {
                self.blur_mode = mode.to_string();
            }
        }
        ui.separator();
        for shape in ["rect", "ellipse"] {
            if ui
                .selectable_label(self.blur_shape == shape, tr(shape))
                .clicked()
            {
                self.blur_shape = shape.to_string();
            }
        }
        ui.separator();
        ui.add(
            egui::Slider::new(&mut self.blur_strength, 1.0..=100.0)
                .step_by(1.0)
                .text(tr("Strength")),
        );
        widgets::subtitle(ui, &tr("Drag on the photo to blur an area"));
    }

    /// Tuỳ chọn Tem nhân bản: cỡ cọ, độ cứng, độ đậm, căn chỉnh (xem clone-stamp.md).
    fn ui_clone_options(&mut self, ui: &mut Ui) {
        let max = self
            .doc
            .as_ref()
            .map(|d| (d.size().0.max(d.size().1) as f64 / 2.0).max(50.0))
            .unwrap_or(500.0);
        ui.add(
            egui::Slider::new(&mut self.clone_size, 2.0..=max)
                .logarithmic(true)
                .step_by(1.0)
                .suffix(" px")
                .text(tr("Brush size")),
        );
        let mut hardness = self.clone_hardness * 100.0;
        if ui
            .add(
                egui::Slider::new(&mut hardness, 0.0..=100.0)
                    .step_by(1.0)
                    .suffix(" %")
                    .text(tr("Hardness")),
            )
            .changed()
        {
            self.clone_hardness = hardness / 100.0;
        }
        let mut opacity = self.clone_opacity * 100.0;
        if ui
            .add(
                egui::Slider::new(&mut opacity, 1.0..=100.0)
                    .step_by(1.0)
                    .suffix(" %")
                    .text(tr("Opacity")),
            )
            .changed()
        {
            self.clone_opacity = opacity / 100.0;
        }
        ui.checkbox(&mut self.clone_aligned, tr("Aligned"))
            .on_hover_text(tr("On: the sample point moves with the brush between strokes. Off: every stroke starts again from the sample point."));
        let hint = if self.clone_source.is_some() {
            tr("Paint to copy the sample · Alt + click = new sample point · [ ] = brush size")
        } else {
            tr("Alt + click on the photo to pick the sample area")
        };
        widgets::subtitle(ui, &hint);
    }

    /// Thanh công cụ dọc bên trái (như Photoshop).
    fn ui_toolbox(&mut self, ui: &mut Ui) {
        let has = self.doc.is_some();
        let mut pressed = None;
        egui::Panel::left(egui::Id::new("photo_toolbox"))
            .exact_size(48.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(6, 8)),
            )
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for entry in TOOLBAR {
                    let Some((button, label, key)) = entry else {
                        ui.add_space(2.0);
                        let (r, _) = ui
                            .allocate_exact_size(egui::vec2(36.0, 1.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 0.0, theme::BORDER);
                        ui.add_space(2.0);
                        continue;
                    };
                    let enabled = (has || button == ToolButton::Collage) && self.mask_edit.is_none();
                    let selected = has && button == ToolButton::Tool(self.tool);
                    let tip = format!("{} ({})", tr(label), key.name());
                    if tool_button(ui, button, selected, enabled)
                        .on_hover_text(tip)
                        .clicked()
                        && enabled
                    {
                        pressed = Some(button);
                    }
                }
            });
        if let Some(b) = pressed {
            self.press_tool_button(b);
        }
    }

    fn ui_status_bar(&mut self, ui: &mut Ui) {
        egui::Panel::bottom(egui::Id::new("photo_status"))
            .frame(
                egui::Frame::new()
                    .fill(theme::SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let Some(doc) = &self.doc else {
                        widgets::subtitle(ui, &tr("No photo open"));
                        return;
                    };
                    let (w, h) = doc.size();
                    let text = |t: String| RichText::new(t).size(11.5).monospace();
                    ui.label(text(format!("{}%", (self.scale * 100.0).round())).color(theme::CYAN));
                    ui.separator();
                    ui.label(text(format!("{w} × {h} px")).color(theme::TEXT_DIM));
                    if let Some((x, y)) = self.cursor {
                        ui.separator();
                        ui.label(
                            text(format!("X {:.0}  Y {:.0}", x.floor(), y.floor()))
                                .color(theme::TEXT_DIM),
                        );
                    }
                    let busy = self.bg_job.is_some();
                    let sr = self.sr_job.as_ref().map(|j| j.progress);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(text(format!("{} {}", doc.state.layers.len(), tr("layers"))).color(theme::TEXT_FAINT));
                        if busy {
                            ui.separator();
                            ui.label(text(tr("Removing background…")).color(theme::CYAN));
                            ui.spinner();
                        }
                        if let Some(p) = sr {
                            ui.separator();
                            ui.label(
                                text(format!("{} {:.0}%", tr("AI sharpening…"), p * 100.0))
                                    .color(theme::CYAN),
                            );
                            ui.spinner();
                        }
                    });
                });
            });
    }

    fn ui_right_panel(&mut self, ui: &mut Ui) {
        egui::Panel::right(egui::Id::new("photo_panels"))
            .default_size(300.0)
            .min_size(260.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(12)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.ui_properties_panel(ui);
                    ui.add_space(6.0);
                    self.ui_cutout_panel(ui);
                    ui.add_space(6.0);
                    self.ui_adjust_panel(ui);
                    ui.add_space(6.0);
                    self.ui_skin_panel(ui);
                    ui.add_space(6.0);
                    self.ui_sharpen_panel(ui);
                    ui.add_space(6.0);
                    self.ui_layers_panel(ui);
                    ui.add_space(6.0);
                    self.ui_export_panel(ui);
                });
            });
    }

    fn ui_adjust_panel(&mut self, ui: &mut Ui) {
        let Some(doc) = &mut self.doc else { return };
        widgets::section(ui, &tr("Adjust"), true, |ui| {
            let mut adjust = doc.state.background.adjust;
            let mut changed = false;
            for spec in ADJUST_SLIDERS.iter() {
                let mut v = adjust.get(spec.key).unwrap_or(0.0);
                if widgets::slider_row(ui, spec, &tr(spec.label), &mut v) {
                    adjust.set(spec.key, v);
                    changed = true;
                }
            }
            let mut angle = doc.state.background.angle;
            let angle_changed =
                widgets::slider_row(ui, &STRAIGHTEN_SPEC, &tr("Straighten"), &mut angle);
            if (changed || angle_changed)
                && (doc.state.background.adjust != adjust || doc.state.background.angle != angle)
            {
                doc.push_undo("Adjust", Some("background_adjust"));
                doc.state.background.adjust = adjust;
                doc.state.background.angle = angle;
            }
            if widgets::secondary(ui, &tr("Reset"), !adjust.is_default() || angle != 0.0).clicked() {
                doc.push_undo("Adjust", None);
                doc.state.background.adjust = PhotoAdjust::default();
                doc.state.background.angle = 0.0;
            }
        });
    }

    /// Mục "Độ nét": Unsharp Mask như Filter › Sharpen › Unsharp Mask (xem sharpen.md).
    fn ui_sharpen_panel(&mut self, ui: &mut Ui) {
        let Some(doc) = &mut self.doc else { return };
        widgets::section(ui, &tr("Sharpen (Unsharp Mask)"), true, |ui| {
            let mut sharpen = doc.state.background.sharpen;
            let mut changed = false;
            for spec in SHARPEN_SLIDERS.iter() {
                let mut v = sharpen.get(spec.key).unwrap_or(spec.default);
                if widgets::slider_row(ui, spec, &tr(spec.label), &mut v) {
                    sharpen.set(spec.key, v);
                    changed = true;
                }
            }
            if changed && doc.state.background.sharpen != sharpen {
                doc.push_undo("Sharpen", Some("background_sharpen"));
                doc.state.background.sharpen = sharpen;
            }
            ui.horizontal(|ui| {
                // Mức gợi ý cho ảnh chụp thường (Photoshop hay dùng 80–150 %, 0.8–1.5 px).
                for (label, amount, radius, threshold) in [
                    ("Light", 60.0, 0.8, 2.0),
                    ("Medium", 120.0, 1.2, 3.0),
                    ("Strong", 200.0, 1.6, 4.0),
                ] {
                    if widgets::secondary(ui, &tr(label), true).clicked() {
                        doc.push_undo("Sharpen", None);
                        doc.state.background.sharpen = SharpenSettings { amount, radius, threshold };
                    }
                }
                if widgets::secondary(ui, &tr("Reset"), !sharpen.is_default()).clicked() {
                    doc.push_undo("Sharpen", None);
                    doc.state.background.sharpen = SharpenSettings::default();
                }
            });
            widgets::subtitle(ui, &tr("Threshold keeps smooth areas (skin, sky) from getting grainy."));
        });
        self.ui_ai_sharpen(ui);
    }

    /// "Làm nét bằng AI": Real-ESRGAN dựng lại chi tiết rồi thu về đúng cỡ ảnh (xem sharpen.md).
    fn ui_ai_sharpen(&mut self, ui: &mut Ui) {
        let running = self.sr_job.as_ref().map(|j| j.progress);
        let mut start = false;
        let mut cancel = false;
        let editing = self.mask_edit.is_some();
        widgets::section(ui, &tr("AI sharpen"), true, |ui| {
            widgets::subtitle(
                ui,
                &tr("AI rebuilds fine detail (good for soft or slightly blurry photos). The photo size stays the same."),
            );
            if let Some(d) = &self.doc {
                let mp = (d.size().0 * d.size().1) as f64 / 1e6;
                widgets::subtitle(
                    ui,
                    &tr("About {0} s for this photo ({1} MP).")
                        .replace("{0}", &format!("{:.0}", (mp * 10.0).max(1.0)))
                        .replace("{1}", &format!("{mp:.1}")),
                );
            }
            ui.add(
                egui::Slider::new(&mut self.ai_strength, 10.0..=100.0)
                    .step_by(1.0)
                    .suffix(" %")
                    .text(tr("AI strength")),
            );
            ui.horizontal(|ui| match running {
                Some(p) => {
                    widgets::progress_bar(ui, p, &format!("{:.0}%", p * 100.0));
                    if widgets::secondary(ui, &tr("Cancel"), true).clicked() {
                        cancel = true;
                    }
                }
                None => {
                    if widgets::primary(ui, &tr("AI sharpen"), !editing).clicked() {
                        start = true;
                    }
                }
            });
        });
        if start {
            self.start_ai_sharpen(ui.ctx());
        }
        if cancel {
            if let Some(j) = &self.sr_job {
                j.cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Chạy "Làm nét bằng AI" ở thread nền; kết quả nhận trong [`Self::poll_sr_job`].
    fn start_ai_sharpen(&mut self, ctx: &egui::Context) {
        if self.sr_job.is_some() {
            return;
        }
        let Some(doc) = &self.doc else { return };
        let img = doc.state.background.img.clone();
        let size = doc.size();
        let strength = (self.ai_strength / 100.0) as f32;
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let stop = cancel.clone();
        std::thread::spawn(move || {
            let result = super_resolver().and_then(|r| {
                let mut last = 0.0f32;
                ai_sharpen(&img, r.as_ref(), strength, &mut |p| {
                    if p - last >= 0.01 || p >= 1.0 {
                        last = p;
                        let _ = tx.send(SrMsg::Progress(p));
                        ctx.request_repaint();
                    }
                    !stop.load(Ordering::Relaxed)
                })
            });
            let _ = tx.send(SrMsg::Done(result));
            ctx.request_repaint();
        });
        self.error = None;
        self.sr_job = Some(SrJob {
            rx,
            progress: 0.0,
            cancel,
            doc_serial: self.doc_serial,
            size,
        });
    }

    fn poll_sr_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.sr_job else { return };
        let mut done = None;
        loop {
            match job.rx.try_recv() {
                Ok(SrMsg::Progress(p)) => job.progress = p,
                Ok(SrMsg::Done(r)) => {
                    done = Some(r);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(Err("AI thread stopped".to_string()));
                    break;
                }
            }
        }
        let Some(result) = done else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        let (serial, size, cancelled) =
            (job.doc_serial, job.size, job.cancel.load(Ordering::Relaxed));
        self.sr_job = None;
        match result {
            Ok(img) => {
                let Some(doc) = &mut self.doc else { return };
                if serial != self.doc_serial || doc.size() != size {
                    self.error = Some(tr("The photo changed while sharpening. Please try again."));
                    return;
                }
                doc.push_undo("AI sharpen", None);
                doc.state.background.img = Arc::new(img);
                self.info = Some(tr("AI sharpen applied (Ctrl+Z to undo)."));
            }
            Err(_) if cancelled => {}
            Err(e) => self.error = Some(format!("{}: {e}", tr("AI sharpen failed"))),
        }
    }

    /// Mục "Làm mịn da" (xem skin-smoothing.md).
    fn ui_skin_panel(&mut self, ui: &mut Ui) {
        let Some(doc) = &mut self.doc else { return };
        widgets::section(ui, &tr("Skin smoothing"), true, |ui| {
            let mut skin = doc.state.background.skin;
            for spec in SKIN_SLIDERS.iter() {
                let mut v = skin.get(spec.key).unwrap_or(spec.default);
                if widgets::slider_row(ui, spec, &tr(spec.label), &mut v) {
                    skin.set(spec.key, v);
                }
            }
            ui.checkbox(&mut skin.skin_only, tr("Skin areas only"))
                .on_hover_text(tr("Off = smooth the whole photo"));
            if skin != doc.state.background.skin {
                doc.push_undo("Skin smoothing", Some("background_skin"));
                doc.state.background.skin = skin;
            }
            ui.horizontal(|ui| {
                for (label, amount, radius, detail) in [
                    ("Light", 35.0, 6.0, 45.0),
                    ("Medium", 60.0, 8.0, 30.0),
                    ("Strong", 85.0, 12.0, 20.0),
                ] {
                    if widgets::secondary(ui, &tr(label), true).clicked() {
                        doc.push_undo("Skin smoothing", None);
                        doc.state.background.skin = SkinSmoothSettings {
                            amount,
                            radius,
                            detail,
                            skin_only: skin.skin_only,
                        };
                    }
                }
                if widgets::secondary(ui, &tr("Reset"), !skin.is_default()).clicked() {
                    doc.push_undo("Skin smoothing", None);
                    doc.state.background.skin = SkinSmoothSettings::default();
                }
            });
            widgets::subtitle(
                ui,
                &tr("Smooths blemishes while keeping eyes, hair and edges sharp."),
            );
        });
    }

    /// Mục "Tách nền": chạy AI, chọn nền mới (trong suốt / màu ảnh thẻ / ảnh), tinh chỉnh viền.
    fn ui_cutout_panel(&mut self, ui: &mut Ui) {
        if let Some(edit) = &mut self.mask_edit {
            let mut action = None;
            widgets::section(ui, &tr("Background removal"), true, |ui| {
                widgets::subtitle(
                    ui,
                    &tr("Red = will be removed. Paint over mistakes, then press Apply."),
                );
                ui.horizontal_wrapped(|ui| action = mask_controls(ui, edit, false));
                widgets::subtitle(
                    ui,
                    &tr("Alt + paint = opposite brush · [ ] = brush size · X = switch Add / Subtract"),
                );
                widgets::subtitle(
                    ui,
                    &tr("Push edge: drag from inside out to grow the selection there, from outside in to shrink it."),
                );
            });
            self.run_mask_action(action);
            return;
        }
        let busy = self.bg_job.is_some();
        let export_png = self.export_fmt == "png";
        let mut start = false;
        let mut pick_image = false;
        let mut edit_selection = false;
        let Some(doc) = &mut self.doc else { return };
        widgets::section(ui, &tr("Background removal"), true, |ui| {
            let Some(cut) = doc.state.background.cutout.clone() else {
                widgets::subtitle(
                    ui,
                    &tr("AI separates the person from the background (works best with portraits)."),
                );
                ui.horizontal(|ui| {
                    if widgets::primary(ui, &tr("Remove background"), !busy).clicked() {
                        start = true;
                    }
                    if busy {
                        ui.spinner();
                    }
                });
                return;
            };

            ui.label(RichText::new(tr("New background")).size(12.0).color(theme::TEXT_DIM));
            let mut backdrop = cut.backdrop.clone();
            let mut merge = None;
            ui.horizontal_wrapped(|ui| {
                if ui
                    .selectable_label(backdrop == Backdrop::Transparent, tr("Transparent"))
                    .clicked()
                {
                    backdrop = Backdrop::Transparent;
                }
                for (label, rgb) in BACKDROP_COLORS {
                    let selected = backdrop == Backdrop::Color(rgb);
                    if color_swatch(ui, rgb, selected).on_hover_text(tr(label)).clicked() {
                        backdrop = Backdrop::Color(rgb);
                    }
                }
                let mut custom = match backdrop {
                    Backdrop::Color(c) => c,
                    _ => [255, 255, 255],
                };
                if ui
                    .color_edit_button_srgb(&mut custom)
                    .on_hover_text(tr("Custom colour"))
                    .changed()
                {
                    backdrop = Backdrop::Color(custom);
                    merge = Some("backdrop_color");
                }
                let is_image = matches!(backdrop, Backdrop::Image(_));
                if ui.selectable_label(is_image, tr("Image…")).clicked() {
                    pick_image = true;
                }
            });
            if backdrop != cut.backdrop {
                doc.push_undo("Background", merge);
                if let Some(c) = &mut doc.state.background.cutout {
                    c.backdrop = backdrop.clone();
                }
            }
            if backdrop == Backdrop::Transparent && !export_png {
                widgets::subtitle(ui, &tr("Export as PNG to keep transparency."));
            }
            ui.add_space(4.0);

            let mut settings = cut.settings;
            let mut changed = false;
            for spec in CUTOUT_SLIDERS.iter() {
                let mut v = settings.get(spec.key).unwrap_or(spec.default);
                if widgets::slider_row(ui, spec, &tr(spec.label), &mut v) {
                    settings.set(spec.key, v);
                    changed = true;
                }
            }
            ui.checkbox(&mut settings.decontaminate, tr("Clean edge colour"))
                .on_hover_text(tr("Removes the old background colour left in hair and edges."));
            if (changed || settings.decontaminate != cut.settings.decontaminate)
                && settings != cut.settings
            {
                doc.push_undo("Refine edge", Some("cutout_refine"));
                if let Some(c) = &mut doc.state.background.cutout {
                    c.settings = settings;
                }
            }
            ui.horizontal(|ui| {
                if widgets::secondary(ui, &tr("Run again"), !busy)
                    .on_hover_text(tr("Run the AI again, e.g. after cropping"))
                    .clicked()
                {
                    start = true;
                }
                if widgets::secondary(ui, &tr("Edit selection"), !busy)
                    .on_hover_text(tr("Show the selection and fix it with a brush"))
                    .clicked()
                {
                    edit_selection = true;
                }
                if widgets::danger(ui, &tr("Restore background"), true).clicked() {
                    doc.push_undo("Restore background", None);
                    doc.state.background.cutout = None;
                }
                if busy {
                    ui.spinner();
                }
            });
            widgets::subtitle(
                ui,
                &tr("Tip: use Insert image to put a PNG (clothes, accessories) over the person."),
            );
        });
        if start {
            self.start_remove_background(ui.ctx());
        }
        if pick_image {
            self.choose_backdrop_image();
        }
        if edit_selection {
            let matte = self
                .doc
                .as_ref()
                .and_then(|d| d.state.background.cutout.as_ref())
                .map(|c| c.matte.clone());
            if let Some(m) = matte {
                self.mask_edit = Some(MaskEdit::new(&m, self.doc_serial));
            }
        }
    }

    /// Huỷ chế độ chỉnh vùng nếu tài liệu đã đổi (mở ảnh khác, đóng ảnh, đổi cỡ).
    fn check_mask_edit(&mut self) {
        let Some(e) = &self.mask_edit else { return };
        let valid = e.doc_serial == self.doc_serial
            && self.doc.as_ref().is_some_and(|d| d.size() == (e.w, e.h));
        if !valid {
            self.mask_edit = None;
            self.mask_base = None;
        }
    }

    fn run_mask_action(&mut self, action: Option<MaskAction>) {
        match action {
            Some(MaskAction::Apply) => self.apply_mask_edit(),
            Some(MaskAction::Cancel) => {
                self.mask_edit = None;
                self.mask_base = None;
            }
            Some(MaskAction::Undo) => {
                if let Some(e) = &mut self.mask_edit {
                    e.undo_stroke();
                }
            }
            Some(MaskAction::Reset) => {
                if let Some(e) = &mut self.mask_edit {
                    e.reset();
                }
            }
            None => {}
        }
    }

    /// Áp vùng đã chỉnh thành tách nền thật (1 bước undo của tài liệu).
    fn apply_mask_edit(&mut self) {
        let Some(edit) = self.mask_edit.take() else { return };
        self.mask_base = None;
        let Some(doc) = &mut self.doc else { return };
        if doc.size() != (edit.w, edit.h) {
            return;
        }
        let matte = edit.into_matte();
        doc.push_undo("Remove background", None);
        let bg = &mut doc.state.background;
        bg.cutout = Some(match bg.cutout.take() {
            // Chạy lại / sửa vùng: giữ nền mới và tinh chỉnh viền đã chọn.
            Some(old) => Cutout { matte, ..old },
            None => Cutout::new(matte),
        });
        bg.visible = true;
    }

    /// Chạy model tách nền ở thread nền; kết quả nhận trong [`Self::poll_bg_job`].
    fn start_remove_background(&mut self, ctx: &egui::Context) {
        if self.bg_job.is_some() {
            return;
        }
        let Some(doc) = &self.doc else { return };
        let img = doc.state.background.img.clone();
        let size = (img.w, img.h);
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = background_remover().and_then(|r| r.matte(&img));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.error = None;
        self.bg_job = Some(BgJob {
            rx,
            doc_serial: self.doc_serial,
            size,
        });
    }

    fn poll_bg_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.bg_job else { return };
        let result = match job.rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => Err("background thread stopped".to_string()),
        };
        let (serial, size) = (job.doc_serial, job.size);
        self.bg_job = None;
        match result {
            Ok(matte) => {
                let Some(doc) = &mut self.doc else { return };
                if serial != self.doc_serial || doc.size() != size {
                    self.error =
                        Some(tr("The photo changed while removing the background. Please try again."));
                    return;
                }
                // Chưa tách thật: hiện vùng AI chọn để người dùng xem và sửa, bấm Áp dụng mới tách.
                self.mask_edit = Some(MaskEdit::new(&matte, serial));
            }
            Err(e) => self.error = Some(format!("{}: {e}", tr("Background removal failed"))),
        }
    }

    fn choose_backdrop_image(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title(tr("Choose background image"))
            .add_filter(tr("Photos"), &OPEN_FILTER_EXTS.map(|e| e.trim_start_matches('.')));
        if let Some(dir) = &self.last_dir {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.pick_file() else { return };
        match load_rgba(&path) {
            Ok(img) => {
                if let Some(d) = &mut self.doc {
                    if d.state.background.cutout.is_some() {
                        d.push_undo("Background", None);
                        if let Some(c) = &mut d.state.background.cutout {
                            c.backdrop = Backdrop::Image(Arc::new(img));
                        }
                    }
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn ui_properties_panel(&mut self, ui: &mut Ui) {
        let Some(doc) = &mut self.doc else { return };
        widgets::section(ui, &tr("Properties"), true, |ui| {
            if doc.selected == Some(BACKGROUND_ID) {
                // Nền: độ trong suốt có tác dụng khi nền nằm trên layer khác.
                let mut opacity = doc.state.background.opacity;
                if ui
                    .add(egui::Slider::new(&mut opacity, 0.0..=1.0).text(tr("Opacity")))
                    .changed()
                {
                    doc.push_undo("Opacity", Some("background_opacity"));
                    doc.state.background.opacity = opacity;
                }
                widgets::subtitle(
                    ui,
                    &tr("Use ▲ ▼ in Layers to put the background above or below other layers."),
                );
                return;
            }
            let Some(layer) = doc.selected_layer().cloned() else {
                widgets::subtitle(ui, &tr("No layer selected"));
                return;
            };
            let mut opacity = layer.opacity();
            if ui
                .add(egui::Slider::new(&mut opacity, 0.0..=1.0).text(tr("Opacity")))
                .changed()
            {
                doc.push_undo("Opacity", Some("layer_opacity"));
                if let Some(l) = doc.selected_layer_mut() {
                    l.set_opacity(opacity);
                }
            }
            if let Layer::Blur(b) = &layer {
                let mut strength = b.strength;
                if widgets::slider_row(ui, &STRENGTH_SPEC, &tr("Strength"), &mut strength) {
                    doc.push_undo("Blur strength", Some("blur_strength"));
                    if let Some(Layer::Blur(l)) = doc.selected_layer_mut() {
                        l.strength = strength;
                    }
                }
                ui.horizontal(|ui| {
                    for mode in BLUR_MODES {
                        if ui.selectable_label(b.mode == mode, tr(mode)).clicked() && b.mode != mode
                        {
                            doc.push_undo("Blur effect", None);
                            if let Some(Layer::Blur(l)) = doc.selected_layer_mut() {
                                l.mode = mode.to_string();
                            }
                        }
                    }
                });
            }
        });
    }

    fn ui_layers_panel(&mut self, ui: &mut Ui) {
        let Some(doc) = &mut self.doc else { return };
        let thumbs = &mut self.thumbs;
        let renaming = &mut self.renaming;
        let title = format!("{} ({})", tr("Layers"), doc.state.layers.len() + 1);
        widgets::section(ui, &title, true, |ui| {
            let mut toggle: Option<u64> = None;
            let mut select: Option<u64> = None;
            let mut drop: Option<(u64, usize)> = None;
            let ids = doc.state.stack_ids();
            // Trên → dưới như Photoshop; "Nền" nằm đúng vị trí của nó, kéo thả để đổi thứ tự.
            for (pos, &id) in ids.iter().enumerate().rev() {
                let (visible, name, thumb) = if id == BACKGROUND_ID {
                    let img = &doc.state.background.img;
                    let tex = layer_thumb(thumbs, ui.ctx(), id, img);
                    (doc.state.background.visible, tr("Background"), Thumb::Image(tex))
                } else {
                    let Some(l) = doc.state.layers.iter().find(|l| l.id() == id) else {
                        continue;
                    };
                    let name = if l.name().is_empty() {
                        tr(l.kind())
                    } else {
                        l.name().to_string()
                    };
                    let thumb = match l {
                        Layer::Image(il) => Thumb::Image(layer_thumb(thumbs, ui.ctx(), id, &il.img)),
                        Layer::Blur(_) => Thumb::Icon(ToolButton::Tool(Tool::Blur)),
                        Layer::Clone(_) => Thumb::Icon(ToolButton::Tool(Tool::Clone)),
                    };
                    (l.visible(), name, thumb)
                };
                let row = ui.horizontal(|ui| {
                    if ui
                        .selectable_label(visible, if visible { "👁" } else { "–" })
                        .clicked()
                    {
                        toggle = Some(id);
                    }
                    // ảnh thu nhỏ trên nền ô vuông (thấy được chỗ trong suốt)
                    let (r, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(r, 3.0, Color32::from_gray(60));
                    match thumb {
                        Thumb::Image(tex) => {
                            let size = tex.size_vec2();
                            let k = (r.width() / size.x).min(r.height() / size.y);
                            let ir = egui::Rect::from_center_size(r.center(), size * k);
                            p.rect_filled(ir, 0.0, Color32::from_gray(200));
                            p.image(tex.id(), ir, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        }
                        Thumb::Icon(b) => paint_icon(p, r.shrink(7.0), b, theme::TEXT_DIM),
                    }
                    let selected = doc.selected == Some(id);
                    if let Some((rid, text)) = renaming.as_mut().filter(|(rid, _)| *rid == id) {
                        let _ = rid;
                        let edit = ui.add(egui::TextEdit::singleline(text).desired_width(140.0));
                        edit.request_focus();
                        let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
                        if esc {
                            *renaming = None;
                        } else if enter || edit.lost_focus() {
                            let new_name = text.clone();
                            *renaming = None;
                            doc.rename_layer(id, &new_name);
                        }
                        return;
                    }
                    let plain = name.clone();
                    let text = if id == BACKGROUND_ID {
                        RichText::new(name).italics()
                    } else {
                        RichText::new(name)
                    };
                    let resp = ui
                        .add(egui::Button::selectable(selected, text).sense(egui::Sense::click_and_drag()))
                        .on_hover_text(tr("Drag to change the order · double-click to rename"));
                    if resp.clicked() || resp.drag_started() {
                        select = Some(id);
                    }
                    if resp.double_clicked() && id != BACKGROUND_ID {
                        *renaming = Some((id, plain));
                    }
                    if resp.drag_started() {
                        resp.dnd_set_drag_payload(id);
                    }
                    if resp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    }
                });
                let rect = row.response.rect;
                let hovered = ui.rect_contains_pointer(rect);
                let dragging = egui::DragAndDrop::payload::<u64>(ui.ctx());
                if let (true, Some(src)) = (hovered, dragging) {
                    // vạch chỉ chỗ thả: kéo lên → mục nằm trên dòng này, kéo xuống → nằm dưới
                    let from = ids.iter().position(|&i| i == *src).unwrap_or(pos);
                    let y = if from < pos { rect.top() } else { rect.bottom() };
                    if from != pos {
                        ui.painter().hline(rect.x_range(), y, Stroke::new(2.0, theme::CYAN));
                    }
                    if ui.input(|i| i.pointer.any_released()) {
                        if let Some(p) = egui::DragAndDrop::take_payload::<u64>(ui.ctx()) {
                            drop = Some((*p, pos));
                        }
                    }
                }
            }
            if let Some(id) = toggle {
                doc.push_undo("Show / hide layer", None);
                if id == BACKGROUND_ID {
                    doc.state.background.visible = !doc.state.background.visible;
                } else if let Some(l) = doc.state.layers.iter_mut().find(|l| l.id() == id) {
                    let v = l.visible();
                    l.set_visible(!v);
                }
            }
            if let Some(id) = select {
                doc.selected = Some(id);
            }
            if let Some((id, target)) = drop {
                doc.move_to(id, target);
                doc.selected = Some(id);
            }
            let has = doc.selected.is_some();
            let is_bg = doc.selected == Some(BACKGROUND_ID);
            ui.horizontal(|ui| {
                if widgets::secondary(ui, "⏶", has)
                    .on_hover_text(tr("Bring to front (Ctrl+Shift+])"))
                    .clicked()
                {
                    doc.move_selected_to_end(true);
                }
                if widgets::secondary(ui, "▲", has)
                    .on_hover_text(tr("Bring forward (Ctrl+])"))
                    .clicked()
                {
                    doc.move_selected(1);
                }
                if widgets::secondary(ui, "▼", has)
                    .on_hover_text(tr("Send backward (Ctrl+[)"))
                    .clicked()
                {
                    doc.move_selected(-1);
                }
                if widgets::secondary(ui, "⏷", has)
                    .on_hover_text(tr("Send to back (Ctrl+Shift+[)"))
                    .clicked()
                {
                    doc.move_selected_to_end(false);
                }
            });
            ui.horizontal(|ui| {
                if widgets::secondary(ui, &tr("Duplicate layer"), has && !is_bg).clicked() {
                    doc.duplicate_selected();
                }
                if widgets::danger(ui, &tr("Delete layer"), has && !is_bg).clicked() {
                    doc.delete_selected();
                }
            });
            if doc.state.bg_pos() > 0
                && doc.state.background.cutout.is_none()
                && doc.state.background.opacity >= 1.0
            {
                widgets::subtitle(
                    ui,
                    &tr("An opaque background covers the layers below it — remove its background or lower its opacity to see them."),
                );
            }
        });
    }

    fn ui_export_panel(&mut self, ui: &mut Ui) {
        widgets::section(ui, &tr("Export"), false, |ui| {
            ui.horizontal(|ui| {
                for (fmt, label) in [("jpeg", "JPEG"), ("png", "PNG")] {
                    if ui.selectable_label(self.export_fmt == fmt, label).clicked() {
                        self.export_fmt = fmt.to_string();
                    }
                }
            });
            if self.export_fmt == "jpeg" {
                let mut q = self.export_quality as f64;
                ui.add(egui::Slider::new(&mut q, 1.0..=100.0).text(tr("Quality")));
                self.export_quality = q.round() as u8;
            }
            if widgets::primary(ui, &tr("Export"), self.doc.is_some()).clicked() {
                self.export();
            }
        });
    }

    // --- Canvas ---------------------------------------------------------------------------

    fn ui_canvas(&mut self, ui: &mut Ui) {
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
        let (iw, ih) = self.doc.as_ref().map(|d| d.size()).unwrap_or((1, 1));

        if self.fit_requested {
            self.scale = fit_scale(iw, ih, rect.width() as i64, rect.height() as i64, FIT_MARGIN);
            self.offset = (
                (rect.width() as f64 - iw as f64 * self.scale) / 2.0,
                (rect.height() as f64 - ih as f64 * self.scale) / 2.0,
            );
            self.fit_requested = false;
        }

        // Zoom bằng con lăn, giữ điểm dưới con trỏ.
        let scroll = ui.input(|i| i.smooth_scroll_delta.y) as f64;
        if resp.hovered() && scroll.abs() > 0.5 {
            if let Some(pos) = resp.hover_pos() {
                let anchor = (
                    (pos.x - rect.min.x) as f64,
                    (pos.y - rect.min.y) as f64,
                );
                let target = clamp_zoom(self.scale * (1.0 + scroll / 400.0));
                self.offset = zoom_at(self.scale, target, self.offset, anchor);
                self.scale = target;
            }
        }
        self.cursor = resp
            .hover_pos()
            .map(|p| self.to_image(rect, p))
            .filter(|&(x, y)| x >= 0.0 && y >= 0.0 && x < iw as f64 && y < ih as f64);

        self.rebuild_composite();
        self.handle_canvas_input(&resp, rect);
        self.paint_canvas(ui, rect, iw, ih);
        if self.mask_edit.is_none() && self.tool == Tool::Move {
            if let Some(d) = &mut self.doc {
                resp.context_menu(|ui| {
                    arrange_menu(ui, d);
                    ui.separator();
                    let is_layer = d.selected.is_some_and(|id| id != BACKGROUND_ID);
                    if menu_item(ui, &tr("Duplicate layer"), "Ctrl+J", is_layer) {
                        d.duplicate_selected();
                    }
                    if menu_item(ui, &tr("Delete layer"), "Del", is_layer) {
                        d.delete_selected();
                    }
                });
            }
        }
    }

    fn rebuild_composite(&mut self) {
        let Some(doc) = &self.doc else { return };
        if self.mask_edit.is_some() {
            if self.mask_base.is_none() || self.mask_base_rev != doc.revision {
                self.mask_base = Some(render::render_mask_base_cached(
                    &doc.state,
                    self.preview_scale,
                    &mut self.base_cache,
                ));
                self.mask_base_rev = doc.revision;
            }
            return;
        }
        if self.composite.is_some() && self.composite_rev == doc.revision {
            return;
        }
        self.composite = Some(render::render_cached(
            &doc.state,
            self.preview_scale,
            &mut self.base_cache,
        ));
        self.composite_rev = doc.revision;
    }

    fn to_image(&self, rect: egui::Rect, pos: Pos2) -> (f64, f64) {
        (
            ((pos.x - rect.min.x) as f64 - self.offset.0) / self.scale,
            ((pos.y - rect.min.y) as f64 - self.offset.1) / self.scale,
        )
    }

    fn to_screen(&self, rect: egui::Rect, x: f64, y: f64) -> Pos2 {
        Pos2::new(
            rect.min.x + (self.offset.0 + x * self.scale) as f32,
            rect.min.y + (self.offset.1 + y * self.scale) as f32,
        )
    }

    /// Nút góc (0 = trên-trái, 1 = trên-phải, 2 = dưới-phải, 3 = dưới-trái) của layer đang
    /// chọn nằm dưới `pos` (chỉ ở công cụ Move).
    fn handle_at(&self, rect: egui::Rect, pos: Pos2) -> Option<(u64, usize, RectF)> {
        if self.tool != Tool::Move {
            return None;
        }
        let layer = self.doc.as_ref()?.selected_layer()?;
        if !layer.resizable() {
            return None;
        }
        let b = layer.bounds();
        corners(b)
            .iter()
            .position(|&(x, y)| self.to_screen(rect, x, y).distance(pos) <= HANDLE_HIT)
            .map(|i| (layer.id(), i, b))
    }

    fn handle_canvas_input(&mut self, resp: &egui::Response, rect: egui::Rect) {
        let (space, alt, shift, middle) = resp.ctx.input(|i| {
            (
                i.key_down(Key::Space),
                i.modifiers.alt,
                i.modifiers.shift,
                i.pointer.middle_down(),
            )
        });
        let pan_override = space || middle;

        // Con trỏ chuột theo công cụ.
        if let Some(pos) = resp.hover_pos() {
            let icon = if (self.mask_edit.is_some() || self.tool == Tool::Clone) && !pan_override {
                egui::CursorIcon::Crosshair
            } else if pan_override || self.tool == Tool::Hand {
                if resp.dragged() {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::Grab
                }
            } else if let Some((_, corner, _)) = self.handle_at(rect, pos) {
                if corner % 2 == 0 {
                    egui::CursorIcon::ResizeNwSe
                } else {
                    egui::CursorIcon::ResizeNeSw
                }
            } else {
                match self.tool {
                    Tool::Zoom if alt => egui::CursorIcon::ZoomOut,
                    Tool::Zoom => egui::CursorIcon::ZoomIn,
                    Tool::Crop | Tool::Blur => egui::CursorIcon::Crosshair,
                    _ => egui::CursorIcon::Default,
                }
            };
            resp.ctx.set_cursor_icon(icon);
        }

        // Nhả chuột ở đâu cũng kết thúc nét cọ.
        if !resp.ctx.input(|i| i.pointer.primary_down()) {
            if let Some(e) = &mut self.mask_edit {
                e.end_stroke();
            }
        }
        let Some(pos) = resp.interact_pointer_pos() else {
            if !resp.dragged() {
                self.drag = None;
            }
            return;
        };
        let img_pt = self.to_image(rect, pos);

        // Chuột phải trên canvas (Move): chọn layer dưới con trỏ để menu Sắp xếp áp cho nó.
        if resp.secondary_clicked() && self.mask_edit.is_none() && self.tool == Tool::Move {
            let hit = self.doc.as_ref().and_then(|d| d.layer_at(img_pt.0, img_pt.1));
            if let Some(d) = &mut self.doc {
                d.selected = hit.or(Some(BACKGROUND_ID));
            }
        }

        // Tem nhân bản: Alt + bấm = chọn điểm mẫu; kéo = tô mẫu lên chỗ khác.
        if self.tool == Tool::Clone && self.mask_edit.is_none() && !pan_override {
            self.handle_clone_input(resp, rect, img_pt, alt);
            return;
        }

        // Chỉnh vùng tách nền: chuột trái = tô cọ (Alt = cọ ngược lại); Space / chuột giữa vẫn cuộn.
        if self.mask_edit.is_some() && !pan_override {
            let start = resp
                .ctx
                .input(|i| i.pointer.press_origin())
                .map(|p| self.to_image(rect, p))
                .unwrap_or(img_pt);
            if let Some(e) = &mut self.mask_edit {
                if resp.drag_started() {
                    e.begin_stroke(start, alt);
                    e.stroke_to(img_pt, alt);
                } else if resp.dragged() {
                    e.stroke_to(img_pt, alt);
                } else if resp.clicked() {
                    e.begin_stroke(img_pt, alt);
                    e.end_stroke();
                }
                if resp.drag_stopped() {
                    e.end_stroke();
                }
            }
            return;
        }

        // Công cụ Zoom: bấm = phóng to tại điểm bấm, Alt + bấm = thu nhỏ.
        if resp.clicked() && self.tool == Tool::Zoom && !pan_override {
            let target = clamp_zoom(next_zoom(self.scale, if alt { -1 } else { 1 }));
            let anchor = ((pos.x - rect.min.x) as f64, (pos.y - rect.min.y) as f64);
            self.offset = zoom_at(self.scale, target, self.offset, anchor);
            self.scale = target;
            return;
        }
        // Công cụ Move: bấm vào chỗ trống = bỏ chọn layer.
        if resp.clicked() && self.tool == Tool::Move && !pan_override {
            let hit = self.doc.as_ref().and_then(|d| d.layer_at(img_pt.0, img_pt.1));
            if let Some(d) = &mut self.doc {
                d.selected = hit;
            }
        }

        if resp.drag_started() {
            self.drag = if pan_override {
                Some(Drag::Pan { last: pos })
            } else {
                match self.tool {
                    Tool::Hand | Tool::Zoom | Tool::Clone => Some(Drag::Pan { last: pos }),
                    Tool::Crop | Tool::Blur => Some(Drag::Rect { anchor: img_pt }),
                    Tool::Move => {
                        if let Some((id, corner, start)) = self.handle_at(rect, pos) {
                            Some(Drag::Resize { id, corner, start })
                        } else {
                            let hit = self
                                .doc
                                .as_ref()
                                .and_then(|d| d.layer_at(img_pt.0, img_pt.1));
                            if let Some(d) = &mut self.doc {
                                d.selected = hit;
                            }
                            match hit {
                                Some(id) => Some(Drag::MoveLayer { id, last: img_pt }),
                                None => Some(Drag::Pan { last: pos }),
                            }
                        }
                    }
                }
            };
        }

        if resp.dragged() {
            let (iw, ih) = self.doc.as_ref().map(|d| d.size()).unwrap_or((1, 1));
            match &mut self.drag {
                Some(Drag::Pan { last }) => {
                    self.offset.0 += (pos.x - last.x) as f64;
                    self.offset.1 += (pos.y - last.y) as f64;
                    *last = pos;
                }
                Some(Drag::MoveLayer { id, last }) => {
                    let (dx, dy) = (img_pt.0 - last.0, img_pt.1 - last.1);
                    *last = img_pt;
                    let id = *id;
                    if let Some(d) = &mut self.doc {
                        d.push_undo("Move layer", Some("move_layer"));
                        if let Some(l) = d.state.layers.iter_mut().find(|l| l.id() == id) {
                            l.translate(dx, dy);
                        }
                    }
                }
                Some(Drag::Resize { id, corner, start }) => {
                    let (x, y, w, h) = resize_from_corner(*start, *corner, img_pt, shift);
                    let id = *id;
                    if let Some(d) = &mut self.doc {
                        d.push_undo("Resize layer", Some("resize_layer"));
                        if let Some(l) = d.state.layers.iter_mut().find(|l| l.id() == id) {
                            l.set_bounds(x, y, w, h);
                        }
                    }
                }
                Some(Drag::Rect { anchor }) => {
                    let ratio = if matches!(self.tool, Tool::Crop) {
                        crop_ratio(&self.crop_ratio)
                    } else {
                        None
                    };
                    self.crop_pending = Some(crop_rect(*anchor, img_pt, ratio, (iw, ih)));
                }
                None => {}
            }
        }

        if resp.drag_stopped() {
            if let (Some(Drag::Rect { .. }), Some(r)) = (&self.drag, self.crop_pending) {
                if matches!(self.tool, Tool::Blur) {
                    if r.2 >= 4.0 && r.3 >= 4.0 {
                        let (mode, strength, shape) =
                            (self.blur_mode.clone(), self.blur_strength, self.blur_shape.clone());
                        if let Some(d) = &mut self.doc {
                            d.add_blur_layer(r, &mode, strength);
                            if let Some(Layer::Blur(l)) = d.selected_layer_mut() {
                                l.shape = shape;
                            }
                        }
                    }
                    self.crop_pending = None;
                }
            }
            if let Some(d) = &mut self.doc {
                d.history.break_merge();
            }
            self.drag = None;
        }
    }

    fn handle_clone_input(
        &mut self,
        resp: &egui::Response,
        rect: egui::Rect,
        img_pt: (f64, f64),
        alt: bool,
    ) {
        if alt {
            if resp.clicked() || resp.drag_started() {
                self.clone_source = Some(img_pt);
                self.clone_offset = None;
                self.info = None;
            }
            return;
        }
        if resp.drag_started() || resp.clicked() {
            let start = resp
                .ctx
                .input(|i| i.pointer.press_origin())
                .map(|p| self.to_image(rect, p))
                .unwrap_or(img_pt);
            match self.clone_source {
                None => {
                    self.info = Some(tr("Alt + click on the photo to pick the sample area"));
                }
                Some(src) => {
                    let offset = match (self.clone_aligned, self.clone_offset) {
                        (true, Some(o)) => o,
                        _ => (src.0 - start.0, src.1 - start.1),
                    };
                    self.clone_offset = Some(offset);
                    self.begin_clone_stroke(start, offset);
                    if resp.drag_started() {
                        self.extend_clone_stroke(img_pt);
                    }
                }
            }
        } else if resp.dragged() {
            self.extend_clone_stroke(img_pt);
        }
        if resp.clicked() || resp.drag_stopped() {
            self.clone_painting = None;
            if let Some(d) = &mut self.doc {
                d.history.break_merge();
            }
        }
    }

    /// Bắt đầu một nét Tem nhân bản: vào layer nhân bản đang chọn, hoặc tạo layer mới trên cùng.
    fn begin_clone_stroke(&mut self, start: (f64, f64), offset: (f64, f64)) {
        let (radius, hardness, opacity) =
            (self.clone_size / 2.0, self.clone_hardness, self.clone_opacity);
        let Some(d) = &mut self.doc else { return };
        d.history.break_merge();
        d.push_undo("Clone stamp", Some("clone_stroke"));
        let id = match d.selected_layer() {
            Some(Layer::Clone(l)) => l.id,
            _ => d.add_clone_layer(),
        };
        if let Some(Layer::Clone(l)) = d.state.layers.iter_mut().find(|l| l.id() == id) {
            l.strokes.push(CloneStroke {
                offset,
                points: vec![start],
                radius,
                hardness,
                opacity,
            });
            l.visible = true;
        }
        self.clone_painting = Some(id);
    }

    fn extend_clone_stroke(&mut self, pt: (f64, f64)) {
        let Some(id) = self.clone_painting else { return };
        let min_step = (self.clone_size * 0.05).max(1.0);
        let Some(d) = &mut self.doc else { return };
        let far_enough = d.state.layers.iter().find(|l| l.id() == id).is_some_and(|l| match l {
            Layer::Clone(c) => c.strokes.last().and_then(|s| s.points.last()).is_none_or(|p| {
                ((p.0 - pt.0).powi(2) + (p.1 - pt.1).powi(2)).sqrt() >= min_step
            }),
            _ => false,
        });
        if !far_enough {
            return;
        }
        d.push_undo("Clone stamp", Some("clone_stroke"));
        if let Some(Layer::Clone(l)) = d.state.layers.iter_mut().find(|l| l.id() == id) {
            if let Some(st) = l.strokes.last_mut() {
                st.points.push(pt);
            }
        }
    }

    fn paint_canvas(&mut self, ui: &mut Ui, rect: egui::Rect, iw: usize, ih: usize) {
        let ppp = ui.ctx().pixels_per_point() as f64;
        let (pw, ph) = (
            (rect.width() as f64 * ppp) as usize,
            (rect.height() as f64 * ppp) as usize,
        );
        let composite = match (&mut self.mask_edit, &self.mask_base) {
            (Some(e), Some(base)) => Some(e.shown(base, self.mask_base_rev).clone()),
            _ => self.composite.clone(),
        };
        let (scale, offset) = (self.scale, self.offset);
        let pending = self.crop_pending;
        let (selected_bounds, show_handles) = if self.tool == Tool::Move {
            match self.doc.as_ref().and_then(|d| d.selected_layer()) {
                Some(l) => (Some(l.bounds()), l.resizable()),
                None => (None, false),
            }
        } else {
            (None, false)
        };

        let fb = self.canvas.begin(pw.max(1), ph.max(1));
        fb.clear([0x0b, 0x10, 0x20, 255]);
        // Ô vuông trong suốt (như Photoshop) chỉ ở vùng của ảnh.
        let (dx, dy) = (offset.0 * ppp, offset.1 * ppp);
        let (dw, dh) = (iw as f64 * scale * ppp, ih as f64 * scale * ppp);
        fb.checker_rect(
            dx as f32,
            dy as f32,
            dw as f32,
            dh as f32,
            (8.0 * ppp) as usize,
            [0xcc, 0xcc, 0xcc, 255],
            [0xff, 0xff, 0xff, 255],
        );
        if let Some(img) = &composite {
            fb.draw_image(img, dx as f32, dy as f32, dw as f32, dh as f32, 1.0);
        }
        fb.stroke_rect(
            dx as f32,
            dy as f32,
            dw as f32,
            dh as f32,
            1.0,
            [0x2a, 0x3a, 0x5c, 255],
        );
        // Khung chọn của layer + 4 nút góc để đổi cỡ.
        if let Some((bx, by, bw, bh)) = selected_bounds {
            let (sx, sy) = (dx + bx * scale * ppp, dy + by * scale * ppp);
            let (sw, sh) = (bw * scale * ppp, bh * scale * ppp);
            fb.stroke_rect(sx as f32, sy as f32, sw as f32, sh as f32, 1.5, [0x4f, 0x8c, 0xff, 255]);
            let k = (4.0 * ppp) as f32;
            let handles = if show_handles { corners((sx, sy, sw, sh)).to_vec() } else { vec![] };
            for (cx, cy) in handles {
                let (cx, cy) = (cx as f32, cy as f32);
                fb.fill_rect(cx - k, cy - k, 2.0 * k, 2.0 * k, [255, 255, 255, 255]);
                fb.stroke_rect(cx - k, cy - k, 2.0 * k, 2.0 * k, 1.0, [0x4f, 0x8c, 0xff, 255]);
            }
        }
        // Khung crop / blur đang kéo: tối phần ngoài như Photoshop.
        if let Some((cx, cy, cw, ch)) = pending {
            let (rx, ry) = (dx + cx * scale * ppp, dy + cy * scale * ppp);
            let (rw, rh) = (cw * scale * ppp, ch * scale * ppp);
            let shade = [0x00, 0x00, 0x00, 110];
            fb.fill_rect(dx as f32, dy as f32, dw as f32, (ry - dy) as f32, shade);
            fb.fill_rect(
                dx as f32,
                (ry + rh) as f32,
                dw as f32,
                (dy + dh - ry - rh) as f32,
                shade,
            );
            fb.fill_rect(dx as f32, ry as f32, (rx - dx) as f32, rh as f32, shade);
            fb.fill_rect(
                (rx + rw) as f32,
                ry as f32,
                (dx + dw - rx - rw) as f32,
                rh as f32,
                shade,
            );
            fb.stroke_rect(rx as f32, ry as f32, rw as f32, rh as f32, 1.0, [255, 255, 255, 230]);
            // Lưới 1/3 khi cắt (như Photoshop).
            if self.tool == Tool::Crop {
                let grid = [255, 255, 255, 90];
                for i in 1..3 {
                    let t = i as f64 / 3.0;
                    fb.fill_rect((rx + rw * t) as f32, ry as f32, 1.0, rh as f32, grid);
                    fb.fill_rect(rx as f32, (ry + rh * t) as f32, rw as f32, 1.0, grid);
                }
            }
        }
        self.canvas.paint(ui, rect);
        // Tem nhân bản: vòng cọ dưới con trỏ + dấu + ở điểm đang lấy mẫu.
        if self.tool == Tool::Clone && self.mask_edit.is_none() {
            let painter = ui.painter_at(rect);
            let hover = ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p));
            if let Some(pos) = hover {
                let r = (self.clone_size / 2.0 * scale) as f32;
                painter.circle_stroke(pos, r, Stroke::new(2.5, Color32::from_black_alpha(160)));
                painter.circle_stroke(pos, r, Stroke::new(1.0, Color32::WHITE));
            }
            let src = match (self.clone_offset, hover) {
                // đã có độ lệch (căn chỉnh hoặc đang tô): điểm mẫu đi theo con trỏ
                (Some(o), Some(pos)) if self.clone_aligned || self.clone_painting.is_some() => {
                    let p = self.to_image(rect, pos);
                    Some((p.0 + o.0, p.1 + o.1))
                }
                _ => self.clone_source,
            };
            if let Some((x, y)) = src {
                let c = self.to_screen(rect, x, y);
                let k = 7.0;
                for (stroke, color) in [(3.0, Color32::from_black_alpha(170)), (1.2, Color32::WHITE)] {
                    let st = Stroke::new(stroke, color);
                    painter.line_segment([c - egui::vec2(k, 0.0), c + egui::vec2(k, 0.0)], st);
                    painter.line_segment([c - egui::vec2(0.0, k), c + egui::vec2(0.0, k)], st);
                }
                let r = (self.clone_size / 2.0 * scale) as f32;
                painter.circle_stroke(c, r, Stroke::new(1.0, Color32::from_white_alpha(120)));
            }
        }
        // Vòng tròn cỡ cọ dưới con trỏ (chế độ chỉnh vùng tách nền).
        if let (Some(e), Some(pos)) = (&self.mask_edit, ui.ctx().pointer_hover_pos()) {
            if rect.contains(pos) {
                let r = (e.size / 2.0 * scale) as f32;
                let painter = ui.painter_at(rect);
                painter.circle_stroke(pos, r, Stroke::new(2.5, Color32::from_black_alpha(160)));
                painter.circle_stroke(pos, r, Stroke::new(1.0, Color32::WHITE));
                let invert = ui.input(|i| i.modifiers.alt);
                let sign = if invert { e.brush.opposite() } else { e.brush }.sign();
                painter.text(
                    pos + egui::vec2(r * 0.71 + 6.0, -(r * 0.71) - 6.0),
                    egui::Align2::LEFT_BOTTOM,
                    sign,
                    egui::FontId::proportional(16.0),
                    Color32::WHITE,
                );
            }
        }
    }

    // --- Lệnh -----------------------------------------------------------------------------

    fn handle_shortcuts(&mut self, ui: &mut Ui) {
        // Cửa sổ collage đang mở → phím thuộc về nó.
        if self.collage.open {
            return;
        }
        let typing = ui.ctx().egui_wants_keyboard_input();
        let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
        if cmd && ui.input(|i| i.key_pressed(Key::O)) {
            self.choose_and_open();
        }
        let editing = self.mask_edit.is_some();
        if editing && !typing {
            self.handle_mask_shortcuts(ui, cmd);
        }
        // Phím chữ cái chọn công cụ (khi không gõ trong ô nhập).
        if !typing && !cmd && !editing {
            let key = TOOLBAR
                .iter()
                .flatten()
                .find(|(_, _, key)| ui.input(|i| i.key_pressed(*key)))
                .map(|(b, _, _)| *b);
            if let Some(b) = key {
                if self.doc.is_some() || b == ToolButton::Collage {
                    self.press_tool_button(b);
                }
            }
        }
        if self.doc.is_none() {
            return;
        }
        let (undo, redo, fit, hundred, zoom_in, zoom_out, delete, export, dup) = ui.input(|i| {
            (
                cmd && !shift && i.key_pressed(Key::Z),
                cmd && shift && i.key_pressed(Key::Z),
                cmd && i.key_pressed(Key::Num0),
                cmd && i.key_pressed(Key::Num1),
                cmd && (i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals)),
                cmd && i.key_pressed(Key::Minus),
                !typing && (i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)),
                cmd && i.key_pressed(Key::E),
                cmd && i.key_pressed(Key::J),
            )
        });
        if editing {
            // Ctrl+Z / Enter / Esc / Delete… thuộc về chế độ chỉnh vùng (handle_mask_shortcuts).
            if fit {
                self.fit_requested = true;
            }
            if hundred {
                self.set_zoom(1.0);
            }
            if zoom_in {
                self.zoom_step(1);
            }
            if zoom_out {
                self.zoom_step(-1);
            }
            let _ = (undo, redo, delete, dup, export);
            return;
        }
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if fit {
            self.fit_requested = true;
        }
        if hundred {
            self.set_zoom(1.0);
        }
        if zoom_in {
            self.zoom_step(1);
        }
        if zoom_out {
            self.zoom_step(-1);
        }
        if delete {
            if let Some(d) = &mut self.doc {
                d.delete_selected();
            }
        }
        if dup {
            if let Some(d) = &mut self.doc {
                d.duplicate_selected();
            }
        }
        if export {
            self.export();
        }
        // Sắp xếp: Ctrl+] / Ctrl+[ lên / xuống một bậc, thêm Shift = lên trên cùng / xuống dưới cùng.
        let (up, down) = ui.input(|i| {
            (cmd && i.key_pressed(Key::CloseBracket), cmd && i.key_pressed(Key::OpenBracket))
        });
        if let Some(d) = &mut self.doc {
            match (up, down, shift) {
                (true, _, true) => d.move_selected_to_end(true),
                (true, _, false) => d.move_selected(1),
                (_, true, true) => d.move_selected_to_end(false),
                (_, true, false) => d.move_selected(-1),
                _ => {}
            }
        }
        // [ ] đổi cỡ cọ của Tem nhân bản.
        if !typing && !cmd && self.tool == Tool::Clone {
            let (smaller, bigger) = ui.input(|i| {
                (i.key_pressed(Key::OpenBracket), i.key_pressed(Key::CloseBracket))
            });
            if smaller {
                self.clone_size = (self.clone_size / 1.2).max(2.0).round();
            }
            if bigger {
                self.clone_size = (self.clone_size * 1.2).min(2000.0).round();
            }
        }
        // Mũi tên nhích layer 1 px (Shift = 10 px) ở công cụ Move.
        if !typing && self.tool == Tool::Move {
            let step = if shift { 10.0 } else { 1.0 };
            let (dx, dy) = ui.input(|i| {
                let mut d = (0.0, 0.0);
                if i.key_pressed(Key::ArrowLeft) {
                    d.0 -= step;
                }
                if i.key_pressed(Key::ArrowRight) {
                    d.0 += step;
                }
                if i.key_pressed(Key::ArrowUp) {
                    d.1 -= step;
                }
                if i.key_pressed(Key::ArrowDown) {
                    d.1 += step;
                }
                d
            });
            if dx != 0.0 || dy != 0.0 {
                if let Some(d) = &mut self.doc {
                    if d.selected_layer().is_some() {
                        d.push_undo("Nudge layer", Some("nudge_layer"));
                        if let Some(l) = d.selected_layer_mut() {
                            l.translate(dx, dy);
                        }
                    }
                }
            }
        }
        if ui.input(|i| i.key_pressed(Key::Enter)) && self.crop_pending.is_some() {
            self.apply_crop();
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.crop_pending = None;
        }
    }

    /// Phím của chế độ chỉnh vùng tách nền: Enter áp dụng, Esc huỷ, Ctrl+Z hoàn tác nét,
    /// [ ] đổi cỡ cọ, X đổi Thêm / Bớt.
    fn handle_mask_shortcuts(&mut self, ui: &mut Ui, cmd: bool) {
        let (enter, esc, undo, smaller, bigger, swap) = ui.input(|i| {
            (
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
                cmd && !i.modifiers.shift && i.key_pressed(Key::Z),
                i.key_pressed(Key::OpenBracket),
                i.key_pressed(Key::CloseBracket),
                !cmd && i.key_pressed(Key::X),
            )
        });
        if let Some(e) = &mut self.mask_edit {
            let max = brush_max(e);
            if smaller {
                e.size = (e.size / 1.2).max(2.0).round();
            }
            if bigger {
                e.size = (e.size * 1.2).min(max).round();
            }
            if swap {
                e.brush = e.brush.opposite();
            }
        }
        if undo {
            self.run_mask_action(Some(MaskAction::Undo));
        }
        if enter {
            self.run_mask_action(Some(MaskAction::Apply));
        } else if esc {
            self.run_mask_action(Some(MaskAction::Cancel));
        }
    }

    fn undo(&mut self) {
        if let Some(d) = &mut self.doc {
            d.undo();
        }
    }

    fn redo(&mut self) {
        if let Some(d) = &mut self.doc {
            d.redo();
        }
    }

    fn reset_adjustments(&mut self) {
        if let Some(d) = &mut self.doc {
            d.push_undo("Reset adjustments", None);
            d.state.background.adjust = PhotoAdjust::default();
            d.state.background.sharpen = SharpenSettings::default();
            d.state.background.skin = SkinSmoothSettings::default();
            d.state.background.angle = 0.0;
        }
    }

    fn set_zoom(&mut self, scale: f64) {
        let target = clamp_zoom(scale);
        self.offset = zoom_at(self.scale, target, self.offset, (0.0, 0.0));
        self.scale = target;
    }

    fn zoom_step(&mut self, direction: i32) {
        self.set_zoom(next_zoom(self.scale, direction));
    }

    fn apply_crop(&mut self) {
        let Some(r) = self.crop_pending.take() else { return };
        let Some(doc) = &mut self.doc else { return };
        let (iw, ih) = doc.size();
        let (x, y, w, h) = round_rect(r, (iw, ih));
        doc.crop(x, y, w, h);
        self.fit_requested = true;
        self.tool = Tool::Move;
    }

    fn insert_image(&mut self) {
        if self.doc.is_none() {
            return;
        }
        let mut dialog = rfd::FileDialog::new().set_title(tr("Insert image")).add_filter(
            tr("Photos"),
            &OPEN_FILTER_EXTS.map(|e| e.trim_start_matches('.')),
        );
        if let Some(dir) = &self.last_dir {
            dialog = dialog.set_directory(dir);
        }
        // chọn được nhiều ảnh cùng lúc (Shift / Cmd + bấm) → mỗi ảnh một layer
        let Some(paths) = dialog.pick_files() else { return };
        self.insert_paths(&paths);
    }

    /// Chèn từng ảnh thành một layer (ảnh sau đè lên ảnh trước, lệch nhau một chút để thấy hết).
    fn insert_paths(&mut self, paths: &[PathBuf]) {
        let mut errors = vec![];
        let mut inserted = 0;
        for path in paths {
            match load_rgba(path) {
                Ok(img) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if let Some(d) = &mut self.doc {
                        d.add_image_layer(img, &name);
                        let step = d.size().0.max(d.size().1) as f64 * 0.03;
                        if let Some(l) = d.selected_layer_mut() {
                            l.translate(step * inserted as f64, step * inserted as f64);
                        }
                        inserted += 1;
                    }
                    self.last_dir = path.parent().map(|p| p.to_path_buf());
                }
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
        if inserted > 0 {
            self.select_tool(Tool::Move);
        }
        self.error = if errors.is_empty() { None } else { Some(errors.join("\n")) };
    }

    fn export(&mut self) {
        let Some(doc) = &self.doc else { return };
        let default = default_export_path(doc.source.as_deref(), &self.export_fmt, &doc.name);
        let name = default
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut dialog = rfd::FileDialog::new()
            .set_title(tr("Export"))
            .set_file_name(name);
        if let Some(dir) = default.parent().or(self.last_dir.as_deref()) {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else { return };
        let full = render::render(&doc.state, 1.0);
        match save_image(&path, &full, &self.export_fmt, self.export_quality) {
            Ok(p) => {
                self.error = None;
                self.info = Some(format!("{} → {}", tr("Exported"), p.display()));
                if let Some(d) = &mut self.doc {
                    d.modified = false;
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
}

/// 4 góc của hình chữ nhật `(x, y, w, h)`: trên-trái, trên-phải, dưới-phải, dưới-trái.
fn corners((x, y, w, h): (f64, f64, f64, f64)) -> [(f64, f64); 4] {
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
}

/// Hình chữ nhật mới khi kéo góc `corner` của `start` tới `pt`; góc đối diện đứng yên.
/// `keep_ratio` (Shift) giữ tỉ lệ ban đầu.
fn resize_from_corner(
    start: (f64, f64, f64, f64),
    corner: usize,
    pt: (f64, f64),
    keep_ratio: bool,
) -> (f64, f64, f64, f64) {
    let (ax, ay) = corners(start)[(corner + 2) % 4];
    let mut w = (pt.0 - ax).abs().max(1.0);
    let mut h = (pt.1 - ay).abs().max(1.0);
    if keep_ratio && start.2 > 0.0 && start.3 > 0.0 {
        let ratio = start.2 / start.3;
        if w / h > ratio {
            h = w / ratio;
        } else {
            w = h * ratio;
        }
    }
    let x = if pt.0 >= ax { ax } else { ax - w };
    let y = if pt.1 >= ay { ay } else { ay - h };
    (x, y, w, h)
}

/// Nút bấm trong chế độ chỉnh vùng tách nền.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MaskAction {
    Apply,
    Cancel,
    Undo,
    Reset,
}

/// Cỡ cọ lớn nhất (px ảnh).
fn brush_max(e: &MaskEdit) -> f64 {
    (e.w.max(e.h) as f64 / 2.0).max(50.0)
}

/// Điều khiển cọ: Thêm / Bớt / Đẩy viền / Co / Giãn, cỡ, độ cứng, chế độ xem, Áp dụng / Huỷ (`compact` = thanh tuỳ chọn:
/// bỏ Hoàn tác / Bỏ nét tô).
fn mask_controls(ui: &mut Ui, e: &mut MaskEdit, compact: bool) -> Option<MaskAction> {
    let mut action = None;
    for brush in MaskBrush::ALL {
        if ui
            .selectable_label(e.brush == brush, tr(brush.label()))
            .on_hover_text(tr(brush.tooltip()))
            .clicked()
        {
            e.brush = brush;
        }
    }
    ui.separator();
    let max = brush_max(e);
    ui.add(
        egui::Slider::new(&mut e.size, 2.0..=max)
            .logarithmic(true)
            .step_by(1.0)
            .suffix(" px")
            .text(tr("Brush size")),
    );
    let mut hardness = e.hardness * 100.0;
    if ui
        .add(
            egui::Slider::new(&mut hardness, 0.0..=100.0)
                .step_by(1.0)
                .suffix(" %")
                .text(tr("Hardness")),
        )
        .changed()
    {
        e.hardness = hardness / 100.0;
    }
    ui.separator();
    for (view, label) in [(MaskView::Overlay, "Selection"), (MaskView::Result, "Result")] {
        if ui.selectable_label(e.view == view, tr(label)).clicked() {
            e.view = view;
        }
    }
    ui.separator();
    if widgets::primary(ui, &tr("Apply"), true)
        .on_hover_text(tr("Remove the background (Enter)"))
        .clicked()
    {
        action = Some(MaskAction::Apply);
    }
    if !compact {
        if widgets::secondary(ui, "↶", e.can_undo())
            .on_hover_text(tr("Undo stroke (Ctrl+Z)"))
            .clicked()
        {
            action = Some(MaskAction::Undo);
        }
        if widgets::secondary(ui, &tr("Discard strokes"), e.is_modified()).clicked() {
            action = Some(MaskAction::Reset);
        }
    }
    if widgets::secondary(ui, &tr("Cancel"), true)
        .on_hover_text(tr("Esc"))
        .clicked()
    {
        action = Some(MaskAction::Cancel);
    }
    action
}

/// Ô màu bấm được (màu nền ảnh thẻ).
fn color_swatch(ui: &mut Ui, rgb: [u8; 3], selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(rect.shrink(2.0), 4.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
    let stroke = if selected {
        Stroke::new(2.0, theme::CYAN)
    } else if resp.hovered() {
        Stroke::new(1.0, theme::TEXT_DIM)
    } else {
        Stroke::new(1.0, theme::BORDER)
    };
    p.rect_stroke(rect.shrink(1.0), 5.0, stroke, egui::StrokeKind::Inside);
    resp
}

/// Ảnh thu nhỏ của một dòng trong bảng Layer.
enum Thumb {
    Image(egui::TextureHandle),
    Icon(ToolButton),
}

/// Texture thu nhỏ (cạnh dài 64 px) của ảnh layer / nền; chỉ tạo lại khi ảnh đổi (con trỏ `Arc`).
fn layer_thumb(
    thumbs: &mut HashMap<u64, (usize, egui::TextureHandle)>,
    ctx: &egui::Context,
    id: u64,
    img: &Arc<RgbaImage>,
) -> egui::TextureHandle {
    let key = Arc::as_ptr(img) as usize;
    if let Some((k, tex)) = thumbs.get(&id) {
        if *k == key {
            return tex.clone();
        }
    }
    let small = crate::preview::downscale_to(img, 64);
    let color = egui::ColorImage::from_rgba_unmultiplied([small.w.max(1), small.h.max(1)], &small.data);
    let tex = ctx.load_texture(format!("layer_thumb_{id}"), color, egui::TextureOptions::LINEAR);
    thumbs.insert(id, (key, tex.clone()));
    tex
}

/// Mục "Sắp xếp" (Edit menu + menu chuột phải trên canvas): đưa lên trên cùng / lên / xuống /
/// xuống dưới cùng cho layer hoặc nền đang chọn.
fn arrange_menu(ui: &mut Ui, d: &mut Document) {
    let has = d.selected.is_some();
    if menu_item(ui, &tr("Bring to front"), "Ctrl+Shift+]", has) {
        d.move_selected_to_end(true);
    }
    if menu_item(ui, &tr("Bring forward"), "Ctrl+]", has) {
        d.move_selected(1);
    }
    if menu_item(ui, &tr("Send backward"), "Ctrl+[", has) {
        d.move_selected(-1);
    }
    if menu_item(ui, &tr("Send to back"), "Ctrl+Shift+[", has) {
        d.move_selected_to_end(false);
    }
}

fn menu_item(ui: &mut Ui, label: &str, shortcut: &str, enabled: bool) -> bool {
    let mut btn = egui::Button::new(label);
    if !shortcut.is_empty() {
        btn = btn.shortcut_text(shortcut);
    }
    let clicked = ui.add_enabled(enabled, btn).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// Nút vuông của thanh công cụ dọc, icon vẽ bằng vector (không phụ thuộc font).
fn tool_button(ui: &mut Ui, button: ToolButton, selected: bool, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(36.0, 34.0), egui::Sense::click());
    let p = ui.painter();
    if selected {
        p.rect(
            rect,
            6.0,
            theme::ACCENT.linear_multiply(0.3),
            Stroke::new(1.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    } else if resp.hovered() && enabled {
        p.rect_filled(rect, 6.0, theme::HOVER);
    }
    let color = if !enabled {
        theme::TEXT_FAINT
    } else if selected {
        Color32::WHITE
    } else {
        theme::TEXT_DIM
    };
    paint_icon(p, rect.shrink(8.0), button, color);
    resp
}

/// Icon đơn giản kiểu Photoshop cho từng công cụ.
fn paint_icon(p: &egui::Painter, r: egui::Rect, button: ToolButton, color: Color32) {
    let at = |x: f32, y: f32| Pos2::new(r.min.x + x * r.width(), r.min.y + y * r.height());
    let s = Stroke::new(1.6, color);
    let line = |pts: &[(f32, f32)]| {
        p.add(egui::Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), s));
    };
    match button {
        ToolButton::Tool(Tool::Move) => {
            line(&[(0.5, 0.05), (0.5, 0.95)]);
            line(&[(0.05, 0.5), (0.95, 0.5)]);
            for (a, b, c) in [
                ((0.35, 0.2), (0.5, 0.05), (0.65, 0.2)),
                ((0.35, 0.8), (0.5, 0.95), (0.65, 0.8)),
                ((0.2, 0.35), (0.05, 0.5), (0.2, 0.65)),
                ((0.8, 0.35), (0.95, 0.5), (0.8, 0.65)),
            ] {
                line(&[a, b, c]);
            }
        }
        ToolButton::Tool(Tool::Crop) => {
            line(&[(0.28, 0.0), (0.28, 0.72), (1.0, 0.72)]);
            line(&[(0.0, 0.28), (0.72, 0.28), (0.72, 1.0)]);
        }
        ToolButton::Tool(Tool::Blur) => {
            // giọt nước
            let c = at(0.5, 0.62);
            let rad = 0.3 * r.width();
            let mut pts = vec![at(0.5, 0.02)];
            for i in 0..=20 {
                let a = (-30.0 + 240.0 * i as f32 / 20.0).to_radians();
                pts.push(Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin()));
            }
            p.add(egui::Shape::closed_line(pts, s));
        }
        ToolButton::Tool(Tool::Clone) => {
            // con dấu: tay cầm + thân + đế
            p.circle_stroke(at(0.5, 0.18), 0.14 * r.width(), s);
            line(&[(0.42, 0.3), (0.42, 0.55)]);
            line(&[(0.58, 0.3), (0.58, 0.55)]);
            p.rect_stroke(
                egui::Rect::from_min_max(at(0.12, 0.55), at(0.88, 0.78)),
                2.0,
                s,
                egui::StrokeKind::Middle,
            );
            line(&[(0.12, 0.92), (0.88, 0.92)]);
        }
        ToolButton::Tool(Tool::Hand) => {
            for x in [0.3, 0.45, 0.6, 0.75] {
                line(&[(x, 0.55), (x, 0.15)]);
            }
            line(&[(0.3, 0.6), (0.12, 0.45)]);
            line(&[(0.25, 0.55), (0.25, 0.75), (0.4, 0.95), (0.7, 0.95), (0.8, 0.75), (0.8, 0.5)]);
        }
        ToolButton::Tool(Tool::Zoom) => {
            p.circle_stroke(at(0.42, 0.42), 0.3 * r.width(), s);
            p.line_segment([at(0.64, 0.64), at(0.95, 0.95)], Stroke::new(3.0, color));
        }
        ToolButton::InsertImage => {
            p.rect_stroke(
                egui::Rect::from_min_max(at(0.0, 0.1), at(1.0, 0.9)),
                2.0,
                s,
                egui::StrokeKind::Middle,
            );
            line(&[(0.08, 0.82), (0.38, 0.45), (0.58, 0.68), (0.72, 0.55), (0.92, 0.82)]);
            p.circle_filled(at(0.72, 0.32), 0.08 * r.width(), color);
        }
        ToolButton::Collage => {
            p.rect_stroke(
                egui::Rect::from_min_max(at(0.0, 0.0), at(1.0, 1.0)),
                2.0,
                s,
                egui::StrokeKind::Middle,
            );
            line(&[(0.58, 0.0), (0.58, 1.0)]);
            line(&[(0.58, 0.5), (1.0, 0.5)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_keeps_the_opposite_corner_fixed() {
        let start = (10.0, 10.0, 100.0, 50.0);
        // kéo góc dưới-phải ra xa
        assert_eq!(resize_from_corner(start, 2, (210.0, 110.0), false), (10.0, 10.0, 200.0, 100.0));
        // kéo góc trên-trái vào trong: góc dưới-phải (110, 60) đứng yên
        assert_eq!(resize_from_corner(start, 0, (60.0, 40.0), false), (60.0, 40.0, 50.0, 20.0));
        // kéo qua góc đối diện → lật nhưng vẫn dương
        let (x, _, w, _) = resize_from_corner(start, 2, (0.0, 70.0), false);
        assert_eq!((x, w), (0.0, 10.0));
    }

    #[test]
    fn resize_with_shift_keeps_the_ratio() {
        let start = (0.0, 0.0, 100.0, 50.0);
        let (_, _, w, h) = resize_from_corner(start, 2, (300.0, 60.0), true);
        assert!((w / h - 2.0).abs() < 1e-9);
        assert_eq!(w, 300.0);
    }
}
