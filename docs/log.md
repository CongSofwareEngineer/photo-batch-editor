# Mục lục docs — Photo Batch Editor

> Mỗi logic có **một file hiện trạng** trong `docs/instruction/` và **một file lịch sử cùng tên** trong
> `docs/changelog/`. File này chỉ là mục lục. Quy tắc đầy đủ và mẫu file: `CLAUDE.md`.

| Logic | Hiện trạng (đọc trước khi làm) | Lịch sử thay đổi |
|---|---|---|
| Chỉnh nhiều ảnh (batch, chạy, kết quả, CLI) | [instruction/batch.md](instruction/batch.md) | [changelog/batch.md](changelog/batch.md) |
| 15 chỉnh sửa kiểu Camera Raw | [instruction/adjustments.md](instruction/adjustments.md) | [changelog/adjustments.md](changelog/adjustments.md) |
| Super Resolution | [instruction/super-resolution.md](instruction/super-resolution.md) | [changelog/super-resolution.md](changelog/super-resolution.md) |
| Image Size | [instruction/image-size.md](instruction/image-size.md) | [changelog/image-size.md](changelog/image-size.md) |
| GPU / CPU backend | [instruction/gpu-cpu.md](instruction/gpu-cpu.md) | [changelog/gpu-cpu.md](changelog/gpu-cpu.md) |
| Setting (preset) & trang Settings | [instruction/settings.md](instruction/settings.md) | [changelog/settings.md](changelog/settings.md) |
| Đăng nhập | [instruction/login.md](instruction/login.md) | [changelog/login.md](changelog/login.md) |
| Đa ngôn ngữ (EN / VI) | [instruction/i18n.md](instruction/i18n.md) | [changelog/i18n.md](changelog/i18n.md) |
| Dev live reload & Build (Windows / macOS) | [instruction/dev-build.md](instruction/dev-build.md) | [changelog/dev-build.md](changelog/dev-build.md) |
| Hỗ trợ macOS (MacBook) | [instruction/macos.md](instruction/macos.md) | [changelog/macos.md](changelog/macos.md) |
| Format / lint / type check code (Ruff + Pyright, "ESLint cho Python") | [instruction/code-format.md](instruction/code-format.md) | [changelog/code-format.md](changelog/code-format.md) |
| Sửa ảnh đơn (canvas, layer, công cụ, xuất, file dự án) | [instruction/photo-editor.md](instruction/photo-editor.md) | [changelog/photo-editor.md](changelog/photo-editor.md) |
| Cắt ghép layout (collage) | [instruction/collage.md](instruction/collage.md) | [changelog/collage.md](changelog/collage.md) |
| Sửa video (FFmpeg: cắt, tắt tiếng, chữ, nhạc, xuất) | [instruction/video-editor.md](instruction/video-editor.md) | [changelog/video-editor.md](changelog/video-editor.md) |

## Thêm logic mới

1. Tạo `docs/instruction/<ten-logic>.md` và `docs/changelog/<ten-logic>.md` (cùng tên) theo mẫu trong `CLAUDE.md`.
2. Thêm một dòng vào bảng trên.
