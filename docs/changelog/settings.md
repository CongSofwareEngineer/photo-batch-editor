# Changelog: Setting (preset) & trang Settings

> Hiện trạng logic: [../instruction/settings.md](../instruction/settings.md)

- 2026-10-03: Ghi lại hiện trạng ban đầu.
- 2026-10-03: Phím tắt trang Settings đổi Ctrl+2 → Ctrl+4 — vì thêm trang Sửa ảnh đơn (Ctrl+2) và Sửa video (Ctrl+3).
- 2026-10-03: `load_settings_json`: tên lấy từ `name` nếu là chuỗi, ngược lại tên file (viết lại cho Pyright, không đổi
  hành vi) — xem code-format.md.
- 2026-10-06: Tách file log cũ `docs/settings.md` thành `instruction/` (hiện trạng) + `changelog/` (lịch sử) — theo rule mới trong CLAUDE.md.
- 2026-10-09 | Thêm mới | Bản Rust của logic này: xem [rust-port.md](../instruction/rust-port.md) (GĐ4 cho `core/`, GĐ5 cho GUI egui+wgpu) — bản Python giữ nguyên, không đổi hành vi.
