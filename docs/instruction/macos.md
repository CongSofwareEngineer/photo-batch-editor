# Hỗ trợ macOS (MacBook)

- **Mục đích:** app chạy được trên MacBook (Apple Silicon M1–M4 và Intel), cả từ source lẫn bản đóng
  gói `.app`. Windows 10/11 chạy như cũ.
- **File:** `rust/core/src/system.rs`, `rust/core/src/paths.rs`, `rust/core/src/io_utils.rs`,
  `rust/core/src/video/ffmpeg.rs`, `rust/gui/src/theme.rs`, `assets/app.icns`, `build_rust_mac.sh`
- **Changelog:** [../changelog/macos.md](../changelog/macos.md)
- **Ghi chú:** mã Python (`core/system.py`, `ui/theme.qss`, `PhotoBatchEditor.spec`,
  `tools/fetch_ffmpeg.py`, `tools/make_icns.py`, `build_mac.sh`…) đã xoá; bản triển khai hiện tại là Rust.

## Logic chính

- **Khác biệt hệ điều hành** nằm trong `rust/core/src/system.rs` (`IS_WINDOWS` / `IS_MAC`) — UI nằm ở
  `rust/gui/`, không import vào `core`.
- **GPU:** Mac không có NVIDIA/CUDA → luôn chạy CPU. Nhánh GPU CuPy của bản Python không port, nên điều
  này đúng cho mọi nền tảng.
- **Mở thư mục / hiện file:** Windows `explorer /select,` · macOS `open -R` (Finder) và `open` · Linux
  `xdg-open`.
- **Thư mục dữ liệu / cache:** `paths::app_data_dir()` = `~/Library/Application Support/PhotoBatchEditor`
  trên macOS (khớp `QStandardPaths.AppDataLocation` cũ), cache ở `~/Library/Caches/PhotoBatchEditor`.
- **Phím tắt:** egui dùng `⌘ Cmd` trên Mac (viết `Ctrl+…` trong code).
- **Build:** `./build_rust_mac.sh` (chạy TRÊN Mac) → `cargo build -p pbe-gui --release` → `cargo bundle
  --release` → `rust/target/release/bundle/osx/Photo Batch Editor.app`, chép `assets/`, `models/`,
  `ffmpeg/` vào app.

## Lưu ý / giới hạn

- Rust không build chéo: bản Mac build trên Mac, bản Windows build trên Windows.
- Kiến trúc theo máy build: build trên M1–M4 ra `arm64`; build trên Intel ra `x86_64` (chạy được trên
  Apple Silicon qua Rosetta 2). Không làm universal2.
- App chưa ký Apple Developer ID / notarize → lần đầu mở: chuột phải › Open, hoặc
  `xattr -dr com.apple.quarantine "Photo Batch Editor.app"`. Script build ký ad-hoc.
- Super Resolution trên Mac chạy CPU (chưa dùng CoreML).

## Test

- `rust/core/src/system.rs` (`#[cfg(test)]`) — bản port test `sys.platform` / đường dẫn theo OS cũ.
- Thủ công trên Mac: `cd rust && cargo run -p pbe-gui` (chạy từ source), `./build_rust_mac.sh`, mở `.app`,
  chạy batch CPU, Sửa ảnh/Sửa video (xuất MP4), "Show in Finder".
