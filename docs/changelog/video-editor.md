# Changelog: Sửa video (Video editor)

> Hiện trạng logic: [../instruction/video-editor.md](../instruction/video-editor.md)

- 2026-10-03: Tạo file log trước khi code (kế hoạch).
- 2026-10-03: Hoàn thành GĐ4–GĐ5: xem trước, timeline, cắt / ghép / xóa đoạn giữa, tắt tiếng, chèn chữ, chèn nhạc,
  xuất MP4/MOV 1080p/720p chạy nền có % và Hủy; kèm ffmpeg.exe khi build.
- 2026-10-03: Build tách `build_windows.bat` / `build_mac.sh`; trên Mac ffmpeg là `ffmpeg/ffmpeg`, đóng gói
  vào `Contents/Frameworks/ffmpeg/`; font chữ mặc định theo hệ điều hành (`core.system.DEFAULT_FONT_FAMILY`).
- 2026-10-03: `TimelineWidget.scroll` đổi tên thành `view_start` (thuộc tính cũ đè lên method `QWidget.scroll()`,
  Pyright báo lỗi); kéo trên timeline khi chưa có project thì bỏ qua — xem code-format.md.
- 2026-10-06: Tách file log cũ `docs/video-editor.md` thành `instruction/` (hiện trạng) + `changelog/` (lịch sử) — theo rule mới trong CLAUDE.md.
- 2026-10-09 | Thêm mới | Bản Rust của logic này: xem [rust-port.md](../instruction/rust-port.md) (GĐ4 cho `core/`, GĐ5 cho GUI egui+wgpu) — bản Python giữ nguyên, không đổi hành vi.
- 2026-10-09 | Sửa | Sửa video: cập nhật `File:` / thứ tự tìm FFmpeg / Test sang Rust (`rust/core/src/video/`, `rust/gui/src/video.rs`, `rust/core/tests/video_export.rs`); vẽ chữ lên video và phát video có tiếng vẫn **chưa** port — vì mã Python đã bị xoá.
