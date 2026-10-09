//! Photo Batch Editor — GUI viết bằng egui (vẽ qua wgpu).
//!
//! Giai đoạn 5 của việc port sang Rust (xem `docs/instruction/rust-port.md`). Toàn bộ logic
//! nằm trong `pbe-core`; crate này chỉ là lớp giao diện, nên mọi hành vi (chỉnh sửa, batch,
//! đọc/ghi ảnh, FFmpeg) dùng đúng code đã có test.
//!
//! Kiến trúc hiển thị: egui vẽ widget qua **wgpu**; canvas của photo editor và khung xem
//! trước của video được **ghép bằng CPU** vào framebuffer RGBA (xem [`fb`]) rồi tải lên
//! texture wgpu — nhờ vậy pixel trên màn hình đúng bằng pixel xuất ra file.

mod adjust;
mod app;
mod batch;
mod fb;
mod login;
mod photo;
mod preview;
mod settings_view;
mod sidebar;
mod theme;
mod video;
mod widgets;

/// Dịch một chuỗi UI (bọc `pbe_core::i18n::tr` cho gọn).
pub fn tr(text: &str) -> String {
    pbe_core::i18n::tr(text)
}

/// Dịch kèm tham số, vd. `tr_args("{done} of {total}", &[("done", "3"), ("total", "9")])`.
pub fn tr_args(text: &str, args: &[(&str, &str)]) -> String {
    pbe_core::i18n::tr_args(text, args)
}

struct Shell {
    app: Option<app::App>,
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let app = self.app.get_or_insert_with(|| app::App::new(ui.ctx()));
        app.ui(ui);
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1100.0, 680.0])
            .with_title(app::TITLE),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        app::TITLE,
        options,
        Box::new(|_cc| Ok(Box::new(Shell { app: None }))),
    )
}
