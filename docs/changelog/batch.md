# Changelog: Chỉnh nhiều ảnh (batch)

> Hiện trạng logic: [../instruction/batch.md](../instruction/batch.md)

- 2026-10-03: Ghi lại hiện trạng ban đầu.
- 2026-10-03: Thêm mục chuột phải "Mở trong Sửa ảnh đơn" (input + kết quả); đổi tên mục điều hướng thành
  "Sửa hàng loạt" — vì có thêm trang Sửa ảnh đơn / Sửa video (xem photo-editor.md, video-editor.md).
- 2026-10-03: Sửa lỗi kiểu do Pyright báo (không đổi hành vi): CLI `reconfigure` stdout qua `getattr`; danh sách kết
  quả / trình xem ảnh bỏ qua dòng không có `output_path` khi tạo thumbnail / preload — xem code-format.md.
- 2026-10-06: Tách file log cũ `docs/batch.md` thành `instruction/` (hiện trạng) + `changelog/` (lịch sử) — theo rule mới trong CLAUDE.md.
- 2026-10-09 | Thêm mới | Bản Rust của logic này: xem [rust-port.md](../instruction/rust-port.md) (GĐ4 cho `core/`, GĐ5 cho GUI egui+wgpu) — bản Python giữ nguyên, không đổi hành vi.
