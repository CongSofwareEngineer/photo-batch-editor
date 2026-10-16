# Dev & Build (Rust)

- **Mục đích:** chạy dev và đóng gói app (Rust) cho Windows 10/11 và macOS bằng lệnh riêng cho từng
  hệ điều hành.
- **File:** `rust/` (Cargo workspace), `build_rust_windows.bat`, `installer.iss`, `build_rust_mac.sh`,
  `tools/bump_version.ps1`, `tools/sign_windows.ps1`, `tools/CaiChungChi.bat`, `rust/gui/build.rs`,
  `version.json`, `rust/core/src/version.rs`, `rust/core/src/paths.rs` (`app_root`), `rust/gui/src/main.rs`,
  `assets/`, `models/`, `presets_builtin/`, `ffmpeg/`
- **Changelog:** [../changelog/dev-build.md](../changelog/dev-build.md)

## Logic chính

- **Dev:** `cd rust && cargo run -p pbe-gui` — không cần build đóng gói. Trước đây bản Python có
  live-reload (`dev.bat` / `dev.sh`) tự restart khi lưu file; cơ chế này **đã bị xoá** cùng bản Python
  (không cần nữa — `cargo run` dựng lại đủ nhanh).
- **Build Windows 10/11:** `build_rust_windows.bat` (một lệnh) →
  1. kiểm tra trước: có `ffmpeg\ffmpeg.exe` và `ISCC.exe` (Inno Setup 6, ở `%LOCALAPPDATA%\Programs` hoặc
     `Program Files`), thiếu thì dừng (chưa tăng version);
  2. `tools\bump_version.ps1` **tăng `build` trong `version.json`** → version 4 số `X.Y.Z.build`;
  3. `cargo build -p pbe-gui --release` → `rust\target\release\photo-batch-editor.exe`; `rust/gui/build.rs`
     (crate `winresource`, cần `rc.exe` của Windows SDK) nhúng **icon `assets/app.ico` + version resource**
     (FileVersion/ProductVersion = `X.Y.Z.build`) vào exe;
  4. `tools\sign_windows.ps1` **ký số exe**;
  5. chép `models/`, `presets_builtin/`, `ffmpeg/`, `version.json` cạnh exe (để chạy thử exe trực tiếp);
  6. compile `installer.iss` với `/DAppVersion=X.Y.Z.build /DSignTool /Spbesign=…` → Inno gọi
     `sign_windows.ps1` ký **Setup và uninstaller** → **`installer_output\PhotoBatchEditor-Setup-X.Y.Z.build.exe`**;
  7. xuất `installer_output\PhotoBatchEditor.cer` + chép `tools\CaiChungChi.bat` vào `installer_output\`.
- **Ký số (`tools/sign_windows.ps1`):** Authenticode SHA256 + timestamp DigiCert (lỗi timestamp → ký
  không timestamp), kiểm tra chữ ký `Valid` sau khi ký, lỗi thì build dừng. Chọn chứng chỉ:
  `PBE_SIGN_PFX` (+ `PBE_SIGN_PFX_PASSWORD`, file .pfx mua từ CA) → `PBE_SIGN_THUMBPRINT` (chứng chỉ trong
  `Cert:\CurrentUser\My`, vd. USB token) → mặc định **tự ký** `CN=Photo Batch Editor` (tạo 1 lần, 5 năm,
  RSA 3072, khoá riêng chỉ nằm trong store của user trên máy build; lần đầu tự thêm vào
  `CurrentUser\Root` để kiểm tra được — Windows hiện hộp thoại xác nhận, bấm Yes).
- **Máy khác (chứng chỉ tự ký):** gửi kèm `PhotoBatchEditor.cer` + `CaiChungChi.bat` cùng thư mục với
  Setup; chạy `CaiChungChi.bat` 1 lần (tự xin quyền Admin) → cài `.cer` vào `LocalMachine\Root` +
  `TrustedPublisher` và `Unblock-File` các `PhotoBatchEditor-Setup-*.exe` cạnh nó (gỡ Mark-of-the-Web
  → SmartScreen không hỏi "Run anyway" cho file đó; UAC hiện publisher "Photo Batch Editor" thay vì
  "Unknown").
- **Installer (`installer.iss`):** cài vào `Program Files\Photo Batch Editor` (hoặc chỉ cho user hiện tại
  nếu chọn ở hộp thoại đầu), shortcut Start Menu + tuỳ chọn Desktop (icon `assets\app.ico`), mục gỡ cài đặt
  trong Apps & features, chạy app ở bước cuối. Gồm exe, `version.json`, `app.ico`, `presets_builtin\`,
  `models\`, `ffmpeg\ffmpeg.exe`. `AppId` cố định (`DC4CD855-…`) để bản mới cài đè/nâng cấp bản cũ —
  **không được đổi**. Dữ liệu người dùng ở `%APPDATA%\PhotoBatchEditor` không bị gỡ khi uninstall.
- **Tìm tài nguyên lúc chạy (`paths::app_root`):** `PBE_APP_ROOT` → thư mục cạnh exe (hoặc
  `../Resources` của `.app`) **nếu có `version.json`** (bản cài) → gốc dự án lúc compile (`cargo run`).
  Trước đây chỉ dùng đường dẫn lúc compile nên máy khác không thấy preset dựng sẵn / version / model SR.
- **Console:** `main.rs` có `windows_subsystem = "windows"` cho bản release (không mở cửa sổ console
  đen kèm app); bản debug (`cargo run`) vẫn có console để xem log.
- **Build macOS:** `build_rust_mac.sh` — chi tiết (tăng version, ký, notarize, `.dmg`) ở
  [macos.md](macos.md).
- **Version:** `version.json` = `{"version": "X.Y.Z", "build": N}` ở gốc dự án. Rust đọc bằng
  `version::read_version()` (`resource_path(["version.json"])`). **Mỗi lần chạy script build (Windows hoặc
  Mac) tăng `build` thêm 1** (`tools/bump_version.ps1`; Mac tăng bằng `sed` trong script); `build` không
  bao giờ reset nên `X.Y.Z.build` luôn tăng → installer cài đè được bản cũ. `X.Y.Z` sửa bằng tay khi phát
  hành. `-Print` chỉ in version hiện tại.
- Rust không build chéo: `.exe` build trên Windows, `.app` build trên Mac.

## Lưu ý / giới hạn

- Bản đóng gói macOS hiện ký **ad-hoc** (chưa có Developer ID); lần mở đầu macOS chặn → chuột phải ›
  Open, hoặc `xattr -dr com.apple.quarantine`.
- **SmartScreen dựa trên uy tín (reputation), không chỉ chữ ký.** Chứng chỉ tự ký chỉ được tin ở máy đã
  chạy `CaiChungChi.bat`; máy chưa cài vẫn thấy "Windows protected your PC". Muốn **mọi** máy không hỏi
  thì cần chứng chỉ code-signing của CA (OV/EV, hoặc Microsoft Trusted/Artifact Signing) → đặt
  `PBE_SIGN_PFX` / `PBE_SIGN_THUMBPRINT`, không cần sửa script; chứng chỉ mới cũng cần thời gian/lượt tải
  để có uy tín SmartScreen. Copy Setup qua USB/LAN (không qua trình duyệt / Zalo / Drive) thì file không
  có Mark-of-the-Web nên SmartScreen không hỏi.
- Cài Windows lại / mất store user trên máy build = mất chứng chỉ tự ký → script tạo cái mới, các máy
  khác phải chạy lại `CaiChungChi.bat`. Muốn giữ: export `.pfx` từ `certmgr.msc` rồi dùng `PBE_SIGN_PFX`.
- Build exe cần Windows SDK (`rc.exe`, có sẵn khi cài Rust MSVC Build Tools); thiếu thì build.rs chỉ
  cảnh báo, exe không có icon/version. Installer chỉ tiếng Anh (Inno Setup 6 không kèm file ngôn ngữ
  tiếng Việt).
- Cần Inno Setup 6: `winget install JRSoftware.InnoSetup`.
- `version.json` được Rust đọc lúc chạy và bị script build sửa mỗi lần build; nhớ commit file này.
- FFmpeg phải có sẵn ở `ffmpeg/` (hoặc `PBE_FFMPEG` / `PATH`); bước tự tải ffmpeg
  (`tools/fetch_ffmpeg.py`) của bản Python đã bị xoá.

## Test

- `cd rust && cargo test` — core + phần CPU của GUI.
- `cd rust && cargo test --features sr` — thêm test Super Resolution (ONNX).
- Chạy thử GUI: `cd rust && cargo run -p pbe-gui`.
- Windows: `build_rust_windows.bat` → `version.json` tăng `build`; exe › Properties › Details hiện
  `X.Y.Z.build` + icon; `Get-AuthenticodeSignature <exe hoặc Setup>` = `Valid`; cài thử
  `installer_output\PhotoBatchEditor-Setup-<version>.exe` (hoặc im lặng:
  `Setup.exe /VERYSILENT /CURRENTUSER /DIR=<thư mục tạm>`), mở app từ Start Menu, rồi gỡ trong Apps & features.
- Mac: `./build_rust_mac.sh` → mở `rust/target/release/bundle/osx/Photo Batch Editor.app`.
