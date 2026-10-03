# Mục lục log — Photo Batch Editor

> Mỗi chức năng có **một file log riêng** trong `docs/`. File này chỉ là mục lục.
> Quy tắc đầy đủ: `CLAUDE.md`.

| Chức năng | File log |
|---|---|
| Chỉnh nhiều ảnh (batch, chạy, kết quả, CLI) | [batch.md](batch.md) |
| 15 chỉnh sửa kiểu Camera Raw | [adjustments.md](adjustments.md) |
| Super Resolution | [super-resolution.md](super-resolution.md) |
| Image Size | [image-size.md](image-size.md) |
| GPU / CPU backend | [gpu-cpu.md](gpu-cpu.md) |
| Setting (preset) & trang Settings | [settings.md](settings.md) |
| Đăng nhập | [login.md](login.md) |
| Đa ngôn ngữ (EN / VI) | [i18n.md](i18n.md) |
| Dev live reload & Build (Windows / macOS) | [dev-build.md](dev-build.md) |
| Hỗ trợ macOS (MacBook) | [macos.md](macos.md) |
| Format / lint / type check code (Ruff + Pyright, "ESLint cho Python") | [code-format.md](code-format.md) |
| Sửa ảnh đơn (canvas, layer, công cụ, xuất, file dự án) | [photo-editor.md](photo-editor.md) |
| Cắt ghép layout (collage) | [collage.md](collage.md) |
| Sửa video (FFmpeg: cắt, tắt tiếng, chữ, nhạc, xuất) | [video-editor.md](video-editor.md) |

## Thêm chức năng mới

1. Tạo file `docs/<ten-chuc-nang>.md` theo mẫu bên dưới.
2. Thêm một dòng vào bảng trên.

## Mẫu file log của một chức năng

```markdown
# Log: <Tên chức năng>

- **Mục đích:** <vì sao cần>
- **File:** <các file/module bị ảnh hưởng>

## Logic chính

- <cách hoạt động, quyết định quan trọng>

## Lưu ý / giới hạn

- <nếu có>

## Test

<file test / cách kiểm tra thủ công>

## Lịch sử thay đổi

- YYYY-MM-DD: <sửa gì> — <vì sao>
```
