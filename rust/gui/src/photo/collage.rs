//! Cửa sổ "Cắt ghép layout" (collage) — port của `ui/photo/collage_dialog.py`.
//!
//! Hình học + ghép pixel nằm ở [`pbe_core::photo::collage`]; ở đây chỉ có tương tác:
//! chọn mẫu / cỡ / khoảng cách / bo góc / màu nền, bấm ô trống để chọn ảnh, kéo ảnh trong ô
//! để căn, lăn chuột để zoom trong ô. Xem trước dùng ảnh thu nhỏ (cạnh dài ≤ 1200 px), xuất
//! dùng ảnh gốc — bố cục giống nhau vì `cover_rect` chỉ phụ thuộc tỉ lệ ảnh.

use std::path::{Path, PathBuf};

use egui::{Color32, RichText, Ui};
use pbe_core::photo::collage::{
    cell_at, cell_rects, clamp_offset, render_collage, template, template_label, CellSource,
    CollageStyle, SIZE_PRESETS, TEMPLATES,
};
use pbe_core::photo::image_io::{load_rgba, OPEN_FILTER_EXTS};
use pbe_core::photo::RgbaImage;

use crate::fb::Canvas;
use crate::preview::downscale_to;
use crate::theme;
use crate::tr;
use crate::widgets;

const PREVIEW_IMG_LONG_SIDE: usize = 1200;
const MAX_CELL_ZOOM: f64 = 5.0;
const EMPTY_FILL: [u8; 4] = [0x1b, 0x26, 0x40, 255];

struct Item {
    full: RgbaImage,
    preview: RgbaImage,
    zoom: f64,
    offset: (f64, f64),
}

pub struct CollageView {
    pub open: bool,
    template: String,
    size: String,
    spacing: f64,
    radius: f64,
    background: [u8; 3],
    items: Vec<Option<Item>>,
    canvas: Canvas,
    cache: Option<RgbaImage>,
    cache_scale: f64,
    dirty: bool,
    drag: Option<(usize, egui::Pos2)>,
    pub last_dir: Option<PathBuf>,
    error: Option<String>,
}

impl Default for CollageView {
    fn default() -> Self {
        CollageView {
            open: false,
            template: "two_horizontal".to_string(),
            size: "1:1".to_string(),
            spacing: 20.0,
            radius: 0.0,
            background: [255, 255, 255],
            items: vec![None, None],
            canvas: Canvas::new("collage_canvas"),
            cache: None,
            cache_scale: 0.0,
            dirty: true,
            drag: None,
            last_dir: None,
            error: None,
        }
    }
}

impl CollageView {
    pub fn show(&mut self, last_dir: Option<PathBuf>) {
        self.open = true;
        if self.last_dir.is_none() {
            self.last_dir = last_dir;
        }
        self.dirty = true;
    }

    fn canvas_size(&self) -> (u32, u32) {
        SIZE_PRESETS
            .iter()
            .find(|(n, _)| *n == self.size)
            .map(|(_, s)| *s)
            .unwrap_or((2000, 2000))
    }

    fn style(&self) -> CollageStyle {
        let (width, height) = self.canvas_size();
        let [r, g, b] = self.background;
        CollageStyle {
            template: self.template.clone(),
            width,
            height,
            spacing: self.spacing,
            radius: self.radius,
            background: [r, g, b, 255],
        }
    }

    /// Đổi mẫu, giữ các ảnh đã chọn theo thứ tự.
    fn set_template(&mut self, name: &str) {
        let n = template(name).map(|c| c.len()).unwrap_or(0);
        let mut kept: Vec<Item> = self.items.drain(..).flatten().collect();
        kept.truncate(n);
        self.items = kept.into_iter().map(Some).collect();
        for item in self.items.iter_mut().flatten() {
            item.offset = (0.0, 0.0);
        }
        self.items.resize_with(n, || None);
        self.template = name.to_string();
        self.dirty = true;
    }

    fn load(&mut self, path: &Path) -> Option<Item> {
        match load_rgba(path) {
            Ok(full) => {
                self.last_dir = path.parent().map(|p| p.to_path_buf());
                let preview = downscale_to(&full, PREVIEW_IMG_LONG_SIDE);
                Some(Item {
                    full,
                    preview,
                    zoom: 1.0,
                    offset: (0.0, 0.0),
                })
            }
            Err(e) => {
                self.error = Some(e.to_string());
                None
            }
        }
    }

    fn dialog(&self, title: &str) -> rfd::FileDialog {
        let mut d = rfd::FileDialog::new().set_title(title).add_filter(
            tr("Photos"),
            &OPEN_FILTER_EXTS.map(|e| e.trim_start_matches('.')),
        );
        if let Some(dir) = &self.last_dir {
            d = d.set_directory(dir);
        }
        d
    }

    fn pick_into(&mut self, idx: usize) {
        let Some(path) = self.dialog(&tr("Choose photo")).pick_file() else { return };
        if let Some(item) = self.load(&path) {
            self.items[idx] = Some(item);
            self.dirty = true;
        }
    }

    /// "Chọn ảnh…": điền lần lượt vào các ô trống.
    fn pick_many(&mut self) {
        let Some(paths) = self.dialog(&tr("Choose photos")).pick_files() else { return };
        let mut paths = paths.into_iter();
        for idx in 0..self.items.len() {
            if self.items[idx].is_some() {
                continue;
            }
            let Some(path) = paths.next() else { break };
            if let Some(item) = self.load(&path) {
                self.items[idx] = Some(item);
            }
        }
        self.dirty = true;
    }

    /// Vẽ cửa sổ; trả về ảnh ghép đầy đủ khi người dùng bấm "Mở trong Sửa ảnh đơn".
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<RgbaImage> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut result = None;
        egui::Window::new(tr("Collage"))
            .open(&mut open)
            .collapsible(false)
            .default_size([1000.0, 660.0])
            .min_size([720.0, 480.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(240.0);
                        egui::ScrollArea::vertical()
                            .id_salt("collage_options")
                            .show(ui, |ui| self.ui_options(ui));
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        if let Some(e) = self.error.clone() {
                            widgets::error_banner(ui, &e);
                        }
                        let avail = ui.available_size() - egui::vec2(0.0, 46.0);
                        self.ui_canvas(ui, avail.max(egui::vec2(100.0, 100.0)));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let filled = self.items.iter().filter(|i| i.is_some()).count();
                            widgets::subtitle(
                                ui,
                                &format!("{filled} / {} {}", self.items.len(), tr("photos")),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if widgets::primary(ui, &tr("Open in photo editor"), filled > 0)
                                        .clicked()
                                    {
                                        result = Some(self.render_full());
                                    }
                                    if widgets::secondary(ui, &tr("Cancel"), true).clicked() {
                                        self.open = false;
                                    }
                                },
                            );
                        });
                    });
                });
            });
        if !open {
            self.open = false;
        }
        if result.is_some() {
            // Giải phóng ảnh gốc; lần sau mở lại từ đầu.
            self.open = false;
            self.items = self.items.iter().map(|_| None).collect();
            self.cache = None;
            self.dirty = true;
        }
        result
    }

    fn ui_options(&mut self, ui: &mut Ui) {
        widgets::caption(ui, &tr("Layout"));
        ui.horizontal_wrapped(|ui| {
            for (name, _) in TEMPLATES {
                let selected = self.template == name;
                if template_button(ui, name, selected)
                    .on_hover_text(tr(template_label(name)))
                    .clicked()
                    && !selected
                {
                    self.set_template(name);
                }
            }
        });
        ui.add_space(8.0);
        widgets::caption(ui, &tr("Size"));
        ui.horizontal_wrapped(|ui| {
            for (name, (w, h)) in SIZE_PRESETS {
                if ui
                    .selectable_label(self.size == name, name)
                    .on_hover_text(format!("{w} × {h} px"))
                    .clicked()
                {
                    self.size = name.to_string();
                    self.dirty = true;
                }
            }
        });
        ui.add_space(8.0);
        let spacing = pbe_core::settings::SliderSpec {
            key: "spacing",
            label: "Spacing",
            panel: "Collage",
            group: "Collage",
            minimum: 0.0,
            maximum: 200.0,
            step: 1.0,
            default: 20.0,
            decimals: 0,
        };
        if widgets::slider_row(ui, &spacing, &tr("Spacing"), &mut self.spacing) {
            self.dirty = true;
        }
        let radius = pbe_core::settings::SliderSpec {
            key: "radius",
            label: "Corner radius",
            panel: "Collage",
            group: "Collage",
            minimum: 0.0,
            maximum: 300.0,
            step: 1.0,
            default: 0.0,
            decimals: 0,
        };
        if widgets::slider_row(ui, &radius, &tr("Corner radius"), &mut self.radius) {
            self.dirty = true;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Background")).size(12.0).color(theme::TEXT_DIM));
            if ui.color_edit_button_srgb(&mut self.background).changed() {
                self.dirty = true;
            }
        });
        ui.add_space(10.0);
        if widgets::primary(ui, &tr("Choose photos…"), self.items.iter().any(|i| i.is_none()))
            .clicked()
        {
            self.pick_many();
        }
        if widgets::secondary(ui, &tr("Clear all"), self.items.iter().any(|i| i.is_some()))
            .clicked()
        {
            self.items = self.items.iter().map(|_| None).collect();
            self.dirty = true;
        }
        ui.add_space(8.0);
        widgets::subtitle(
            ui,
            &tr("Click an empty cell to add a photo, drag to reposition, scroll to zoom, double-click to replace."),
        );
    }

    fn ui_canvas(&mut self, ui: &mut Ui, avail: egui::Vec2) {
        let (rect, resp) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
        let (cw, ch) = self.canvas_size();
        let disp = (rect.width() as f64 / cw as f64).min(rect.height() as f64 / ch as f64) * 0.97;
        let size = egui::vec2((cw as f64 * disp) as f32, (ch as f64 * disp) as f32);
        let area = egui::Rect::from_center_size(rect.center(), size);
        let to_canvas = |p: egui::Pos2| {
            (
                (p.x - area.min.x) as f64 / disp,
                (p.y - area.min.y) as f64 / disp,
            )
        };
        let (tpl, spacing) = (self.template.clone(), self.spacing);
        let hit = move |p: egui::Pos2| {
            let (x, y) = to_canvas(p);
            cell_at(&tpl, cw, ch, spacing, x, y)
        };

        // --- Tương tác ---
        let hovered = resp.hover_pos().and_then(&hit);
        if resp.double_clicked() {
            if let Some(i) = resp.interact_pointer_pos().and_then(&hit) {
                self.pick_into(i);
            }
        } else if resp.clicked() {
            if let Some(i) = resp.interact_pointer_pos().and_then(&hit) {
                if self.items[i].is_none() {
                    self.pick_into(i);
                }
            }
        }
        if resp.drag_started() {
            if let Some(pos) = resp.interact_pointer_pos() {
                self.drag = hit(pos)
                    .filter(|&i| self.items[i].is_some())
                    .map(|i| (i, pos));
            }
        }
        let cells = cell_rects(&self.template, cw, ch, self.spacing);
        if resp.dragged() {
            if let (Some((i, last)), Some(pos)) = (self.drag, resp.interact_pointer_pos()) {
                if let (Some(item), Some(cell)) = (self.items[i].as_mut(), cells.get(i)) {
                    let off = (
                        item.offset.0 + (pos.x - last.x) as f64 / disp,
                        item.offset.1 + (pos.y - last.y) as f64 / disp,
                    );
                    item.offset =
                        clamp_offset(item.full.w as u32, item.full.h as u32, cell, item.zoom, off);
                    self.dirty = true;
                }
                self.drag = Some((i, pos));
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        let scroll = ui.input(|i| i.smooth_scroll_delta.y) as f64;
        if let Some(i) = hovered {
            if scroll.abs() > 0.5 {
                if let (Some(item), Some(cell)) = (self.items[i].as_mut(), cells.get(i)) {
                    item.zoom = (item.zoom * (1.0 + scroll / 400.0)).clamp(1.0, MAX_CELL_ZOOM);
                    item.offset = clamp_offset(
                        item.full.w as u32,
                        item.full.h as u32,
                        cell,
                        item.zoom,
                        item.offset,
                    );
                    self.dirty = true;
                }
            }
            let cursor = if self.items[i].is_some() {
                egui::CursorIcon::Grab
            } else {
                egui::CursorIcon::PointingHand
            };
            ui.ctx().set_cursor_icon(cursor);
        }

        // --- Ghép xem trước (chỉ khi có thay đổi) ---
        let ppp = ui.ctx().pixels_per_point() as f64;
        let scale = (disp * ppp).min(1.0);
        if self.dirty || self.cache.is_none() || (scale - self.cache_scale).abs() > scale * 0.02 {
            let style = self.style();
            let sources: Vec<Option<CellSource>> = self
                .items
                .iter()
                .map(|i| {
                    i.as_ref().map(|i| CellSource {
                        img: &i.preview,
                        zoom: i.zoom,
                        offset: i.offset,
                    })
                })
                .collect();
            self.cache = Some(render_collage(&style, &sources, scale, Some(EMPTY_FILL)));
            self.cache_scale = scale;
            self.dirty = false;
        }

        let (pw, ph) = (
            ((rect.width() as f64 * ppp) as usize).max(1),
            ((rect.height() as f64 * ppp) as usize).max(1),
        );
        let fb = self.canvas.begin(pw, ph);
        fb.clear([0x0b, 0x10, 0x20, 255]);
        if let Some(img) = &self.cache {
            let (dx, dy) = ((area.min.x - rect.min.x) * ppp as f32, (area.min.y - rect.min.y) * ppp as f32);
            fb.draw_image(img, dx, dy, area.width() * ppp as f32, area.height() * ppp as f32, 1.0);
        }
        self.canvas.paint(ui, rect);

        // Nhãn "+" trên ô trống, viền ô đang trỏ.
        let painter = ui.painter_at(rect);
        for (i, cell) in cells.iter().enumerate() {
            let r = egui::Rect::from_min_size(
                area.min + egui::vec2((cell.x * disp) as f32, (cell.y * disp) as f32),
                egui::vec2((cell.w * disp) as f32, (cell.h * disp) as f32),
            );
            if self.items[i].is_none() {
                painter.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("+ {}", tr("Add photo")),
                    egui::FontId::proportional(13.0),
                    theme::TEXT_DIM,
                );
            }
            if hovered == Some(i) {
                painter.rect_stroke(
                    r,
                    0.0,
                    egui::Stroke::new(1.5, theme::ACCENT),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    fn render_full(&self) -> RgbaImage {
        let sources: Vec<Option<CellSource>> = self
            .items
            .iter()
            .map(|i| {
                i.as_ref().map(|i| CellSource {
                    img: &i.full,
                    zoom: i.zoom,
                    offset: i.offset,
                })
            })
            .collect();
        render_collage(&self.style(), &sources, 1.0, None)
    }
}

/// Nút chọn mẫu: vẽ thu nhỏ các ô của mẫu.
fn template_button(ui: &mut Ui, name: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(50.0, 40.0), egui::Sense::click());
    let p = ui.painter();
    let (fill, stroke) = if selected {
        (theme::ACCENT.linear_multiply(0.25), theme::ACCENT)
    } else if resp.hovered() {
        (theme::HOVER, theme::BORDER_STRONG)
    } else {
        (theme::CARD, theme::BORDER)
    };
    p.rect(rect, 6.0, fill, egui::Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    let inner = rect.shrink(6.0);
    for &(fx, fy, fw, fh) in template(name).unwrap_or(&[]) {
        let r = egui::Rect::from_min_size(
            inner.min + egui::vec2(fx as f32 * inner.width(), fy as f32 * inner.height()),
            egui::vec2(fw as f32 * inner.width(), fh as f32 * inner.height()),
        )
        .shrink(1.0);
        let c = if selected { theme::TEXT } else { theme::TEXT_DIM };
        p.rect_filled(r, 1.5, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 150));
    }
    resp
}
