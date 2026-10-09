# Super Resolution

- **Mục đích:** phóng to ảnh 2x / 4x bằng AI.
- **File:** `rust/core/src/enhance.rs` (feature `sr`), `models/realesr-general-x4v3.onnx`
- **Changelog:** [../changelog/super-resolution.md](../changelog/super-resolution.md)
- **Ghi chú:** mã Python (`core/enhance.py`) và tool `tools/export_onnx.py` đã xoá cùng bản Python. Model ONNX vẫn nằm trong repo.

## Logic chính

- Real-ESRGAN chạy qua ONNX Runtime (`ort`), xử lý theo tile, sau Cargo feature `sr` (mặc định tắt
  để `core` không kéo ONNX runtime).
- Model ONNX được convert từ weights chính thức bằng `tools/export_onnx.py` (đã xoá; chỉ cần khi
  muốn tạo lại model từ weights, cần `torch`).

## Lưu ý / giới hạn

- Bản Python cũ yêu cầu `onnxruntime-gpu` < 1.27 (build cho CUDA 12); bản Rust dùng crate `ort`.
- License Real-ESRGAN: BSD-3-Clause (`models/LICENSE-Real-ESRGAN.txt`).

## Test

`rust/core/src/enhance.rs` (`#[cfg(test)]`) + `rust/core/tests/sr_onnx.rs` — chạy `cargo test --features sr`.
