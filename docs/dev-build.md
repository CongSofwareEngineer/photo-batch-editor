# Log: Dev live reload & Build

- **Mục đích:** chạy dev không cần build, và đóng gói app cho Windows 10/11 (.exe + installer) và
  macOS (.app + .dmg) bằng lệnh riêng cho từng hệ điều hành.
- **File:** `dev.py`, `dev.bat`, `dev.sh`, `ui/devtools.py`, `ui/theme.qss`, `main.py`, `build_windows.bat`,
  `build_mac.sh`, `build_installer.bat`, `PhotoBatchEditor.spec`, `installer.iss`, `tools/fetch_ffmpeg.py`,
  `tools/make_icns.py`, `assets/app.ico`, `assets/app.icns`, `.gitattributes`

## Logic chính

- **Dev:** `dev.bat` (Windows) / `./dev.sh` (Mac) chạy từ source (`venv`). Lưu `.py` trong `core/`,
  `ui/` hoặc `main.py` → app tự restart (~2 s), giữ trang và vị trí cửa sổ. Lưu `ui/theme.qss` → áp
  style ngay. Thanh tiêu đề hiện `[DEV — reloaded: <files>]`. App crash → sửa file, lưu là chạy lại.
- **Build Windows 10/11:** `build_windows.bat` (`--skip-tests` để bỏ test), chạy trên Windows:
  pytest → `tools/fetch_ffmpeg.py` → PyInstaller `--onedir` → `dist\PhotoBatchEditor\PhotoBatchEditor.exe`
  → installer nếu có Inno Setup 6. Một bản build chạy được cả Windows 10 và 11 (64-bit) — không cần
  build riêng từng bản.
- `build_installer.bat`: đóng gói thành `installer_output\PhotoBatchEditor-Setup-1.0.0.exe`.
- **Build macOS:** `./build_mac.sh` (`--skip-tests`), chạy trên Mac: tạo `venv/` bằng python3.12/3.11
  nếu chưa có → pytest (offscreen) → ffmpeg → `tools/make_icns.py` nếu thiếu icon → PyInstaller
  → `dist/PhotoBatchEditor.app` (ký ad-hoc `codesign -s -`) → `dist/PhotoBatchEditor-1.0.0-<arm64|x86_64>.dmg`
  (`hdiutil`, có lối tắt Applications để kéo thả). Chi tiết Mac: [macos.md](macos.md).
- **Spec chung** `PhotoBatchEditor.spec` rẽ nhánh theo `sys.platform`: Windows kèm CuPy + DLL CUDA,
  icon `.ico`, ffmpeg.exe dạng data; macOS bỏ CUDA, icon `.icns`, ffmpeg dạng binary, thêm `BUNDLE`
  (bundle id `com.photobatcheditor.app`, macOS ≥ 11, hỗ trợ Retina / dark mode).
- FFmpeg (Sửa video): build chạy `tools/fetch_ffmpeg.py` → copy ffmpeg của gói pip `imageio-ffmpeg`
  vào `ffmpeg/ffmpeg.exe` (Mac: `ffmpeg/ffmpeg`, chmod 755) + `README.txt` license; spec exclude gói
  `imageio_ffmpeg` (không kèm 2 lần). Plugin multimedia của PySide6 (xem trước video) do hook PyInstaller tự gom.
- Dev reload giữ cả trang Sửa ảnh đơn / Sửa video (không giữ ảnh/video đang mở).

## Lưu ý / giới hạn

- Thư mục build Windows khoảng 1.5–3 GB vì kèm thư viện CUDA/cuDNN; bản Mac nhỏ hơn nhiều (không CUDA).
- `main.py` phải gọi `multiprocessing.freeze_support()` (process pool trong bản đóng gói).
- PyInstaller không build chéo: `.exe` build trên Windows, `.app` build trên Mac.
- `.sh` phải giữ xuống dòng LF (`.gitattributes`; `tests/test_system.py` kiểm tra). Copy sang Mac
  nếu mất quyền chạy: `chmod +x *.sh` hoặc chạy `bash build_mac.sh`.
- Không dùng Python heredoc / chuỗi có `\f` để sửa `.bat`: `build.bat` cũ từng bị `tools\fetch_ffmpeg.py`
  thành ký tự form-feed (`\f`) → bước ffmpeg hỏng.

## Test

- `tests/test_system.py` (requirements theo OS, file build, LF cho `.sh`, không có `\f` trong `.bat`).
- Windows: `build_windows.bat --skip-tests` → chạy `dist\PhotoBatchEditor\PhotoBatchEditor.exe`.
- Mac: `./build_mac.sh` → mở `dist/PhotoBatchEditor.app` hoặc `.dmg`.

## Lịch sử thay đổi

- 2026-10-03: Ghi lại hiện trạng ban đầu.
- 2026-10-03: Kèm ffmpeg.exe khi build (tools/fetch_ffmpeg.py, spec, build.bat); thêm `imageio-ffmpeg` vào
  requirements; devtools nhớ trang photo/video.
- 2026-10-03: Tách lệnh build theo hệ điều hành: `build.bat` → `build_windows.bat` (Windows 10/11),
  thêm `build_mac.sh` (.app + .dmg) và `dev.sh`; spec đa nền tảng (`BUNDLE`, `.icns`); sửa lỗi
  `tools\fetch_ffmpeg.py` bị hỏng thành form-feed trong `build.bat` cũ — người dùng muốn build riêng
  Mac / Windows và chạy được trên MacBook.
- 2026-10-03: Build chạy `lint --check` (Ruff + Pyright) trước pytest; `devtools` lưu geometry bằng
  `bytes(...toBase64().data())`, áp lại QSS qua `isinstance(app, QApplication)`.
