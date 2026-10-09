//! Bảng chỉnh sửa gom nhóm kiểu Camera Raw (Basic / Detail / Effects + Super Resolution +
//! Image Size). Port của `ui/adjustment_panel.py`.

use egui::{RichText, Ui};
use pbe_core::settings::{
    AdjustmentSettings, ImageSizeSettings, IMAGE_SIZE_MODES, PERCENT_RANGE, PIXEL_RANGE,
    RESAMPLE_OPTIONS, SLIDERS, SUPER_RESOLUTION_OPTIONS,
};

use crate::theme;
use crate::widgets;
use crate::tr;

pub const SR_TIP: &str = "Real-ESRGAN already sharpens and denoises: with Super Resolution on, a lower Sharpening › Amount is recommended.";

/// Giá trị gõ gần nhất cho từng loại đơn vị, để đổi mode không mất số đã nhập.
pub struct ImageSizeState {
    pub last_percent: f64,
    pub last_px: f64,
}

impl Default for ImageSizeState {
    fn default() -> Self {
        ImageSizeState {
            last_percent: 100.0,
            last_px: 2048.0,
        }
    }
}

impl ImageSizeState {
    fn kind(mode: &str) -> &'static str {
        if mode == "percent" || mode == "off" {
            "percent"
        } else {
            "px"
        }
    }

    pub fn remember(&mut self, s: &ImageSizeSettings) {
        if s.mode != "off" {
            match Self::kind(&s.mode) {
                "percent" => self.last_percent = s.value,
                _ => self.last_px = s.value,
            }
        }
    }

    fn value_for(&self, mode: &str) -> f64 {
        match Self::kind(mode) {
            "percent" => self.last_percent,
            _ => self.last_px,
        }
    }
}

/// Vẽ toàn bộ bảng; trả `true` khi có gì đổi (người gọi tính lại xem trước).
pub fn panel(
    ui: &mut Ui,
    settings: &mut AdjustmentSettings,
    size_state: &mut ImageSizeState,
    enabled: bool,
) -> bool {
    let mut changed = false;
    ui.add_enabled_ui(enabled, |ui| {
        // 15 thanh trượt, gom theo panel (Basic / Detail / Effects) rồi theo group.
        for panel_name in ["Basic", "Detail", "Effects"] {
            widgets::section(ui, &tr(panel_name), true, |ui| {
                let mut last_group = "";
                for spec in SLIDERS.iter().filter(|s| s.panel == panel_name) {
                    if spec.group != last_group {
                        if spec.group != spec.label {
                            widgets::group_label(ui, &tr(spec.group));
                        }
                        last_group = spec.group;
                    }
                    let mut v = settings.get(spec.key).unwrap_or(spec.default);
                    if widgets::slider_row(ui, spec, &tr(spec.label), &mut v) {
                        settings.set(spec.key, v);
                        changed = true;
                    }
                }
            });
        }

        widgets::section(ui, &tr("Super Resolution"), true, |ui| {
            ui.horizontal(|ui| {
                for opt in SUPER_RESOLUTION_OPTIONS {
                    let label = if opt == "off" { tr("Off") } else { opt.to_string() };
                    let selected = settings.super_resolution == opt;
                    if ui.selectable_label(selected, label).clicked() && !selected {
                        settings.super_resolution = opt.to_string();
                        changed = true;
                    }
                }
            });
            if settings.sr_factor() > 1 {
                ui.label(RichText::new(tr(SR_TIP)).size(10.5).color(theme::TEXT_FAINT));
            }
        });

        widgets::section(ui, &tr("Image Size"), true, |ui| {
            if image_size_controls(ui, &mut settings.image_size, size_state) {
                changed = true;
            }
        });
    });
    changed
}

/// Chế độ resize, giá trị (% / px), phương pháp lấy mẫu lại và Don't Enlarge.
pub fn image_size_controls(
    ui: &mut Ui,
    s: &mut ImageSizeSettings,
    state: &mut ImageSizeState,
) -> bool {
    let mut changed = false;
    egui::Grid::new("image_size_grid")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label(RichText::new(tr("Resize:")).size(12.0).color(theme::TEXT_DIM));
            let current = IMAGE_SIZE_MODES
                .iter()
                .find(|(k, _)| *k == s.mode)
                .map(|(_, l)| tr(l))
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("is_mode")
                .selected_text(current)
                .width(150.0)
                .show_ui(ui, |ui| {
                    for (key, label) in IMAGE_SIZE_MODES {
                        if ui.selectable_label(s.mode == key, tr(label)).clicked() && s.mode != key {
                            state.remember(s);
                            s.mode = key.to_string();
                            s.value = state.value_for(key);
                            changed = true;
                        }
                    }
                });
            ui.end_row();

            let on = s.mode != "off";
            let percent = ImageSizeState::kind(&s.mode) == "percent";
            let (lo, hi) = if percent { PERCENT_RANGE } else { PIXEL_RANGE };
            ui.label(RichText::new(tr("Value:")).size(12.0).color(theme::TEXT_DIM));
            ui.add_enabled_ui(on, |ui| {
                ui.horizontal(|ui| {
                    let drag = egui::DragValue::new(&mut s.value)
                        .range(lo..=hi)
                        .speed(if percent { 0.5 } else { 4.0 })
                        .fixed_decimals(0);
                    if ui.add(drag).changed() {
                        state.remember(s);
                        changed = true;
                    }
                    ui.label(
                        RichText::new(if percent { "%" } else { "px" })
                            .size(11.0)
                            .color(theme::TEXT_FAINT),
                    );
                });
            });
            ui.end_row();

            ui.label(RichText::new(tr("Resample:")).size(12.0).color(theme::TEXT_DIM));
            ui.add_enabled_ui(on, |ui| {
                let current = RESAMPLE_OPTIONS
                    .iter()
                    .find(|(k, _)| *k == s.resample)
                    .map(|(_, l)| tr(l))
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("is_resample")
                    .selected_text(current)
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for (key, label) in RESAMPLE_OPTIONS {
                            if ui.selectable_label(s.resample == key, tr(label)).clicked()
                                && s.resample != key
                            {
                                s.resample = key.to_string();
                                changed = true;
                            }
                        }
                    });
            });
            ui.end_row();

            ui.label("");
            ui.add_enabled_ui(on, |ui| {
                if ui.checkbox(&mut s.dont_enlarge, tr("Don't Enlarge")).changed() {
                    changed = true;
                }
            });
            ui.end_row();
        });
    changed
}

/// Dòng mô tả "đang bật những gì" (giống `presets.describe_settings`).
pub fn summary(settings: &AdjustmentSettings) -> String {
    let parts = pbe_core::presets::describe_settings(settings);
    if parts.is_empty() {
        tr("No adjustments")
    } else {
        parts.join(" · ")
    }
}
