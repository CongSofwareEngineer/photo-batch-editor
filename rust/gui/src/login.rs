//! Trang đăng nhập, hiện cho tới khi người dùng đăng nhập. Port của `ui/login_view.py`.

use egui::{Color32, CornerRadius, RichText, Stroke, Ui, vec2};
use pbe_core::auth::AuthStore;
use pbe_core::i18n::LANGUAGES;

use crate::theme;
use crate::tr;
use crate::widgets;

pub struct LoginView {
    pub username: String,
    pub password: String,
    pub remember: bool,
    pub reveal: bool,
    pub error: Option<String>,
}

impl Default for LoginView {
    fn default() -> Self {
        LoginView {
            username: String::new(),
            password: String::new(),
            remember: true,
            reveal: false,
            error: None,
        }
    }
}

/// Kết quả một khung của trang đăng nhập.
pub struct LoginOutcome {
    pub signed_in: bool,
    pub language: Option<String>,
}

impl LoginView {
    pub fn reset(&mut self) {
        self.password.clear();
        self.error = None;
    }

    pub fn ui(&mut self, ui: &mut Ui, auth: &mut AuthStore) -> LoginOutcome {
        let mut out = LoginOutcome {
            signed_in: false,
            language: None,
        };
        if self.username.is_empty() {
            self.username = auth.username().to_string();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::WINDOW))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() - 420.0).max(20.0) / 2.0);
                    egui::Frame::new()
                        .fill(theme::PANEL)
                        .stroke(Stroke::new(1.0, Color32::from_rgb(0x24, 0x31, 0x4f)))
                        .corner_radius(CornerRadius::same(18))
                        .inner_margin(egui::Margin::same(26))
                        .show(ui, |ui| {
                            ui.set_width(380.0);
                            ui.vertical_centered(|ui| {
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(56.0, 56.0), egui::Sense::hover());
                                ui.painter().rect_filled(rect, CornerRadius::same(14), theme::ACCENT);
                                ui.painter().rect_filled(
                                    egui::Rect::from_center_size(rect.center(), vec2(20.0, 20.0)),
                                    CornerRadius::same(6),
                                    theme::ACCENT_2,
                                );
                                ui.add_space(8.0);
                                widgets::heading(ui, "Photo Batch Editor");
                                widgets::subtitle(ui, &tr("Sign in to continue"));
                            });
                            ui.add_space(16.0);
                            ui.add(
                                egui::TextEdit::singleline(&mut self.username)
                                    .hint_text(tr("User name"))
                                    .desired_width(f32::INFINITY)
                                    .margin(egui::Margin::symmetric(10, 8)),
                            );
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                let mut edit = egui::TextEdit::singleline(&mut self.password)
                                    .hint_text(tr("Password"))
                                    .desired_width(ui.available_width() - 40.0)
                                    .margin(egui::Margin::symmetric(10, 8));
                                if !self.reveal {
                                    edit = edit.password(true);
                                }
                                ui.add(edit);
                                if ui
                                    .add(egui::Button::new(if self.reveal { "🙈" } else { "👁" }).frame(false))
                                    .on_hover_text(tr("Show password"))
                                    .clicked()
                                {
                                    self.reveal = !self.reveal;
                                }
                            });
                            ui.add_space(8.0);
                            ui.checkbox(&mut self.remember, tr("Keep me signed in"))
                                .on_hover_text(tr(
                                    "Next time the app opens directly, without asking for the password",
                                ));
                            if let Some(e) = &self.error {
                                ui.add_space(8.0);
                                widgets::error_banner(ui, e);
                            }
                            ui.add_space(14.0);
                            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                            let clicked = ui
                                .add_sized(
                                    vec2(ui.available_width(), 38.0),
                                    egui::Button::new(
                                        RichText::new(tr("Sign in")).strong().color(Color32::WHITE),
                                    )
                                    .fill(theme::ACCENT)
                                    .corner_radius(CornerRadius::same(10)),
                                )
                                .clicked();
                            if clicked || enter {
                                if auth.login(&self.username, &self.password, self.remember) {
                                    self.password.clear();
                                    self.error = None;
                                    out.signed_in = true;
                                } else {
                                    self.error = Some(tr("Wrong user name or password."));
                                }
                            }
                            if auth.uses_default_credentials() {
                                ui.add_space(10.0);
                                widgets::info_banner(
                                    ui,
                                    &tr("Signed in with the default account. Change it in Settings."),
                                    theme::WARNING,
                                );
                            }
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                for (code, label) in LANGUAGES {
                                    let selected = pbe_core::i18n::language() == code;
                                    if ui.selectable_label(selected, label).clicked() && !selected {
                                        out.language = Some(code.to_string());
                                    }
                                }
                            });
                        });
                });
            });
        out
    }
}
