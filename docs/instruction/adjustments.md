# 15 chỉnh sửa kiểu Camera Raw

- **Mục đích:** các thanh chỉnh ảnh đặt tên giống Camera Raw.
- **File:** `rust/core/src/adjustments.rs`, `rust/core/src/settings.rs`, `rust/core/src/pipeline.rs`,
  `rust/gui/src/adjust.rs`, `rust/gui/src/preview.rs`
- **Changelog:** [../changelog/adjustments.md](../changelog/adjustments.md)
- **Ghi chú:** mã Python (`core/adjustments.py`, `ui/adjustment_panel.py`…) đã xoá; bản triển khai hiện tại là Rust. Nhánh GPU không port. Chi tiết port: [rust-port.md](rust-port.md).

## Logic chính

- 15 chỉnh sửa: Temperature, Tint, Exposure, Brightness, Contrast, Highlights, Shadows,
  Whites, Blacks, Clarity, Vibrance, Saturation, Sharpening › Amount, Noise Reduction,
  Post-Crop Vignetting › Amount.
- Có bản tham chiếu (reference) và bản fused (fast path) chạy nhanh.
- Giới hạn giá trị + đọc/ghi JSON nằm trong `AdjustmentSettings` (`core/settings.py`).
- Editor có live preview.

## Test

`tests/test_adjustments.py`
