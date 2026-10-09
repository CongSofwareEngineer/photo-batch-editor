//! Thanh điều hướng bên trái: logo, các trang, người đang đăng nhập. Port của `ui/sidebar.py`.

use egui::{Color32, CornerRadius, RichText, Stroke, Ui, vec2};

use crate::theme;
use crate::tr;
use crate::widgets;

/// `(khoá, nhãn, mô tả)` — thứ tự hiện trên thanh điều hướng.
pub const PAGES: [(&str, &str, &str); 4] = [
    ("editor", "Batch edit", "Edit a folder of photos (Ctrl+1)"),
    (
        "photo",
        "Photo editor",
        "Edit one photo: layers, text, crop, blur, collage (Ctrl+2)",
    ),
    (
        "video",
        "Video editor",
        "Cut a video, mute it, add text and music (Ctrl+3)",
    ),
    (
        "settings",
        "Settings",
        "Saved filter settings, account and language (Ctrl+4)",
    ),
];

pub struct SidebarResult {
    pub page: Option<String>,
    pub sign_out: bool,
}

pub fn ui(ui: &mut Ui, current: &str, user: &str, status: &str) -> SidebarResult {
    let mut out = SidebarResult {
        page: None,
        sign_out: false,
    };
    // Logo
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(38.0, 38.0), egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(10), theme::ACCENT);
        p.rect_filled(
            egui::Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
            CornerRadius::same(4),
            theme::ACCENT_2,
        );
        ui.vertical(|ui| {
            ui.label(RichText::new("Photo Batch").size(15.0).strong().color(theme::TEXT));
            ui.label(
                RichText::new("EDITOR  ·  RUST")
                    .size(9.5)
                    .color(theme::TEXT_FAINT),
            );
        });
    });
    ui.add_space(20.0);
    widgets::caption(ui, &tr("WORKSPACE"));
    ui.add_space(4.0);
    for (key, label, tip) in PAGES {
        if nav_button(ui, &tr(label), current == key).on_hover_text(tr(tip)).clicked() {
            out.page = Some(key.to_string());
        }
    }
    ui.add_space(ui.available_height() - 86.0);
    if !status.is_empty() {
        ui.label(RichText::new(status).size(11.0).color(theme::CYAN));
        ui.add_space(4.0);
    }
    // Thẻ người dùng
    egui::Frame::new()
        .fill(theme::CARD)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(32.0, 32.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, CornerRadius::same(16), theme::ACCENT_2);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    user.chars().next().unwrap_or('?').to_uppercase().to_string(),
                    egui::FontId::proportional(14.0),
                    Color32::WHITE,
                );
                ui.vertical(|ui| {
                    ui.label(RichText::new(user).size(12.5).strong().color(theme::TEXT));
                    ui.label(RichText::new(tr("Signed in")).size(10.0).color(theme::TEXT_FAINT));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new("⏻").frame(false))
                        .on_hover_text(tr("Sign out"))
                        .clicked()
                    {
                        out.sign_out = true;
                    }
                });
            });
        });
    out
}

fn nav_button(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let fill = if selected { theme::HOVER } else { Color32::TRANSPARENT };
    let stroke = if selected {
        Stroke::new(1.0, theme::ACCENT)
    } else {
        Stroke::NONE
    };
    let text = RichText::new(label)
        .size(13.0)
        .color(if selected { theme::TEXT } else { theme::TEXT_DIM });
    let btn = egui::Button::new(text)
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(9))
        .min_size(vec2(ui.available_width(), 34.0));
    ui.add(btn)
}
