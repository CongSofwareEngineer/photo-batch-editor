//! Trình sửa một ảnh — bố cục kiểu Photoshop: thanh menu + thanh tuỳ chọn công cụ ở trên,
//! thanh công cụ dọc bên trái, canvas ở giữa, panel (Properties / Adjust / Sharpen / Layers)
//! bên phải, thanh trạng thái ở dưới. Port của `ui/photo/photo_editor_view.py`,
//! `ui/photo/canvas.py`, `ui/photo/tools.py`, `ui/photo/panels.py`.

pub mod collage;
pub mod doc;
pub mod render;

use std::path::{Path, PathBuf};

use egui::{Color32, Key, Pos2, RichText, Stroke, Ui};
use pbe_core::photo::effects::{
    PhotoAdjust, SharpenSettings, ADJUST_SLIDERS, BLUR_MODES, SHARPEN_SLIDERS,
};
use pbe_core::photo::geometry::{
    clamp_zoom, crop_rect, crop_ratio, fit_scale, next_zoom, round_rect, zoom_at, CROP_RATIOS,
    FIT_MARGIN,
};
use pbe_core::photo::image_io::{default_export_path, load_rgba, save_image, OPEN_FILTER_EXTS};
use pbe_core::photo::RgbaImage;
use pbe_core::settings::SliderSpec;

use crate::fb::Canvas;
use crate::preview::PREVIEW_LONG_SIDE;
use crate::theme;
use crate::tr;
use crate::widgets;
use doc::{Document, Layer};

#[derive(PartialEq, Clone, Copy, Debug)]
enum Tool {
    Move,
    Crop,
    Blur,
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
const TOOLBAR: [Option<(ToolButton, &str, Key)>; 9] = [
    Some((ToolButton::Tool(Tool::Move), "Move tool", Key::V)),
    Some((ToolButton::Tool(Tool::Crop), "Crop tool", Key::C)),
    None,
    Some((ToolButton::Tool(Tool::Blur), "Blur tool", Key::B)),
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
        self.composite = None;
        self.composite_rev = 0;
        self.fit_requested = true;
        self.crop_pending = None;
        self.drag = None;
        self.tool = Tool::Move;
        self.error = None;
        self.info = None;
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
        self.handle_shortcuts(ui);
    }

    /// Thanh menu kiểu Photoshop: File / Edit / Image / View + Xuất, Hoàn tác ở bên phải.
    fn ui_menu_bar(&mut self, ui: &mut Ui) {
        let has = self.doc.is_some();
        let (can_undo, can_redo, has_layer) = match &self.doc {
            Some(d) => (d.history.can_undo(), d.history.can_redo(), d.selected.is_some()),
            None => (false, false, false),
        };
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
                    });
                    ui.menu_button(tr("Image"), |ui| {
                        if menu_item(ui, &tr("Rotate 90° left"), "", has) {
                            if let Some(d) = &mut self.doc {
                                d.rotate90(false);
                            }
                        }
                        if menu_item(ui, &tr("Rotate 90° right"), "", has) {
                            if let Some(d) = &mut self.doc {
                                d.rotate90(true);
                            }
                        }
                        if menu_item(ui, &tr("Flip horizontal"), "", has) {
                            if let Some(d) = &mut self.doc {
                                d.flip(true);
                            }
                        }
                        if menu_item(ui, &tr("Flip vertical"), "", has) {
                            if let Some(d) = &mut self.doc {
                                d.flip(false);
                            }
                        }
                        ui.separator();
                        if menu_item(ui, &tr("Crop"), "C", has) {
                            self.select_tool(Tool::Crop);
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
                    match self.tool {
                        Tool::Move => self.ui_move_options(ui, has),
                        Tool::Crop => self.ui_crop_options(ui),
                        Tool::Blur => self.ui_blur_options(ui),
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
                    let enabled = has || button == ToolButton::Collage;
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
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(text(format!("{} {}", doc.state.layers.len(), tr("layers"))).color(theme::TEXT_FAINT));
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
                    self.ui_adjust_panel(ui);
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
    }

    fn ui_properties_panel(&mut self, ui: &mut Ui) {
        let Some(doc) = &mut self.doc else { return };
        widgets::section(ui, &tr("Properties"), true, |ui| {
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
        widgets::section(ui, &tr("Layers"), true, |ui| {
            let mut toggle: Option<u64> = None;
            let mut select: Option<u64> = None;
            for layer in doc.state.layers.iter().rev() {
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(layer.visible(), if layer.visible() { "👁" } else { "–" })
                        .clicked()
                    {
                        toggle = Some(layer.id());
                    }
                    let selected = doc.selected == Some(layer.id());
                    let name = if layer.name().is_empty() {
                        tr(layer.kind())
                    } else {
                        layer.name().to_string()
                    };
                    if ui.selectable_label(selected, name).clicked() {
                        select = Some(layer.id());
                    }
                });
            }
            // "Nền" luôn ở dưới cùng, chỉ ẩn / hiện được.
            ui.horizontal(|ui| {
                let visible = doc.state.background.visible;
                if ui
                    .selectable_label(visible, if visible { "👁" } else { "–" })
                    .clicked()
                {
                    doc.push_undo("Show / hide layer", None);
                    doc.state.background.visible = !visible;
                }
                ui.label(RichText::new(tr("Background")).color(theme::TEXT_DIM));
            });
            if let Some(id) = toggle {
                doc.push_undo("Show / hide layer", None);
                if let Some(l) = doc.state.layers.iter_mut().find(|l| l.id() == id) {
                    let v = l.visible();
                    l.set_visible(!v);
                }
            }
            if let Some(id) = select {
                doc.selected = Some(id);
            }
            let has = doc.selected.is_some();
            ui.horizontal(|ui| {
                if widgets::secondary(ui, "▲", has).clicked() {
                    doc.move_selected(1);
                }
                if widgets::secondary(ui, "▼", has).clicked() {
                    doc.move_selected(-1);
                }
                if widgets::secondary(ui, &tr("Duplicate layer"), has).clicked() {
                    doc.duplicate_selected();
                }
                if widgets::danger(ui, &tr("Delete layer"), has).clicked() {
                    doc.delete_selected();
                }
            });
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
    }

    fn rebuild_composite(&mut self) {
        let Some(doc) = &self.doc else { return };
        if self.composite.is_some() && self.composite_rev == doc.revision {
            return;
        }
        self.composite = Some(render::render(&doc.state, self.preview_scale));
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
            let icon = if pan_override || self.tool == Tool::Hand {
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

        let Some(pos) = resp.interact_pointer_pos() else {
            if !resp.dragged() {
                self.drag = None;
            }
            return;
        };
        let img_pt = self.to_image(rect, pos);

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
                    Tool::Hand | Tool::Zoom => Some(Drag::Pan { last: pos }),
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

    fn paint_canvas(&mut self, ui: &mut Ui, rect: egui::Rect, iw: usize, ih: usize) {
        let ppp = ui.ctx().pixels_per_point() as f64;
        let (pw, ph) = (
            (rect.width() as f64 * ppp) as usize,
            (rect.height() as f64 * ppp) as usize,
        );
        let composite = self.composite.clone();
        let (scale, offset) = (self.scale, self.offset);
        let pending = self.crop_pending;
        let selected_bounds = if self.tool == Tool::Move {
            self.doc
                .as_ref()
                .and_then(|d| d.selected_layer().map(|l| l.bounds()))
        } else {
            None
        };

        let fb = self.canvas.begin(pw.max(1), ph.max(1));
        fb.clear([0x0b, 0x10, 0x20, 255]);
        // Ô vuông trong suốt chỉ ở vùng của ảnh.
        let (dx, dy) = (offset.0 * ppp, offset.1 * ppp);
        let (dw, dh) = (iw as f64 * scale * ppp, ih as f64 * scale * ppp);
        fb.fill_rect(dx as f32, dy as f32, dw as f32, dh as f32, [0x16, 0x1e, 0x33, 255]);
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
            for (cx, cy) in corners((sx, sy, sw, sh)) {
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
        // Phím chữ cái chọn công cụ (khi không gõ trong ô nhập).
        if !typing && !cmd {
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
                    if d.selected.is_some() {
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
        let Some(path) = dialog.pick_file() else { return };
        match load_rgba(&path) {
            Ok(img) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if let Some(d) = &mut self.doc {
                    d.add_image_layer(img, &name);
                }
                self.select_tool(Tool::Move);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
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
