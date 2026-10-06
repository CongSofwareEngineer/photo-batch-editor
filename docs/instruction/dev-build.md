# Dev live reload & Build

- **Mục đích:** chạy dev không cần build, và đóng gói app cho Windows 10/11 (.exe + installer) và
  macOS (.app + .dmg) bằng lệnh riêng cho từng hệ điều hành.
- **File:** `dev.py`, `dev.bat`, `dev.sh`, `ui/devtools.py`, `ui/theme.qss`, `main.py`, `build_windows.bat`,
  `build_mac.sh`, `build_installer.bat`, `PhotoBatchEditor.spec`, `installer.iss`, `tools/fetch_ffmpeg.py`,
  `tools/make_icns.py`, `tools/bump_build.py`,
  `tools/build_bootloader.bat`, `version.json`, `core/version.py`, `assets/app.ico`, `assets/app.icns`, `.gitattributes`
- **Changelog:** [../changelog/dev-build.md](../changelog/dev-build.md)

## Logic chính

- **Dev:** `dev.bat` (Windows) / `./dev.sh` (Mac) chạy từ source (`venv`). Lưu `.py` trong `core/`,
  `ui/` hoặc `main.py` → app tự restart (~2 s), giữ trang và vị trí cửa sổ. Lưu `ui/theme.qss` → áp
  style ngay. Thanh tiêu đề hiện `[DEV — reloaded: <files>]`. App crash → sửa file, lưu là chạy lại.
- **Build Windows 10/11:** `build_windows.bat` (`--skip-tests` để bỏ test), chạy trên Windows:
  pytest → `tools/fetch_ffmpeg.py` → bootloader tự build → tăng số build → PyInstaller `--onedir` → `dist\PhotoBatchEditor\PhotoBatchEditor.exe`
  → installer nếu có Inno Setup 6. Một bản build chạy được cả Windows 10 và 11 (64-bit) — không cần
  build riêng từng bản.
- `build_installer.bat`: đóng gói thành `installer_output\PhotoBatchEditor-Setup-<version>.exe`
  (đọc version bằng `tools/bump_build.py --print`, truyền `/DAppVersion=` cho `installer.iss`).
- **Version mỗi lần build:** `version.json` = `{"version": "X.Y.Z", "build": N}`. `X.Y.Z` sửa tay khi
  phát hành; `tools/bump_build.py` (gọi bởi `build_windows.bat` / `build_mac.sh`, sau test, trước
  PyInstaller) tăng `build` +1 và sinh `build/version_info.txt` (định dạng `VSVersionInfo`). Spec đọc
  `version.json` (qua `SPECPATH`), nhúng `version_info.txt` vào `.exe` (Properties › Details hiện
  `X.Y.Z.N`) và kèm `version.json` vào bundle; Mac: `CFBundleShortVersionString` = `X.Y.Z`,
  `CFBundleVersion` = `N`, tên dmg `PhotoBatchEditor-X.Y.Z.N-<arch>.dmg`. Lúc chạy `core/version.py`
  (`read_version()`) đọc file → `QApplication.setApplicationVersion` trong `ui/main_window.py`.
- **Giảm cảnh báo Windows Defender (không cần mua chứng chỉ):** `tools\build_bootloader.bat` build lại
  bootloader PyInstaller từ source (`PYINSTALLER_COMPILE_BOOTLOADER=1` +
  `pip install --no-binary pyinstaller pyinstaller==<bản đang dùng>`). Lý do: bootloader có sẵn trên PyPI
  giống hệt mọi app PyInstaller (kể cả malware) nên Defender hay báo nhầm (`Trojan:Win32/Wacatac`…).
  Cần Microsoft C++ Build Tools (miễn phí, kiểm tra bằng `vswhere`). Chỉ build 1 lần cho mỗi phiên bản
  PyInstaller (marker `venv\custom_bootloader.txt`; `--force` để build lại). Thiếu Build Tools / build
  lỗi → cài lại PyInstaller bản có sẵn, `build_windows.bat` in WARNING và vẫn build tiếp.
  Spec giữ `upx=False` và `--onedir` (UPX / onefile làm tăng báo nhầm).
- **Build macOS:** `./build_mac.sh` (`--skip-tests`), chạy trên Mac: tạo `venv/` bằng python3.12/3.11
  nếu chưa có → pytest (offscreen) → ffmpeg → `tools/make_icns.py` nếu thiếu icon → PyInstaller
  → `dist/PhotoBatchEditor.app` (ký ad-hoc `codesign -s -`) → `dist/PhotoBatchEditor-<version>-<arm64|x86_64>.dmg`
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
- Mỗi lần build thay đổi `version.json` (số build) → nhớ commit file này. Muốn đổi bản phát hành:
  sửa `"version"` (có thể đặt lại `"build"` về 0).
- Không có chứng chỉ ký số nên SmartScreen ("Windows protected your PC") vẫn có thể hiện với app
  mới — người dùng bấm More info › Run anyway. Bootloader tự build chỉ giảm, không loại bỏ hoàn toàn
  báo nhầm; nếu vẫn bị báo: gửi file tại https://www.microsoft.com/wdsi/filesubmission (miễn phí).
- Không dùng Python heredoc / chuỗi có `\f` để sửa `.bat`: `build.bat` cũ từng bị `tools\fetch_ffmpeg.py`
  thành ký tự form-feed (`\f`) → bước ffmpeg hỏng.

## Test

- `tests/test_version.py` (đọc/ghi `version.json`, file `version_info.txt`, các script build có gọi
  bump / bootloader, `tools/build_bootloader.bat` giữ CRLF).
- `tests/test_system.py` (requirements theo OS, file build, LF cho `.sh`, không có `\f` trong `.bat`).
- Windows: `build_windows.bat --skip-tests` → chạy `dist\PhotoBatchEditor\PhotoBatchEditor.exe`.
- Mac: `./build_mac.sh` → mở `dist/PhotoBatchEditor.app` hoặc `.dmg`.
