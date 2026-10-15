# Cắt ghép layout (Collage)

- **Mục đích:** tạo ảnh ghép từ nhiều ảnh theo mẫu bố cục, chỉnh khoảng cách, màu nền, bo góc, kéo ảnh trong
  từng ô để căn vị trí; kết quả mở trong Sửa ảnh đơn như một ảnh mới (thêm chữ, xuất… như bình thường).
- **File:** `rust/core/src/photo/collage.rs` (mẫu, hình học ô, `render_collage`, `cell_at` — không UI),
  `rust/gui/src/photo/collage.rs` (cửa sổ egui `CollageView`), mở từ `rust/gui/src/photo/mod.rs`
  (nút L trên thanh công cụ dọc, File › Ảnh ghép mới…, nút ở màn hình trống).
- **Changelog:** [../changelog/collage.md](../changelog/collage.md)
- **Ghi chú:** mã Python đã xoá; đã port đủ sang Rust (hình học + ghép pixel ở core, cửa sổ ở gui). Chi tiết: [rust-port.md](rust-port.md).

## Logic chính

- 8 mẫu (`TEMPLATES`, toạ độ ô theo tỉ lệ 0..1): 2 ảnh ngang, 2 ảnh dọc, lưới 2×2, 1 lớn + 2 nhỏ,
  3 ảnh ngang, 3 ảnh dọc, 1 trên + 2 dưới, lưới 3×3. Nút chọn mẫu vẽ thu nhỏ bố cục.
- `cell_rects`: đổi sang pixel; khoảng cách = đủ ở viền ngoài, một nửa mỗi bên ở đường nối giữa các ô
  → khoảng cách giữa 2 ô luôn bằng khoảng cách viền.
- `cover_rect`: ảnh phủ kín ô (kiểu "cover") × zoom (≥ 1) + offset; `clamp_offset` giữ cho ảnh luôn phủ kín ô.
- Kích thước: 1:1 (2000²), 4:3, 3:4, 16:9, 9:16. Khoảng cách 0–200 px, bo góc 0–300 px, màu nền tuỳ chọn.
- `render_collage(style, cells, scale, empty_fill)`: ghép ở tỉ lệ `scale` (xem trước < 1, xuất = 1). Góc bo
  khử răng cưa theo độ phủ pixel; ảnh nguồn lớn hơn ô ≥ 2 lần được thu nhỏ trung bình khối trước khi lấy mẫu
  song tuyến tính. Ô trống: xem trước tô `empty_fill` + nhãn "+ Thêm ảnh", khi xuất để lộ nền.
- Xem trước dùng ảnh thu nhỏ (cạnh dài ≤ 1200 px), xuất dùng ảnh gốc — bố cục giống hệt vì `cover_rect` chỉ
  phụ thuộc tỉ lệ ảnh. Chỉ ghép lại khi có thay đổi (`dirty`) hoặc cỡ hiển thị đổi > 2 %.
- Tương tác: bấm ô trống → chọn ảnh; double-click → thay ảnh; kéo → căn vị trí; lăn chuột → zoom trong ô
  (1–5×); "Chọn ảnh…" điền nhiều ảnh lần lượt vào các ô trống; "Xoá hết". Đổi mẫu giữ lại các ảnh đã chọn
  theo thứ tự (lệch về 0). Ô đang trỏ có viền xanh.
- "Mở trong Sửa ảnh đơn" ghép ở kích thước đầy đủ → document mới tên "collage" (đánh dấu chưa lưu), rồi đóng
  cửa sổ và giải phóng ảnh đã chọn. Khi cửa sổ mở, phím tắt của editor tạm tắt.

## Lưu ý / giới hạn

- Ảnh ghép là 1 ảnh nền phẳng (các ô không còn là layer riêng sau khi mở trong editor).
- Ảnh được đọc đồng bộ trên luồng UI khi chọn (ảnh rất lớn có thể khựng một chút).
- Chưa hỏi lưu khi mở ảnh ghép đè lên ảnh đang sửa chưa xuất (giống mở ảnh khác ở bản Rust).

## Test

`rust/core/src/photo/collage.rs` (`cargo test -p pbe-core collage`): `collage_cells_spacing` (mọi mẫu: trong
lề, không chồng, đủ khoảng cách), `collage_cover_and_clamp`, `render_collage_places_images_and_background`,
`render_collage_rounds_corners_and_fills_empty_cells`, `cell_at_finds_the_cell_under_the_point`.
Thủ công: `cargo run -p pbe-gui` → Sửa ảnh đơn → phím L → chọn 3 ảnh, mẫu 1 lớn + 2 nhỏ, bo góc 40, kéo /
lăn chuột trong ô, đổi màu nền → Mở trong Sửa ảnh đơn → xuất JPEG.
