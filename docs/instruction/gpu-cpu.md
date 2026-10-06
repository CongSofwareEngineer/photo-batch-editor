# Log: GPU / CPU backend

- **Mục đích:** tăng tốc bằng NVIDIA GPU, tự chuyển sang CPU khi không dùng được GPU.
- **File:** `core/backend.py`, `ui/device_status.py`, `requirements-cuda.txt`

## Logic chính

- Mặc định NVIDIA GPU (CUDA 12, CuPy + ONNX Runtime), tự fallback sang CPU (NumPy/OpenCV).
- Status chip luôn hiển thị thiết bị đang dùng và lý do nếu không dùng được GPU.
- Cache kernel CuPy: `%LOCALAPPDATA%\PhotoBatchEditor\cupy_cache`.
- `--selftest` ghi `Documents\PhotoBatchEditor-selftest.json` (shortcut "GPU check").

## Lưu ý / giới hạn

- Cần driver hỗ trợ CUDA 12 (≥ 525). Không cần CUDA Toolkit: runtime CUDA cài qua pip.
- Máy dev hiện không có GPU → test `gpu` tự skip.

## Test

`tests/test_backend.py`

## Lịch sử thay đổi

- 2026-10-03: Ghi lại hiện trạng ban đầu.
