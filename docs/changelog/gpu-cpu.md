# Changelog: GPU / CPU backend

> Hiện trạng logic: [../instruction/gpu-cpu.md](../instruction/gpu-cpu.md)

- 2026-10-03: Ghi lại hiện trạng ban đầu.
- 2026-10-06: Không có GPU / driver dùng được → tự chạy CPU, bỏ banner cảnh báo, disable mục GPU, lời nhắc driver thành "tùy chọn" — user mở app dùng được ngay, không cần cài gì.
- 2026-10-06: Tách file log cũ `docs/gpu-cpu.md` thành `instruction/` (hiện trạng) + `changelog/` (lịch sử) — theo rule mới trong CLAUDE.md.
- 2026-10-09 | Sửa | GPU / CPU backend: cập nhật `File:` sang Rust (`rust/core/src/backend.rs`) và ghi rõ nhánh GPU CuPy + dò thiết bị **chưa** port (Rust luôn chạy CPU) — vì mã Python đã bị xoá.
