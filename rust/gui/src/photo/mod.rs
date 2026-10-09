//! Trình sửa một ảnh — canvas kiểu Photoshop (zoom/pan, công cụ, layer, xuất file).
//! Port của `ui/photo/photo_editor_view.py`, `ui/photo/canvas.py`, `ui/photo/tools.py`,
//! `ui/photo/panels.py`.

pub mod doc;
pub mod render;

use std::path::{Path, PathBuf};

use egui::{Key, RichText, Ui};
use pbe_core::photo::effects::{PhotoAdjust, ADJUST_SLIDERS, BLUR_MODES};
use pbe_core::photo::geometry::{
    clamp_zoom, crop_rect, crop_ratio, fit_scale, next_zoom, round_rect, zoom_at, CROP_RATIOS,
    FIT_MARGIN,
};
use pbe_core::photo::image_io::{default_export_path, load_rgba, save_image, OPEN_FILTER_EXTS};
use pbe_core::photo::RgbaImage;

use crate::fb::Canvas;
use crate::preview::PREVIEW_LONG_SIDE;
use crate::theme;
use crate::tr;
use crate::widgets;
use doc::{Document, Layer};

#[derive(PartialEq, Clone, Copy)]
enum Tool {
    Move,
    Crop,
    Blur,
}

enum Drag {
    Pan { last: egui::Pos2 },
    MoveLayer { id: u64, last: (f64, f64) },
    Rect { anchor: (f64, f64) },
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
                let long = img.long_side().max(1);
                self.preview_scale = (PREVIEW_LONG_SIDE as f64 / long as f64).min(1.0);
                self.doc = Some(Document::new(img, Some(path.to_path_buf())));
                self.composite = None;
                self.composite_rev = 0;
                self.fit_requested = true;
                self.error = None;
                self.info = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
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

    // --- Vẽ -------------------------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui) {
        self.ui_toolbar(ui);
        if self.doc.is_some() {
            self.ui_right_panel(ui);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::WINDOW))
            .show(ui, |ui| {
                if let Some(doc) = &self.doc {
                    let mark = if doc.modified { " •" } else { "" };
                    widgets::subtitle(ui, &format!("{}{mark}", doc.name));
                }
                if let Some(e) = self.error.clone() {
                    widgets::error_banner(ui, &e);
                }
                if let Some(i) = self.info.clone() {
                    widgets::info_banner(ui, &i, theme::OK);
                }
                if self.doc.is_none() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 2.0 - 40.0);
                        widgets::subtitle(ui, &tr("Open a photo to start editing"));
                        ui.add_space(8.0);
                        if widgets::primary(ui, &tr("Open photo"), true).clicked() {
                            self.choose_and_open();
                        }
                    });
                    return;
                }
                self.ui_canvas(ui);
            });
        self.handle_shortcuts(ui);
    }

    fn ui_toolbar(&mut self, ui: &mut Ui) {
        egui::Panel::top(egui::Id::new("photo_toolbar"))
            .frame(theme::bar())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if widgets::secondary(ui, &tr("Open photo"), true).clicked() {
                        self.choose_and_open();
                    }
                    let has = self.doc.is_some();
                    if widgets::primary(ui, &tr("Export"), has).clicked() {
                        self.export();
                    }
                    ui.separator();
                    for (tool, label) in [
                        (Tool::Move, tr("Move")),
                        (Tool::Crop, tr("Crop")),
                        (Tool::Blur, tr("Blur")),
                    ] {
                        if ui
                            .selectable_label(self.tool == tool && has, label)
                            .clicked()
                            && has
                        {
                            self.tool = tool;
                            self.crop_pending = None;
                            self.drag = None;
                        }
                    }
                    ui.separator();
                    let (can_undo, can_redo) = match &self.doc {
                        Some(d) => (d.history.can_undo(), d.history.can_redo()),
                        None => (false, false),
                    };
                    if widgets::secondary(ui, "↶", can_undo)
                        .on_hover_text(tr("Undo"))
                        .clicked()
                    {
                        self.undo();
                    }
                    if widgets::secondary(ui, "↷", can_redo)
                        .on_hover_text(tr("Redo"))
                        .clicked()
                    {
                        self.redo();
                    }
                    ui.separator();
                    if widgets::secondary(ui, "⟲ 90°", has).clicked() {
                        if let Some(d) = &mut self.doc {
                            d.rotate90(false);
                        }
                    }
                    if widgets::secondary(ui, "⟳ 90°", has).clicked() {
                        if let Some(d) = &mut self.doc {
                            d.rotate90(true);
                        }
                    }
                    if widgets::secondary(ui, &tr("Flip horizontal"), has).clicked() {
                        if let Some(d) = &mut self.doc {
                            d.flip(true);
                        }
                    }
                    if widgets::secondary(ui, &tr("Flip vertical"), has).clicked() {
                        if let Some(d) = &mut self.doc {
                            d.flip(false);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{}%", (self.scale * 100.0).round()))
                                .size(11.5)
                                .color(theme::CYAN),
                        );
                        if widgets::secondary(ui, "+", has).clicked() {
                            self.zoom_step(1);
                        }
                        if widgets::secondary(ui, "−", has).clicked() {
                            self.zoom_step(-1);
                        }
                        if widgets::secondary(ui, "100%", has).clicked() {
                            self.set_zoom(1.0);
                        }
                        if widgets::secondary(ui, &tr("Fit"), has).clicked() {
                            self.fit_requested = true;
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
                    self.ui_adjust_panel(ui);
                    ui.add_space(6.0);
                    self.ui_tool_options(ui);
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
            let straighten = pbe_core::settings::SliderSpec {
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
            let angle_changed = widgets::slider_row(ui, &straighten, &tr("Straighten"), &mut angle);
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

    fn ui_tool_options(&mut self, ui: &mut Ui) {
        match self.tool {
            Tool::Crop => {
                widgets::section(ui, &tr("Crop"), true, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (label, _) in CROP_RATIOS {
                            if ui
                                .selectable_label(self.crop_ratio == label, tr(label))
                                .clicked()
                            {
                                self.crop_ratio = label.to_string();
                            }
                        }
                    });
                    let pending = self.crop_pending;
                    ui.horizontal(|ui| {
                        if widgets::primary(ui, &tr("Apply"), pending.is_some()).clicked() {
                            self.apply_crop();
                        }
                        if widgets::secondary(ui, &tr("Cancel"), pending.is_some()).clicked() {
                            self.crop_pending = None;
                        }
                    });
                    if let Some(r) = pending {
                        widgets::subtitle(
                            ui,
                            &format!("{} × {} px", r.2.round() as i64, r.3.round() as i64),
                        );
                    } else {
                        widgets::subtitle(ui, &tr("Drag on the photo to choose the crop"));
                    }
                });
            }
            Tool::Blur => {
                widgets::section(ui, &tr("Blur"), true, |ui| {
                    ui.horizontal(|ui| {
                        for mode in BLUR_MODES {
                            if ui
                                .selectable_label(self.blur_mode == mode, tr(mode))
                                .clicked()
                            {
                                self.blur_mode = mode.to_string();
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        for shape in ["rect", "ellipse"] {
                            if ui
                                .selectable_label(self.blur_shape == shape, tr(shape))
                                .clicked()
                            {
                                self.blur_shape = shape.to_string();
                            }
                        }
                    });
                    let spec = pbe_core::settings::SliderSpec {
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
                    widgets::slider_row(ui, &spec, &tr("Strength"), &mut self.blur_strength);
                    widgets::subtitle(ui, &tr("Drag on the photo to blur an area"));
                });
            }
            Tool::Move => {
                widgets::section(ui, &tr("Layer"), true, |ui| {
                    if widgets::secondary(ui, &tr("Insert image"), true).clicked() {
                        self.insert_image();
                    }
                    let Some(doc) = &mut self.doc else { return };
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
                        let spec = pbe_core::settings::SliderSpec {
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
                        if widgets::slider_row(ui, &spec, &tr("Strength"), &mut strength) {
                            doc.push_undo("Blur strength", Some("blur_strength"));
                            if let Some(Layer::Blur(l)) = doc.selected_layer_mut() {
                                l.strength = strength;
                            }
                        }
                        ui.horizontal(|ui| {
                            for mode in BLUR_MODES {
                                if ui.selectable_label(b.mode == mode, tr(mode)).clicked()
                                    && b.mode != mode
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
        }
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
            if doc.state.layers.is_empty() {
                widgets::subtitle(ui, &tr("Only the photo (no layers yet)"));
            }
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

    fn to_image(&self, rect: egui::Rect, pos: egui::Pos2) -> (f64, f64) {
        (
            ((pos.x - rect.min.x) as f64 - self.offset.0) / self.scale,
            ((pos.y - rect.min.y) as f64 - self.offset.1) / self.scale,
        )
    }

    fn handle_canvas_input(&mut self, resp: &egui::Response, rect: egui::Rect) {
        let Some(pos) = resp.interact_pointer_pos() else {
            if !resp.dragged() {
                self.drag = None;
            }
            return;
        };
        let img_pt = self.to_image(rect, pos);
        let space_pan = resp.ctx.input(|i| i.modifiers.alt || i.pointer.middle_down());

        if resp.drag_started() {
            self.drag = if space_pan {
                Some(Drag::Pan { last: pos })
            } else {
                match self.tool {
                    Tool::Crop | Tool::Blur => Some(Drag::Rect { anchor: img_pt }),
                    Tool::Move => {
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
        let selected_bounds = self
            .doc
            .as_ref()
            .and_then(|d| d.selected_layer().map(|l| l.bounds()));

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
        // Khung chọn của layer.
        if let Some((bx, by, bw, bh)) = selected_bounds {
            fb.stroke_rect(
                (dx + bx * scale * ppp) as f32,
                (dy + by * scale * ppp) as f32,
                (bw * scale * ppp) as f32,
                (bh * scale * ppp) as f32,
                2.0,
                [0x4f, 0x8c, 0xff, 255],
            );
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
        }
        self.canvas.paint(ui, rect);
    }

    // --- Lệnh -----------------------------------------------------------------------------

    fn handle_shortcuts(&mut self, ui: &mut Ui) {
        if self.doc.is_none() {
            return;
        }
        let (undo, redo, fit, hundred, zoom_in, zoom_out, delete) = ui.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && !i.modifiers.shift && i.key_pressed(Key::Z),
                cmd && i.modifiers.shift && i.key_pressed(Key::Z),
                cmd && i.key_pressed(Key::Num0),
                cmd && i.key_pressed(Key::Num1),
                cmd && (i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals)),
                cmd && i.key_pressed(Key::Minus),
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
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
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn export(&mut self) {
        let Some(doc) = &self.doc else { return };
        let default = default_export_path(doc.source.as_deref(), &self.export_fmt, "image");
        let name = default
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut dialog = rfd::FileDialog::new()
            .set_title(tr("Export"))
            .set_file_name(name);
        if let Some(dir) = default.parent() {
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
