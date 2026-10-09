# Dev & Build (Rust)

- **Mục đích:** chạy dev và đóng gói app (Rust) cho Windows 10/11 và macOS bằng lệnh riêng cho từng
  hệ điều hành.
- **File:** `rust/` (Cargo workspace), `build_rust_windows.bat`, `build_rust_mac.sh`, `version.json`,
  `rust/core/src/version.rs`, `assets/`, `models/`, `ffmpeg/`
- **Changelog:** [../changelog/dev-build.md](../changelog/dev-build.md)

## Logic chính

- **Dev:** `cd rust && cargo run -p pbe-gui` — không cần build đóng gói. Trước đây bản Python có
  live-reload (`dev.bat` / `dev.sh`) tự restart khi lưu file; cơ chế này **đã bị xoá** cùng bản Python
  (không cần nữa — `cargo run` dựng lại đủ nhanh).
- **Build Windows 10/11:** `build_rust_windows.bat` → `cargo build -p pbe-gui --release` → ra
  `rust\target\release\photo-batch-editor.exe`.
- **Build macOS:** `build_rust_mac.sh` → `cargo build -p pbe-gui --release` → `cargo bundle --release`,
  chép `assets/`, `models/`, `ffmpeg/` và README vào
  `rust/target/release/bundle/osx/Photo Batch Editor.app`.
- **Version:** `version.json` = `{"version": "X.Y.Z", "build": N}` ở gốc dự án. Rust đọc bằng
  `version::read_version()` (`resource_path(["version.json"])`). Sửa `version` bằng tay khi phát hành;
  (script build Rust hiện **chưa** tự tăng `build` — tool `tools/bump_build.py` đã bị xoá cùng bản Python).
- Rust không build chéo: `.exe` build trên Windows, `.app` build trên Mac.

## Lưu ý / giới hạn

- Bản đóng gói macOS hiện ký **ad-hoc** (chưa có Developer ID); lần mở đầu macOS chặn → chuột phải ›
  Open, hoặc `xattr -dr com.apple.quarantine`.
- Phần ký số Windows và installer (Inno Setup) của bản Python (`tools/sign_windows.ps1`,
  `build_installer.bat`, `installer.iss`) **đã bị xoá**; bản Rust hiện chỉ xuất `.exe`/`.app`, chưa có
  installer hay ký số.
- `version.json` được Rust đọc lúc chạy; nhớ commit file này mỗi khi đổi bản phát hành.
- FFmpeg phải có sẵn ở `ffmpeg/` (hoặc `PBE_FFMPEG` / `PATH`); bước tự tải ffmpeg
  (`tools/fetch_ffmpeg.py`) của bản Python đã bị xoá.

## Test

- `cd rust && cargo test` — core + phần CPU của GUI.
- `cd rust && cargo test --features sr` — thêm test Super Resolution (ONNX).
- Chạy thử GUI: `cd rust && cargo run -p pbe-gui`.
- Windows: `build_rust_windows.bat` → chạy `rust\target\release\photo-batch-editor.exe`.
- Mac: `./build_rust_mac.sh` → mở `rust/target/release/bundle/osx/Photo Batch Editor.app`.
