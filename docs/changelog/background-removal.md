# Changelog: Tách nền (xoá phông, thay nền)

> Hiện trạng logic: [../instruction/background-removal.md](../instruction/background-removal.md)

- 2026-10-15 | Thêm mới | Tách nền / thay nền (Sửa ảnh đơn): model MODNet (ONNX, `models/modnet.onnx`) chạy ở thread nền, matte lưu riêng đi theo crop / xoay / lật, panel "Tách nền" với nền Trong suốt / 5 màu ảnh thẻ / màu tuỳ chọn / Ảnh…, Dịch viền, Làm mềm viền, Khử màu nền ở viền, Chạy lại, Khôi phục nền gốc; menu Image › Tách nền; canvas hiện ô vuông chỗ trong suốt — người dùng cần tách người khỏi phông (ảnh thẻ đổi nền, ghép nền / áo khác).
- 2026-10-15 | Sửa | Xem & sửa vùng chọn trước khi tách: AI xong không áp dụng ngay mà hiện vùng sẽ xoá phủ đỏ + viền cyan (hoặc xem Kết quả), cọ + Thêm / − Bớt (Alt đảo, X đổi, [ ] cỡ cọ, độ cứng), Ctrl+Z từng nét, Bỏ mọi nét tô, Áp dụng (Enter) / Huỷ (Esc); thêm nút "Sửa vùng chọn" sau khi đã tách — người dùng muốn thấy vùng AI chọn và tự chỉnh chỗ chưa hợp lý rồi mới tách thật.
