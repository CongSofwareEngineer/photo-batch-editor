//! Cửa sổ chính: trang đăng nhập, rồi vỏ "sidebar + trang" (chỉnh nhiều ảnh / sửa ảnh /
//! sửa video / Settings). Port của `ui/main_window.py`.

use std::path::PathBuf;

use egui::{Key, Ui};
use pbe_core::auth::AuthStore;
use pbe_core::i18n::{load_language, save_language, set_language};
use pbe_core::paths::local_appdata_dir;
use pbe_core::settings::AdjustmentSettings;

use crate::batch::{BatchView, Screen};
use crate::login::LoginView;
use crate::photo::PhotoEditorView;
use crate::settings_view::SettingsView;
use crate::sidebar;
use crate::theme;
use crate::tr;
use crate::video::VideoEditorView;
use crate::widgets;

pub const TITLE: &str = "Photo Batch Editor";
const STATE_FILE: &str = "state.json";
const AUTH_FILE: &str = "auth.json";

/// `%LOCALAPPDATA%\PhotoBatchEditor` / `~/Library/Caches/PhotoBatchEditor`.
pub fn app_data_dir() -> PathBuf {
    local_appdata_dir()
}

/// Hộp thoại xác nhận đang mở.
enum Confirm {
    None,
    SignOut,
    Quit,
}

pub struct App {
    auth: AuthStore,
    data_dir: PathBuf,
    signed_in: bool,
    page: String,
    login: LoginView,
    pub batch: BatchView,
    pub photo: PhotoEditorView,
    pub video: VideoEditorView,
    settings: SettingsView,
    confirm: Confirm,
    theme_applied: bool,
    quitting: bool,
}

impl App {
    pub fn new(ctx: &egui::Context) -> Self {
        let data_dir = app_data_dir();
        set_language(&load_language(&data_dir));
        theme::apply(ctx);
        let auth = AuthStore::new(&data_dir.join(AUTH_FILE));
        let preset_dir = data_dir.join("presets");
        let mut app = App {
            signed_in: auth.is_remembered(),
            auth,
            data_dir,
            page: "editor".to_string(),
            login: LoginView::default(),
            batch: BatchView::new(preset_dir.clone()),
            photo: PhotoEditorView::default(),
            video: VideoEditorView::default(),
            settings: SettingsView::new(preset_dir),
            confirm: Confirm::None,
            theme_applied: true,
            quitting: false,
        };
        app.load_state();
        app
    }

    // --- Trạng thái -----------------------------------------------------------------------

    fn load_state(&mut self) {
        let Ok(text) = std::fs::read_to_string(self.data_dir.join(STATE_FILE)) else {
            return;
        };
        let Ok(state) = serde_json::from_str::<serde_json::Value>(&text) else {
            return;
        };
        if let Some(s) = state.get("settings") {
            self.batch.set_settings(AdjustmentSettings::from_dict(s));
        }
        if let Some(name) = state.get("preset").and_then(|v| v.as_str()) {
            if !name.is_empty() {
                self.batch.current_preset = Some(name.to_string());
            }
        }
        if let Some(folder) = state.get("last_folder").and_then(|v| v.as_str()) {
            let path = PathBuf::from(folder);
            if !folder.is_empty() && path.is_dir() {
                self.batch.set_folder(Some(path));
            }
        }
    }

    fn save_state(&self) {
        let state = serde_json::json!({
            "version": 1,
            "device": "cpu",
            "settings": self.batch.settings.to_dict(),
            "last_folder": self.batch.folder.as_ref().map(|f| f.display().to_string()).unwrap_or_default(),
            "preset": self.batch.current_preset.clone().unwrap_or_default(),
        });
        let _ = std::fs::create_dir_all(&self.data_dir);
        let path = self.data_dir.join(STATE_FILE);
        let tmp = path.with_extension("json.tmp");
        if serde_json::to_string_pretty(&state)
            .map(|t| std::fs::write(&tmp, t))
            .is_ok()
        {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    fn switch_language(&mut self, code: &str) {
        if self.batch.is_running() || self.video.is_busy() {
            return;
        }
        set_language(code);
        save_language(&self.data_dir, code);
        // Các nhãn được dịch lại ở khung sau; chỉ cần nạp lại danh sách preset có tên dịch.
        self.batch.reload_presets();
        self.settings.reload(None);
    }

    fn show_page(&mut self, key: &str) {
        self.page = key.to_string();
    }

    // --- Vẽ -------------------------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui) {
        if !self.theme_applied {
            theme::apply(ui.ctx());
            self.theme_applied = true;
        }
        if !self.signed_in {
            let out = self.login.ui(ui, &mut self.auth);
            if let Some(code) = out.language {
                self.switch_language(&code);
            }
            if out.signed_in {
                self.signed_in = true;
                self.show_page("editor");
            }
            return;
        }

        self.page_shortcuts(ui);
        let status = if self.batch.is_running() {
            tr("Batch running…")
        } else if self.video.is_busy() {
            tr("Exporting video…")
        } else {
            String::new()
        };
        let page = self.page.clone();
        let username = self.auth.username().to_string();
        let nav = egui::Panel::left(egui::Id::new("sidebar"))
            .exact_size(220.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(14, 18)),
            )
            .show(ui, |ui| sidebar::ui(ui, &page, &username, &status))
            .inner;
        if let Some(next) = nav.page {
            self.show_page(&next);
        }
        if nav.sign_out {
            self.confirm = Confirm::SignOut;
        }

        match self.page.as_str() {
            "photo" => self.photo.ui(ui),
            "video" => self.video.ui(ui),
            "settings" => {
                let out = self.settings.ui(ui, &mut self.auth);
                if let Some(code) = out.language {
                    self.switch_language(&code);
                }
                if out.presets_changed {
                    self.batch.reload_presets();
                }
                if let Some(p) = out.apply_preset {
                    self.batch.current_preset = Some(p.name.clone());
                    self.batch.set_settings(p.settings);
                    if self.batch.screen == Screen::Results {
                        self.batch.screen = Screen::Prepare;
                    }
                    self.show_page("editor");
                }
                if out.sign_out {
                    self.confirm = Confirm::SignOut;
                }
            }
            _ => self.batch.ui(ui),
        }
        self.ui_confirm(ui);
        self.handle_close(ui);
    }

    fn page_shortcuts(&mut self, ui: &mut Ui) {
        // Ctrl+1 là "100 %" trong photo editor (như Photoshop): phím trang nhường chỗ ở đó.
        let in_photo = self.page == "photo";
        let pressed = ui.input(|i| {
            let cmd = i.modifiers.command;
            [
                cmd && i.key_pressed(Key::Num1),
                cmd && i.key_pressed(Key::Num2),
                cmd && i.key_pressed(Key::Num3),
                cmd && i.key_pressed(Key::Num4),
            ]
        });
        for (i, (key, _, _)) in sidebar::PAGES.iter().enumerate() {
            if pressed[i] && !(i == 0 && in_photo) {
                self.show_page(key);
            }
        }
    }

    fn ui_confirm(&mut self, ui: &mut Ui) {
        let (title, body, ok_label) = match &self.confirm {
            Confirm::None => return,
            Confirm::SignOut if self.batch.is_running() => (
                tr("Sign out"),
                tr("A batch is running. Wait for it to finish or cancel it first."),
                String::new(),
            ),
            Confirm::SignOut => (
                tr("Sign out"),
                tr("Sign out? You will need your password next time."),
                tr("Sign out"),
            ),
            Confirm::Quit if self.batch.is_running() => (
                tr("Quit"),
                tr("A batch is running. Cancel it and quit?"),
                tr("Quit"),
            ),
            Confirm::Quit if self.video.is_busy() => (
                tr("Quit"),
                tr("A video is being exported. Cancel it and quit?"),
                tr("Quit"),
            ),
            Confirm::Quit => (
                tr("Quit"),
                tr("There are unsaved changes. Quit anyway?"),
                tr("Quit"),
            ),
        };
        let mut accepted = false;
        let mut dismissed = false;
        egui::Modal::new(egui::Id::new("confirm")).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            widgets::heading(ui, &title);
            ui.add_space(8.0);
            ui.label(body);
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if !ok_label.is_empty() && widgets::primary(ui, &ok_label, true).clicked() {
                    accepted = true;
                }
                if widgets::secondary(ui, &tr("Cancel"), true).clicked() {
                    dismissed = true;
                }
            });
        });
        if dismissed {
            self.confirm = Confirm::None;
            self.quitting = false;
            return;
        }
        if !accepted {
            return;
        }
        match std::mem::replace(&mut self.confirm, Confirm::None) {
            Confirm::SignOut => {
                self.save_state();
                self.auth.logout();
                self.login.reset();
                self.signed_in = false;
            }
            Confirm::Quit => {
                self.batch.cancel();
                self.video.shutdown();
                self.save_state();
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }

    fn handle_close(&mut self, ui: &mut Ui) {
        if !ui.ctx().input(|i| i.viewport().close_requested()) {
            return;
        }
        let busy = self.batch.is_running() || self.video.is_busy();
        let unsaved = self.photo.modified() || self.video.modified;
        if (busy || unsaved) && !self.quitting {
            self.quitting = true;
            self.confirm = Confirm::Quit;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        self.video.shutdown();
        self.batch.cancel();
        self.save_state();
    }
}
