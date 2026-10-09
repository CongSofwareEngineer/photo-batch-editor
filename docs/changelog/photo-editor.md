# Changelog: Sửa ảnh đơn (Photo editor)

> Hiện trạng logic: [../instruction/photo-editor.md](../instruction/photo-editor.md)

- 2026-10-03: Tạo file log trước khi code (kế hoạch).
- 2026-10-03: Hoàn thành GĐ1–GĐ3: khung trang + canvas zoom/pan + layer + undo; chữ, crop, chèn ảnh, làm mờ /
  pixelate, chỉnh ảnh, xoay/lật, xuất JPEG/PNG, file dự án `.pbep`; mở từ tab hàng loạt bằng chuột phải.
  Thanh điều hướng thêm trang: Sửa hàng loạt (Ctrl+1), Sửa ảnh đơn (Ctrl+2), Sửa video (Ctrl+3),
  Cài đặt (Ctrl+4 — trước là Ctrl+2).
- 2026-10-03: Font mặc định của layer chữ theo hệ điều hành (`core.system.DEFAULT_FONT_FAMILY`: Segoe UI / Helvetica
  Neue trên Mac) — hỗ trợ macOS, xem macos.md.
- 2026-10-03: Sửa lỗi kiểu do Pyright báo (không đổi hành vi): `QPolygonF.at(i)` thay `poly[i]`, `enterEvent(QEnterEvent)`,
  `edit_layer(..., None, **{field: ...})` khi lật; `'PNG'` trong `project_io` giữ str (stub PySide6 sai) — xem code-format.md.
- 2026-10-06: Tách file log cũ `docs/photo-editor.md` thành `instruction/` (hiện trạng) + `changelog/` (lịch sử) — theo rule mới trong CLAUDE.md.
- 2026-10-09 | Thêm mới | Bản Rust của logic này: xem [rust-port.md](../instruction/rust-port.md) (GĐ4 cho `core/`, GĐ5 cho GUI egui+wgpu) — bản Python giữ nguyên, không đổi hành vi.
- 2026-10-09 | Sửa | Sửa ảnh đơn: cập nhật `File:` / Test sang Rust (`rust/core/src/photo/`, `rust/gui/src/photo/`, `rust/gui/tests/cpu_render.rs`); layer chữ và `.pbep` vẫn **chưa** port — vì mã Python đã bị xoá.
