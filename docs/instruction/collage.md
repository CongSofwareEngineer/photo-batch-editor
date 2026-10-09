# Cắt ghép layout (Collage)

- **Mục đích:** tạo ảnh ghép từ nhiều ảnh theo mẫu bố cục, chỉnh khoảng cách, màu nền, bo góc, kéo ảnh trong
  từng ô để căn vị trí; kết quả mở trong Sửa ảnh đơn như một ảnh mới (thêm chữ, xuất… như bình thường).
- **File:** `core/photo/collage.py` (hình học, không Qt), `ui/photo/collage_dialog.py` (hộp thoại + vẽ),
  mở từ `ui/photo/photo_editor_view.py` (công cụ L / menu "Tạo ảnh ghép mới…")
- **Changelog:** [../changelog/collage.md](../changelog/collage.md)
- **Bản Rust:** hình học ô → `rust/core/src/photo/collage.rs`. **Chưa** port: hộp thoại collage. Chi tiết: [rust-port.md](rust-port.md).

## Logic chính

- 4 mẫu (`TEMPLATES`, toạ độ ô theo tỉ lệ 0..1): 2 ảnh ngang, 2 ảnh dọc, lưới 2×2, 1 lớn + 2 nhỏ.
- `cell_rects`: đổi sang pixel; khoảng cách = đủ ở viền ngoài, một nửa mỗi bên ở đường nối giữa các ô
  → khoảng cách giữa 2 ô luôn bằng khoảng cách viền.
- `cover_rect`: ảnh phủ kín ô (kiểu "cover") × zoom (≥ 1) + offset; `clamp_offset` giữ cho ảnh luôn phủ kín ô.
- Kích thước: 1:1 (2000²), 4:3, 3:4, 16:9, 9:16. Bo góc = `addRoundedRect` làm clip; ô trống vẽ mờ.
- Tương tác: bấm ô trống → chọn ảnh; double-click → thay ảnh; kéo → căn vị trí; lăn chuột → zoom trong ô;
  "Chọn ảnh…" điền nhiều ảnh lần lượt vào các ô trống. Đổi mẫu giữ lại các ảnh đã chọn theo thứ tự.
- "Mở trong Sửa ảnh đơn" vẽ ảnh ghép ở kích thước đầy đủ → document mới tên "collage" (chưa lưu → hỏi khi đóng).

## Lưu ý / giới hạn

- Ảnh ghép là 1 ảnh nền phẳng (các ô không còn là layer riêng sau khi mở trong editor).

## Test

`tests/test_photo_core.py`: `test_collage_cells_spacing` (mọi mẫu: trong lề, không chồng, đủ khoảng cách),
`test_collage_cover_and_clamp`. Thủ công: công cụ L → chọn 3 ảnh, mẫu 1 lớn + 2 nhỏ, bo góc 40, kéo ảnh.
