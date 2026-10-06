# Hỗ trợ macOS (MacBook)

- **Mục đích:** app chạy được trên MacBook (Apple Silicon M1–M4 và Intel), cả từ source lẫn bản
  đóng gói `.app` / `.dmg`. Windows 10/11 giữ nguyên như cũ.
- **File:** `requirements.txt`, `requirements-cuda.txt`, `core/system.py` (mới), `core/io_utils.py`,
  `core/paths.py`, `core/video/project.py`, `ui/photo/document.py`, `ui/photo/panels.py`, `ui/theme.qss`,
  `ui/file_list.py`, `ui/results_view.py`, `ui/image_viewer.py`, `core/i18n_vi.py`,
  `PhotoBatchEditor.spec`, `tools/fetch_ffmpeg.py`, `tools/make_icns.py` (mới), `assets/app.icns`,
  `build_mac.sh`, `dev.sh`
- **Changelog:** [../changelog/macos.md](../changelog/macos.md)

## Logic chính

- **Thư viện:** trên macOS không có NVIDIA/CUDA → `requirements.txt` dùng environment marker:
  `cupy-cuda12x` và `onnxruntime-gpu` chỉ cài khi `sys_platform != "darwin"`; trên Mac cài
  `onnxruntime` thường (CPU). `requirements-cuda.txt` cũng có marker nên `pip install -r
  requirements-dev.txt` chạy được trên cả hai hệ điều hành.
- **GPU:** Mac luôn chạy CPU (NumPy/OpenCV). `detect_gpu()` báo "No NVIDIA GPU found" như máy
  Windows không có card NVIDIA — không cần sửa logic backend.
- **`core/system.py`:** `IS_WINDOWS`, `IS_MAC`, `DEFAULT_FONT_FAMILY` (Segoe UI / Helvetica Neue /
  DejaVu Sans) — font mặc định cho layer chữ ảnh và chữ video.
- **Mở thư mục / hiện file:** Windows `explorer /select,` · macOS `open -R` (Finder) và `open` ·
  Linux `xdg-open`. Nhãn menu "Show in File Explorer" → "Show in Finder" trên Mac.
- **Thư mục cache:** macOS `~/Library/Caches/PhotoBatchEditor`; dữ liệu người dùng vẫn theo
  `QStandardPaths.AppDataLocation` (`~/Library/Application Support/...`).
- **Phím tắt:** Qt tự đổi `Ctrl+…` thành `⌘ Cmd+…` trên Mac.
- **Build:** `./build_mac.sh` (chạy TRÊN Mac) → test → ffmpeg (`ffmpeg/ffmpeg` từ `imageio-ffmpeg`
  bản Mac) → PyInstaller → `dist/PhotoBatchEditor.app` → `dist/PhotoBatchEditor-<arch>.dmg`.
  Spec dùng `BUNDLE(...)`, icon `assets/app.icns`, ffmpeg đưa vào dạng *binary* (vào
  `Contents/Frameworks/ffmpeg/ffmpeg`, `app_root()` = `sys._MEIPASS` trỏ đúng chỗ này).

## Lưu ý / giới hạn

- PyInstaller không build chéo: bản Mac phải build trên Mac, bản Windows build trên Windows.
- Kiến trúc theo Python dùng để build: build trên Mac M1–M4 → chỉ chạy Mac Apple Silicon; build trên
  Mac Intel → chạy Mac Intel (và Apple Silicon qua Rosetta 2). Không làm universal2 vì wheel
  numpy/opencv không có bản universal2.
- App chưa ký Apple Developer ID / notarize → lần đầu mở: chuột phải › Open, hoặc
  `xattr -dr com.apple.quarantine /Applications/PhotoBatchEditor.app`. Build tự ký ad-hoc.
- Super Resolution trên Mac chạy CPU (chưa dùng CoreML).
- Máy dev là Windows: phần Mac chỉ kiểm tra bằng test giả lập `sys.platform` — cần chạy thử
  `./build_mac.sh` trên MacBook thật.

## Test

- `tests/test_system.py`: requirements markers, mở Finder (`open -R`) khi giả lập darwin, font mặc
  định, spec/script build tồn tại.
- Thủ công trên Mac: `./dev.sh` (chạy từ source), `./build_mac.sh`, mở `.app`, chạy batch CPU, Sửa
  ảnh/Sửa video (xuất MP4), "Show in Finder".
