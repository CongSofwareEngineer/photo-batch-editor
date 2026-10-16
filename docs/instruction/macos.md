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
- **Build:** `./build_rust_mac.sh` (chạy TRÊN Mac, cần `cargo install cargo-bundle` và `ffmpeg/ffmpeg`
  bản macOS) →
  1. **tăng `build` trong `version.json`** (giống Windows, xem [dev-build.md](dev-build.md));
  2. `cargo build -p pbe-gui --release` → `cargo bundle -p pbe-gui --release` →
     `rust/target/release/bundle/osx/Photo Batch Editor.app`;
  3. chép `version.json`, `models/`, `presets_builtin/`, `ffmpeg/ffmpeg`, README vào
     `Contents/Resources` (app tìm tài nguyên ở đó nhờ `version.json`, xem `paths::app_root`); ghi
     `CFBundleShortVersionString = X.Y.Z`, `CFBundleVersion = build` vào `Info.plist`;
  4. ký: `ffmpeg` trước rồi `.app`. Có `PBE_MAC_SIGN_IDENTITY` ("Developer ID Application: … (TEAMID)")
     → ký hardened runtime + timestamp; không có → ký ad-hoc. `codesign --verify --strict` sau khi ký;
  5. tạo **`installer_output/PhotoBatchEditor-X.Y.Z.build.dmg`** (app + lối tắt Applications để kéo thả);
     có Developer ID thì ký `.dmg`, có thêm `PBE_NOTARY_PROFILE` (tạo 1 lần bằng
     `xcrun notarytool store-credentials <tên>`) thì **notarize** + `stapler staple` `.dmg` và `.app`.
- `rust/gui/Cargo.toml` `[package.metadata.bundle]` không khai báo `resources` nữa (trước có
  `README.txt` không tồn tại); tài nguyên do script chép.

## Lưu ý / giới hạn

- Rust không build chéo: bản Mac build trên Mac, bản Windows build trên Windows.
- Kiến trúc theo máy build: build trên M1–M4 ra `arm64`; build trên Intel ra `x86_64` (chạy được trên
  Apple Silicon qua Rosetta 2). Không làm universal2.
- Gatekeeper chỉ cho mở không hỏi khi app ký **Developer ID + notarize** (tài khoản Apple Developer
  $99/năm); không có cách miễn phí (chứng chỉ tự ký không giúp gì trên Mac). Bản ký ad-hoc: máy khác
  lần đầu mở → chuột phải › Open (macOS 15: System Settings › Privacy & Security › Open Anyway), hoặc
  `xattr -dr com.apple.quarantine "/Applications/Photo Batch Editor.app"`. Copy qua USB/AirDrop từ máy
  mình vẫn có thể bị gắn quarantine.
- Chưa có file entitlements; nếu sau này app nạp dylib không ký (vd. ONNX Runtime dạng dynamic) thì bản
  hardened runtime cần thêm entitlement `com.apple.security.cs.disable-library-validation`. Hiện ONNX
  Runtime link tĩnh (`ort` `download-binaries`).
- Script build Mac **chưa chạy thử trên Mac thật** (viết trên Windows, mới kiểm tra `bash -n`).
- Super Resolution trên Mac chạy CPU (chưa dùng CoreML).

## Test

- `rust/core/src/system.rs` (`#[cfg(test)]`) — bản port test `sys.platform` / đường dẫn theo OS cũ.
- Thủ công trên Mac: `cd rust && cargo run -p pbe-gui` (chạy từ source), `./build_rust_mac.sh`, mở `.app`,
  chạy batch CPU, Sửa ảnh/Sửa video (xuất MP4), "Show in Finder".
- Sau build: `version.json` tăng `build`; `codesign -dv --verbose=2 "<app>"` (xem Authority / TeamIdentifier);
  `spctl --assess -vv "<app>"` = `accepted … Notarized Developer ID` khi đã notarize; mở `.dmg`, kéo app
  vào Applications, Get Info hiện đúng version.
