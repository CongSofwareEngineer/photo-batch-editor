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
- 2026-10-15 | Sửa | Bố cục kiểu Photoshop: thêm thanh menu File / Edit / Image / View, thanh tuỳ chọn công cụ, thanh công cụ dọc bên trái (icon vector, phím V C B I L H Z), thanh trạng thái (zoom, cỡ ảnh, toạ độ con trỏ) — người dùng muốn thao tác giống Photoshop.
- 2026-10-15 | Thêm mới | Công cụ Bàn tay / Thu phóng, nút góc đổi cỡ layer (Shift giữ tỉ lệ), mũi tên nhích layer, Space + kéo để cuộn, lưới 1/3 khi cắt, ẩn/hiện Nền, Ctrl+O / Ctrl+E / Ctrl+J — đưa các thao tác Photoshop cơ bản (đã có ở bản Python) vào bản Rust.
- 2026-10-15 | Thêm mới | Tách nền trong Sửa ảnh đơn: `Background.cutout` (matte + nền thay thế), panel "Tách nền", menu Image › Tách nền, nền ô vuông chỗ trong suốt trên canvas (chi tiết: background-removal.md) — người dùng muốn xoá / đổi phông ảnh chân dung, ảnh thẻ.
- 2026-10-15 | Thêm mới | Chế độ chỉnh vùng tách nền trong Sửa ảnh đơn (`photo/mask_edit.rs`): cọ Thêm / Bớt trên canvas, vòng tròn cỡ cọ, phím Enter / Esc / Ctrl+Z / [ ] / X riêng cho chế độ này, tạm tắt công cụ khác (chi tiết: background-removal.md) — để người dùng sửa vùng AI chọn trước khi tách thật.
