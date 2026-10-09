# Changelog: Đăng nhập (Login)

> Hiện trạng logic: [../instruction/login.md](../instruction/login.md)

- 2026-10-03: Ghi lại hiện trạng ban đầu.
- 2026-10-06: Tách file log cũ `docs/login.md` thành `instruction/` (hiện trạng) + `changelog/` (lịch sử) — theo rule mới trong CLAUDE.md.
- 2026-10-09 | Thêm mới | Bản Rust của logic này: xem [rust-port.md](../instruction/rust-port.md) (GĐ4 cho `core/`, GĐ5 cho GUI egui+wgpu) — bản Python giữ nguyên, không đổi hành vi.
- 2026-10-09 | Sửa | Đăng nhập: cập nhật `File:` sang Rust (`rust/core/src/auth.rs`, `rust/gui/src/login.rs`, `sidebar.rs`, `settings_view.rs`) — vì mã Python đã bị xoá.
