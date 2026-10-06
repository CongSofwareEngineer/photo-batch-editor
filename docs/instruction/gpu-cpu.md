# GPU / CPU backend

- **Mục đích:** tăng tốc bằng NVIDIA GPU, tự chuyển sang CPU khi không dùng được GPU.
- **File:** `core/backend.py`, `ui/device_status.py`, `ui/prepare_view.py`, `ui/main_window.py`,
  `requirements-cuda.txt`
- **Changelog:** [../changelog/gpu-cpu.md](../changelog/gpu-cpu.md)

## Logic chính

- Mặc định NVIDIA GPU (CUDA 12, CuPy + ONNX Runtime), tự fallback sang CPU (NumPy/OpenCV).
- Không dùng được GPU (máy không có card NVIDIA, chưa cài driver, driver cũ, lỗi CUDA) là
  **trường hợp bình thường**: app tự chọn CPU và **không báo lỗi, không bắt user cài gì**.
  Mục "⚡ NVIDIA GPU" trong ô chọn bị disable; chip màu xám "CPU · n process"; tooltip ghi
  "Không có GPU NVIDIA dùng được — app chạy bằng CPU. Không cần cài thêm gì." + lý do chi tiết
  (để hỗ trợ). Đã bỏ banner vàng cảnh báo ở màn Batch.
- `DeviceStatus` giữ lựa chọn của user (`preferred_device()`) tách khỏi thiết bị đang chạy
  (`device()`); `state.json` lưu lựa chọn → máy nào sau này cài driver thì tự dùng lại GPU.
- Cache kernel CuPy: `%LOCALAPPDATA%\PhotoBatchEditor\cupy_cache`.
- `--selftest` ghi `Documents\PhotoBatchEditor-selftest.json` (shortcut "GPU check").

## Lưu ý / giới hạn

- Muốn tăng tốc GPU thì cần driver NVIDIA hỗ trợ CUDA 12 (≥ 525). Driver là driver hệ thống
  (kernel, cần quyền admin) nên **không thể đóng gói vào app**; thiếu driver thì app chạy CPU.
  Không cần CUDA Toolkit: runtime CUDA đóng gói từ các gói pip `nvidia-*-cu12`.
- Lời nhắc driver cũ (`OLD_DRIVER`) chỉ là gợi ý "tùy chọn", chỉ thấy trong tooltip.
- Máy dev hiện không có GPU → test `gpu` tự skip.

## Test

`tests/test_backend.py`
