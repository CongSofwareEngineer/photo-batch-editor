# Log: 15 chỉnh sửa kiểu Camera Raw

- **Mục đích:** các thanh chỉnh ảnh đặt tên giống Camera Raw.
- **File:** `core/adjustments.py`, `core/settings.py`, `core/pipeline.py`,
  `ui/adjustment_panel.py`, `ui/preview.py`

## Logic chính

- 15 chỉnh sửa: Temperature, Tint, Exposure, Brightness, Contrast, Highlights, Shadows,
  Whites, Blacks, Clarity, Vibrance, Saturation, Sharpening › Amount, Noise Reduction,
  Post-Crop Vignetting › Amount.
- Có bản tham chiếu (reference) và bản fused (fast path) chạy nhanh.
- Giới hạn giá trị + đọc/ghi JSON nằm trong `AdjustmentSettings` (`core/settings.py`).
- Editor có live preview.

## Test

`tests/test_adjustments.py`

## Lịch sử thay đổi

- 2026-10-03: Ghi lại hiện trạng ban đầu.
