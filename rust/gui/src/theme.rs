//! Chủ đề "midnight" — port của `ui/theme.qss` sang `egui::Visuals`.
//!
//! Bảng màu giữ y nguyên bản Qt để hai bản (Python / Rust) trông giống nhau:
//! window `#0b1020` · panel `#10172a` · card/input `#151e33` · hover `#1b2640`
//! viền `#1f2b45` / `#2a3a5c` · chữ `#e7ecf6` / `#8d9bb8` / `#5d6b88`
//! nhấn `#4f8cff` → `#8b5cf6` · cyan `#22d3ee` · ok `#34d399` · cảnh báo `#fbbf24` · lỗi `#f87171`

use egui::{Color32, CornerRadius, Margin, Stroke, Visuals};

pub const WINDOW: Color32 = Color32::from_rgb(0x0b, 0x10, 0x20);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x0d, 0x13, 0x26);
pub const PANEL: Color32 = Color32::from_rgb(0x10, 0x17, 0x2a);
pub const CARD: Color32 = Color32::from_rgb(0x15, 0x1e, 0x33);
pub const HOVER: Color32 = Color32::from_rgb(0x1b, 0x26, 0x40);
pub const BORDER: Color32 = Color32::from_rgb(0x1f, 0x2b, 0x45);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x2a, 0x3a, 0x5c);
pub const TEXT: Color32 = Color32::from_rgb(0xe7, 0xec, 0xf6);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8d, 0x9b, 0xb8);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x5d, 0x6b, 0x88);
pub const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x8c, 0xff);
pub const ACCENT_2: Color32 = Color32::from_rgb(0x8b, 0x5c, 0xf6);
pub const CYAN: Color32 = Color32::from_rgb(0x22, 0xd3, 0xee);
pub const OK: Color32 = Color32::from_rgb(0x34, 0xd3, 0x99);
pub const WARNING: Color32 = Color32::from_rgb(0xfb, 0xbf, 0x24);
pub const ERROR: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);

/// Màu theo trạng thái của một file trong báo cáo batch.
pub fn status_color(status: &str) -> Color32 {
    match status {
        "ok" => OK,
        "warning" => WARNING,
        "error" => ERROR,
        _ => TEXT_DIM,
    }
}

/// Đặt bảng màu, bán kính góc và khoảng cách cho toàn app.
pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = WINDOW;
    v.window_fill = PANEL;
    v.extreme_bg_color = CARD;
    v.faint_bg_color = CARD;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(12);
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = CYAN;
    v.warn_fg_color = WARNING;
    v.error_fg_color = ERROR;
    v.selection.bg_fill = ACCENT.linear_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    w.inactive.bg_fill = CARD;
    w.inactive.weak_bg_fill = CARD;
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.bg_fill = HOVER;
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    w.active.bg_fill = ACCENT;
    w.active.weak_bg_fill = ACCENT;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    w.open.bg_fill = CARD;
    w.open.weak_bg_fill = CARD;
    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = CornerRadius::same(8);
    }

    // App chỉ có một chủ đề tối, nên đặt cho cả hai biến thể sáng/tối của egui.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.visuals = v.clone();
        style.spacing.item_spacing = egui::vec2(8.0, 7.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.slider_width = 150.0;
        style.spacing.interact_size.y = 24.0;
        style.spacing.window_margin = Margin::same(14);
    });
}

/// Khung của một "Card" / "Panel" trong theme.qss (nền panel, viền mảnh, góc 12).
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
}

/// Khung không viền cho dải công cụ / thanh trên.
pub fn bar() -> egui::Frame {
    egui::Frame::new()
        .fill(SIDEBAR)
        .inner_margin(Margin::symmetric(12, 8))
}
