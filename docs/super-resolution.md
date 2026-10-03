# Log: Super Resolution

- **Mục đích:** phóng to ảnh 2x / 4x bằng AI.
- **File:** `core/enhance.py`, `models/realesr-general-x4v3.onnx`, `tools/export_onnx.py`,
  `tools/requirements-export.txt`

## Logic chính

- Real-ESRGAN chạy qua ONNX Runtime, xử lý theo tile.
- Model ONNX được convert từ weights chính thức bằng `tools/export_onnx.py` (chỉ chạy trên
  máy dev, cần `torch`).

## Lưu ý / giới hạn

- `onnxruntime-gpu` phải < 1.27 (bản mới hơn build cho CUDA 13). Không cài `onnxruntime`
  thường song song.
- License Real-ESRGAN: BSD-3-Clause (`models/LICENSE-Real-ESRGAN.txt`).

## Test

`tests/test_enhance.py`

## Lịch sử thay đổi

- 2026-10-03: Ghi lại hiện trạng ban đầu.
