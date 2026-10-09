//! Điểm vào của app: mở cửa sổ eframe (egui + wgpu) và chạy [`pbe_gui::app::App`].

use pbe_gui::app;

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
