//! Photo Batch Editor — GUI viết bằng egui (vẽ qua wgpu).
//!
//! Phần CPU (framebuffer, ghép tài liệu, xem trước) tách thành crate thư viện để test được;
//! `main.rs` chỉ mở cửa sổ.
//!
//! Giai đoạn 5 của việc port sang Rust (xem `docs/instruction/rust-port.md`). Toàn bộ logic
//! nằm trong `pbe-core`; crate này chỉ là lớp giao diện, nên mọi hành vi (chỉnh sửa, batch,
//! đọc/ghi ảnh, FFmpeg) dùng đúng code đã có test.
//!
//! Kiến trúc hiển thị: egui vẽ widget qua **wgpu**; canvas của photo editor và khung xem
//! trước của video được **ghép bằng CPU** vào framebuffer RGBA (xem [`fb`]) rồi tải lên
//! texture wgpu — nhờ vậy pixel trên màn hình đúng bằng pixel xuất ra file.

pub mod adjust;
pub mod app;
pub mod batch;
pub mod fb;
pub mod login;
pub mod photo;
pub mod preview;
pub mod settings_view;
pub mod sidebar;
pub mod theme;
pub mod video;
pub mod widgets;

/// Dịch một chuỗi UI (bọc `pbe_core::i18n::tr` cho gọn).
pub fn tr(text: &str) -> String {
    pbe_core::i18n::tr(text)
}

/// Dịch kèm tham số, vd. `tr_args("{done} of {total}", &[("done", "3"), ("total", "9")])`.
pub fn tr_args(text: &str, args: &[(&str, &str)]) -> String {
    pbe_core::i18n::tr_args(text, args)
}
