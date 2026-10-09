//! Widget dùng lại — port của `ui/widgets.py` + các phần style lặp trong `ui/*.py`.

use egui::{Color32, CornerRadius, Response, RichText, Stroke, Ui, vec2};
use pbe_core::settings::SliderSpec;

use crate::theme;

pub fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(19.0).strong().color(theme::TEXT));
}

pub fn subtitle(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(theme::TEXT_DIM));
}

pub fn caption(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(10.5)
            .strong()
            .color(theme::TEXT_FAINT),
    );
}

pub fn group_label(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(text).size(11.0).strong().color(theme::TEXT_DIM));
}

/// Nút nhấn chính (màu nhấn).
pub fn primary(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let btn = egui::Button::new(RichText::new(text).strong().color(Color32::WHITE))
        .fill(theme::ACCENT)
        .corner_radius(CornerRadius::same(9))
        .min_size(vec2(0.0, 32.0));
    ui.add_enabled(enabled, btn)
}

/// Nút nhấn phụ (nền card).
pub fn secondary(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let btn = egui::Button::new(RichText::new(text).color(theme::TEXT))
        .fill(theme::CARD)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(9))
        .min_size(vec2(0.0, 30.0));
    ui.add_enabled(enabled, btn)
}

/// Nút nguy hiểm (xoá).
pub fn danger(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let btn = egui::Button::new(RichText::new(text).color(theme::ERROR))
        .fill(theme::CARD)
        .stroke(Stroke::new(1.0, theme::ERROR.linear_multiply(0.5)))
        .corner_radius(CornerRadius::same(9))
        .min_size(vec2(0.0, 30.0));
    ui.add_enabled(enabled, btn)
}

/// Một thanh trượt của bảng chỉnh sửa: nhãn + giá trị + trượt; nháy đúp nhãn = về mặc định.
///
/// Trả `true` khi giá trị đổi trong khung này.
pub fn slider_row(ui: &mut Ui, spec: &SliderSpec, label: &str, value: &mut f64) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let name = ui.add(
            egui::Label::new(RichText::new(label).size(12.0).color(theme::TEXT_DIM))
                .sense(egui::Sense::click()),
        );
        if name.double_clicked() {
            *value = spec.default;
            changed = true;
        }
        name.on_hover_text(crate::tr("Double-click the name to reset"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let txt = format!("{:.*}", spec.decimals, *value);
            let color = if *value == spec.default {
                theme::TEXT_FAINT
            } else {
                theme::CYAN
            };
            ui.label(RichText::new(txt).size(12.0).monospace().color(color));
        });
    });
    let slider = egui::Slider::new(value, spec.minimum..=spec.maximum)
        .step_by(spec.step)
        .show_value(false);
    if ui.add(slider).changed() {
        changed = true;
    }
    ui.add_space(2.0);
    changed
}

/// Tiêu đề nhóm gập được (Basic / Detail / Effects…).
pub fn section<R>(ui: &mut Ui, title: &str, open: bool, body: impl FnOnce(&mut Ui) -> R) {
    egui::CollapsingHeader::new(RichText::new(title).size(13.0).strong().color(theme::TEXT))
        .id_salt(title)
        .default_open(open)
        .show(ui, body);
}

/// Dải thông báo lỗi / cảnh báo đỏ.
pub fn error_banner(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .fill(theme::ERROR.linear_multiply(0.12))
        .stroke(Stroke::new(1.0, theme::ERROR.linear_multiply(0.5)))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(theme::ERROR));
        });
}

/// Dải thông báo trung tính.
pub fn info_banner(ui: &mut Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.linear_multiply(0.12))
        .stroke(Stroke::new(1.0, color.linear_multiply(0.45)))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(color));
        });
}

/// Thanh tiến độ 0–1 với nhãn ở giữa.
pub fn progress_bar(ui: &mut Ui, fraction: f32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(11), theme::CARD);
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.0 {
        let mut filled = rect;
        filled.set_width(rect.width() * f);
        p.rect_filled(filled, CornerRadius::same(11), theme::ACCENT);
    }
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.0),
        theme::TEXT,
    );
}

/// Ô chọn trong danh sách (file, preset, clip…). Trả `true` khi được nhấn.
pub fn list_row(ui: &mut Ui, text: &str, sub: Option<&str>, selected: bool) -> Response {
    let fill = if selected { theme::HOVER } else { Color32::TRANSPARENT };
    let stroke = if selected {
        Stroke::new(1.0, theme::ACCENT)
    } else {
        Stroke::NONE
    };
    let resp = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(9, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                ui.label(RichText::new(text).size(12.5).color(theme::TEXT));
                if let Some(s) = sub {
                    ui.label(RichText::new(s).size(10.5).color(theme::TEXT_FAINT));
                }
            });
        })
        .response;
    resp.interact(egui::Sense::click())
}
