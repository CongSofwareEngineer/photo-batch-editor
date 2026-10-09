//! Trang Settings: thiết lập đã lưu (preset), tài khoản, ngôn ngữ.
//! Port của `ui/settings_view.py`.

use egui::{RichText, Ui};
use pbe_core::auth::AuthStore;
use pbe_core::i18n::LANGUAGES;
use pbe_core::presets::{self, Preset};
use pbe_core::settings::AdjustmentSettings;

use crate::adjust::{self, ImageSizeState};
use crate::theme;
use crate::tr;
use crate::widgets;

#[derive(PartialEq, Clone, Copy)]
pub enum Tab {
    Filters,
    Account,
    General,
}

pub struct SettingsView {
    pub tab: Tab,
    pub presets: Vec<Preset>,
    pub selected: Option<usize>,
    draft: AdjustmentSettings,
    draft_name: String,
    size_state: ImageSizeState,
    user_dir: std::path::PathBuf,
    error: Option<String>,
    info: Option<String>,
    // tài khoản
    current_password: String,
    new_username: String,
    new_password: String,
}

/// Việc trang Settings muốn cửa sổ chính làm.
#[derive(Default)]
pub struct SettingsOutcome {
    pub language: Option<String>,
    pub apply_preset: Option<Preset>,
    pub presets_changed: bool,
    pub sign_out: bool,
}

impl SettingsView {
    pub fn new(user_dir: std::path::PathBuf) -> Self {
        let mut v = SettingsView {
            tab: Tab::Filters,
            presets: vec![],
            selected: None,
            draft: AdjustmentSettings::default(),
            draft_name: String::new(),
            size_state: ImageSizeState::default(),
            user_dir,
            error: None,
            info: None,
            current_password: String::new(),
            new_username: String::new(),
            new_password: String::new(),
        };
        v.reload(None);
        v
    }

    pub fn reload(&mut self, select: Option<&str>) {
        self.presets = presets::list_builtin_presets();
        self.presets.extend(presets::list_user_presets(&self.user_dir));
        self.selected = match select {
            Some(name) => self.presets.iter().position(|p| p.name == name),
            None => self.selected.filter(|i| *i < self.presets.len()),
        };
        self.sync_draft();
    }

    fn sync_draft(&mut self) {
        if let Some(p) = self.selected.and_then(|i| self.presets.get(i)) {
            self.draft = p.settings.clone();
            self.draft_name = p.name.clone();
            self.size_state.remember(&self.draft.image_size);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, auth: &mut AuthStore) -> SettingsOutcome {
        let mut out = SettingsOutcome::default();
        egui::Panel::top(egui::Id::new("settings_tabs"))
            .frame(theme::bar())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (Tab::Filters, tr("Filter settings")),
                        (Tab::Account, tr("Account")),
                        (Tab::General, tr("General")),
                    ] {
                        if ui.selectable_label(self.tab == tab, label).clicked() {
                            self.tab = tab;
                        }
                    }
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(e) = self.error.clone() {
                widgets::error_banner(ui, &e);
                ui.add_space(6.0);
            }
            if let Some(i) = self.info.clone() {
                widgets::info_banner(ui, &i, theme::OK);
                ui.add_space(6.0);
            }
            match self.tab {
                Tab::Filters => self.ui_filters(ui, &mut out),
                Tab::Account => self.ui_account(ui, auth, &mut out),
                Tab::General => self.ui_general(ui, &mut out),
            }
        });
        out
    }

    fn ui_filters(&mut self, ui: &mut Ui, out: &mut SettingsOutcome) {
        egui::Panel::left(egui::Id::new("settings_presets"))
            .default_size(260.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(12)))
            .show(ui, |ui| {
                widgets::caption(ui, &tr("Saved settings"));
                ui.separator();
                let mut pick = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, p) in self.presets.iter().enumerate() {
                        let sub = if p.builtin { tr("Built-in") } else { tr("Mine") };
                        if widgets::list_row(
                            ui,
                            &presets::display_name(p),
                            Some(&sub),
                            self.selected == Some(i),
                        )
                        .clicked()
                        {
                            pick = Some(i);
                        }
                    }
                });
                if let Some(i) = pick {
                    self.selected = Some(i);
                    self.sync_draft();
                }
                ui.separator();
                if widgets::secondary(ui, &tr("New setting"), true).clicked() {
                    self.create_new();
                    out.presets_changed = true;
                }
            });

        egui::CentralPanel::default().show(ui, |ui| {
            let Some(index) = self.selected else {
                widgets::subtitle(ui, &tr("Choose a setting on the left"));
                return;
            };
            let builtin = self.presets[index].builtin;
            ui.horizontal(|ui| {
                if builtin {
                    widgets::heading(ui, &presets::display_name(&self.presets[index]));
                } else {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.draft_name)
                            .desired_width(240.0)
                            .margin(egui::Margin::symmetric(8, 6)),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary(ui, &tr("Use in Batch edit"), true).clicked() {
                        out.apply_preset = Some(Preset {
                            name: self.draft_name.clone(),
                            settings: self.draft.clone(),
                            path: self.presets[index].path.clone(),
                            builtin,
                        });
                    }
                    if widgets::secondary(ui, &tr("Save"), !builtin).clicked() {
                        self.save_current();
                        out.presets_changed = true;
                    }
                    if widgets::danger(ui, &tr("Delete"), !builtin).clicked() {
                        self.delete_current();
                        out.presets_changed = true;
                    }
                });
            });
            widgets::subtitle(ui, &adjust::summary(&self.draft));
            ui.add_space(8.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_max_width(420.0);
                let mut size_state = std::mem::take(&mut self.size_state);
                adjust::panel(ui, &mut self.draft, &mut size_state, !builtin);
                self.size_state = size_state;
            });
        });
    }

    fn create_new(&mut self) {
        let existing: Vec<String> = self.presets.iter().map(|p| p.name.clone()).collect();
        let name = presets::unique_name(&tr("My setting"), existing.iter().map(String::as_str));
        match presets::save_user_preset(
            &self.user_dir,
            &name,
            &AdjustmentSettings::default(),
            None,
        ) {
            Ok(_) => {
                self.reload(Some(&name));
                self.info = Some(tr("Setting saved"));
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn save_current(&mut self) {
        let Some(index) = self.selected else { return };
        let preset = self.presets[index].clone();
        if preset.builtin {
            return;
        }
        let new_name = self.draft_name.trim().to_string();
        if new_name.is_empty() {
            self.error = Some(tr("The name cannot be empty."));
            return;
        }
        // Đổi tên trước (file được đổi tên theo), rồi ghi giá trị.
        let target = if new_name != preset.name {
            match presets::rename_user_preset(&preset, &new_name) {
                Ok(p) => p,
                Err(e) => {
                    self.error = Some(e.to_string());
                    return;
                }
            }
        } else {
            preset
        };
        match presets::save_user_preset(
            &self.user_dir,
            &new_name,
            &self.draft,
            target.path.as_deref(),
        ) {
            Ok(_) => {
                self.reload(Some(&new_name));
                self.error = None;
                self.info = Some(tr("Setting saved"));
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn delete_current(&mut self) {
        let Some(index) = self.selected else { return };
        let preset = self.presets[index].clone();
        match presets::delete_user_preset(&preset) {
            Ok(()) => {
                self.selected = None;
                self.reload(None);
                self.error = None;
                self.info = Some(tr("Setting deleted"));
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn ui_account(&mut self, ui: &mut Ui, auth: &mut AuthStore, out: &mut SettingsOutcome) {
        ui.set_max_width(420.0);
        widgets::heading(ui, &tr("Account"));
        widgets::subtitle(
            ui,
            &format!("{}: {}", tr("Signed in"), auth.username()),
        );
        if auth.uses_default_credentials() {
            ui.add_space(6.0);
            widgets::info_banner(
                ui,
                &tr("Signed in with the default account. Change it in Settings."),
                theme::WARNING,
            );
        }
        ui.add_space(12.0);
        if self.new_username.is_empty() {
            self.new_username = auth.username().to_string();
        }
        ui.label(RichText::new(tr("Current password")).size(12.0).color(theme::TEXT_DIM));
        ui.add(
            egui::TextEdit::singleline(&mut self.current_password)
                .password(true)
                .desired_width(f32::INFINITY),
        );
        ui.label(RichText::new(tr("User name")).size(12.0).color(theme::TEXT_DIM));
        ui.add(egui::TextEdit::singleline(&mut self.new_username).desired_width(f32::INFINITY));
        ui.label(
            RichText::new(tr("New password (leave empty to keep it)"))
                .size(12.0)
                .color(theme::TEXT_DIM),
        );
        ui.add(
            egui::TextEdit::singleline(&mut self.new_password)
                .password(true)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if widgets::primary(ui, &tr("Save"), true).clicked() {
                match auth.change_credentials(
                    &self.current_password,
                    &self.new_username,
                    &self.new_password,
                ) {
                    Ok(()) => {
                        self.current_password.clear();
                        self.new_password.clear();
                        self.error = None;
                        self.info = Some(tr("Account updated"));
                    }
                    Err(e) => self.error = Some(pbe_core::i18n::tr_msg(&e.to_string())),
                }
            }
            if widgets::danger(ui, &tr("Sign out"), true).clicked() {
                out.sign_out = true;
            }
        });
    }

    fn ui_general(&mut self, ui: &mut Ui, out: &mut SettingsOutcome) {
        ui.set_max_width(420.0);
        widgets::heading(ui, &tr("General"));
        ui.add_space(10.0);
        widgets::caption(ui, &tr("Language"));
        for (code, label) in LANGUAGES {
            let selected = pbe_core::i18n::language() == code;
            if ui.selectable_label(selected, label).clicked() && !selected {
                out.language = Some(code.to_string());
            }
        }
        ui.add_space(14.0);
        widgets::caption(ui, &tr("Version"));
        let v = pbe_core::version::read_version(None);
        widgets::subtitle(ui, &v.full());
        ui.add_space(14.0);
        widgets::caption(ui, &tr("FFmpeg"));
        match pbe_core::video::ffmpeg::find_ffmpeg(false) {
            Ok(p) => widgets::subtitle(ui, &p.display().to_string()),
            Err(e) => widgets::error_banner(ui, &pbe_core::i18n::tr_msg(&e.to_string())),
        }
    }
}
