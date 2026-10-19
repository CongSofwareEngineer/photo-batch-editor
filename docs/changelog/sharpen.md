# Changelog: Tăng độ nét (Sharpen) trong Sửa ảnh đơn

> Hiện trạng logic: [../instruction/sharpen.md](../instruction/sharpen.md)

- 2026-10-15 | Thêm mới | Tăng độ nét (Unsharp Mask, Clarity): thêm Unsharp Mask (Amount / Radius / Threshold, mức Nhẹ / Vừa / Mạnh) trên độ sáng và thanh Clarity vào Sửa ảnh đơn — người dùng cần làm nét ảnh kiểu Photoshop, thanh "Độ nét" cũ không chỉnh được bán kính / ngưỡng.
- 2026-10-19 | Thêm mới | Làm nét bằng AI: `effects::ai_sharpen` (Real-ESRGAN ×4 theo ô 480 px rồi thu về đúng cỡ, trộn theo Mức độ AI), mục "Làm nét bằng AI" + menu Image, chạy nền có tiến độ / Huỷ, 1 bước undo; GUI bật feature `sr` của pbe-core — người dùng muốn tăng độ nét ảnh, Unsharp Mask chỉ tăng tương phản cạnh chứ không dựng lại chi tiết cho ảnh mềm / hơi mờ.
