# Changelog: Dev live reload & Build

> Hiện trạng logic: [../instruction/dev-build.md](../instruction/dev-build.md)

- 2026-10-03: Ghi lại hiện trạng ban đầu.
- 2026-10-03: Kèm ffmpeg.exe khi build (tools/fetch_ffmpeg.py, spec, build.bat); thêm `imageio-ffmpeg` vào
  requirements; devtools nhớ trang photo/video.
- 2026-10-03: Tách lệnh build theo hệ điều hành: `build.bat` → `build_windows.bat` (Windows 10/11),
  thêm `build_mac.sh` (.app + .dmg) và `dev.sh`; spec đa nền tảng (`BUNDLE`, `.icns`); sửa lỗi
  `tools\fetch_ffmpeg.py` bị hỏng thành form-feed trong `build.bat` cũ — người dùng muốn build riêng
  Mac / Windows và chạy được trên MacBook.
- 2026-10-03: Build chạy `lint --check` (Ruff + Pyright) trước pytest; `devtools` lưu geometry bằng
  `bytes(...toBase64().data())`, áp lại QSS qua `isinstance(app, QApplication)`.
- 2026-10-06: Thêm version tự tăng mỗi lần build (`version.json`, `tools/bump_build.py`, version resource
  trong `.exe`, tên installer/dmg theo version) và tự build bootloader PyInstaller từ source
  (`tools/build_bootloader.bat`) — `.exe` copy sang máy khác bị Windows Defender báo virus (báo nhầm do
  bootloader PyInstaller dùng chung), không có tiền mua chứng chỉ ký số.
- 2026-10-06: Tách file log cũ `docs/dev-build.md` thành `instruction/` (hiện trạng) + `changelog/` (lịch sử) — theo rule mới trong CLAUDE.md.
- 2026-10-06 | Thêm mới | Ký số self-signed cho bản build Windows: thêm `tools/sign_windows.ps1` (tạo chứng chỉ
  `CN=Photo Batch Editor` trong `Cert:\CurrentUser\My`, ký `PhotoBatchEditor.exe` + Setup.exe, xuất `.cer` cạnh
  `dist\` và `installer_output\`), `build_windows.bat` và `build_installer.bat` gọi script sau khi PyInstaller /
  Inno Setup, build dừng nếu ký lỗi — user máy khác cài `PhotoBatchEditor.cer` 1 lần là hết cảnh báo Windows
  Defender/SmartScreen, không mất tiền mua chứng chỉ thương mại.
- 2026-10-09 | Sửa | Dev & Build (Rust): xoá bản Python nên logic build/dev chuyển sang Rust — bỏ live-reload (`dev.bat` / `dev.sh` / `ui/devtools.py`), bỏ build PyInstaller/installer/ký số/bootloader và `tools/fetch_ffmpeg.py`, `tools/bump_build.py`; dev = `cargo run -p pbe-gui`, build = `build_rust_windows.bat` (`.exe`) và `build_rust_mac.sh` (`.app` qua cargo-bundle). `version.json` vẫn là nguồn version, đọc bằng `version.rs` — vì đã chuyển hẳn sang Rust, các script build Python không còn dùng.
- 2026-10-15 | Sửa | Build Windows: `build_rust_windows.bat` chép `models/` cạnh `photo-batch-editor.exe` (`target\release\models`); app tìm model ở `<thư mục exe>/models` và `.app/Contents/Resources/models` — trước đây exe chỉ tìm theo đường dẫn lúc compile nên máy khác không thấy model tách nền.
- 2026-10-16 | Thêm mới | Installer Windows (Inno Setup): thêm `installer.iss`, `build_rust_windows.bat` build luôn ra `installer_output\PhotoBatchEditor-Setup-<version>.exe` (Program Files, shortcut Start Menu / Desktop, gỡ cài đặt), bắt buộc có `ffmpeg\ffmpeg.exe` — người dùng muốn cài app như app bình thường thay vì copy exe + thư mục.
- 2026-10-16 | Sửa | Tìm tài nguyên bản cài (`paths::app_root`): ưu tiên thư mục cạnh exe / `../Resources` khi có `version.json` — trước đây dùng đường dẫn lúc compile nên máy khác không thấy preset dựng sẵn, version, model Super Resolution.
- 2026-10-16 | Sửa | Ẩn console bản release Windows: `windows_subsystem = "windows"` trong `main.rs` — exe release mở kèm cửa sổ console đen.
- 2026-10-16 | Sửa | Tự tăng version mỗi lần build: `build_rust_windows.bat` gọi `tools/bump_version.ps1` tăng `build` trong `version.json`, installer dùng version 4 số `X.Y.Z.build` (tên file + AppVersion) — người dùng muốn mỗi bản build có version riêng, cài đè được bản cũ.
- 2026-10-16 | Thêm mới | Nhúng icon + version resource vào exe Windows: `rust/gui/build.rs` (crate `winresource`) đọc `version.json`, nhúng `assets/app.ico` + FileVersion/ProductVersion — exe trước đây không icon, không version, dễ bị antivirus nghi.
- 2026-10-16 | Thêm mới | Ký số bản Windows: `tools/sign_windows.ps1` ký exe, Setup và uninstaller (qua `SignTool` của Inno) bằng SHA256 + timestamp; chứng chỉ mua (`PBE_SIGN_PFX` / `PBE_SIGN_THUMBPRINT`) hoặc tự ký mặc định; kèm `PhotoBatchEditor.cer` + `CaiChungChi.bat` (cài chứng chỉ vào Root/TrustedPublisher, gỡ Mark-of-the-Web khỏi Setup) cho máy khác — người dùng muốn đưa sang máy khác không bị Windows Defender SmartScreen bắt chọn "Run anyway".
