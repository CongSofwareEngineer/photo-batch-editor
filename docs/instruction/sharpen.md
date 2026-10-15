# Tăng độ nét (Sharpen) trong Sửa ảnh đơn

- **Mục đích:** cho phép làm ảnh nét hơn kiểu Photoshop ngay trong trang Sửa ảnh đơn: ngoài thanh "Độ nét"
  cũ (Sharpening Amount của batch) có thêm **Unsharp Mask** (Amount / Radius / Threshold như
  Filter › Sharpen › Unsharp Mask của Photoshop) và **Clarity** (tương phản cục bộ, cùng thuật toán batch).
- **File:**
  - `rust/core/src/photo/effects.rs`: `SharpenSettings` (+ `SHARPEN_SLIDERS`, `get` / `set` kẹp khoảng,
    `to_json` / `from_json`), `unsharp_mask(img, settings, scale)`; `PhotoAdjust.clarity` (thanh thứ 6
    của `ADJUST_SLIDERS`, map sang `AdjustmentSettings.clarity`).
  - `rust/gui/src/photo/doc.rs`: `Background.sharpen`.
  - `rust/gui/src/photo/render.rs`: áp Unsharp Mask sau Adjust, trước xoay tự do.
  - `rust/gui/src/photo/mod.rs`: mục "Tăng độ nét (Unsharp Mask)" ở panel phải (`ui_sharpen_panel`),
    menu Image › "Đặt lại chỉnh sửa" xoá cả độ nét.
  - `rust/core/src/i18n_vi_data.json`: chuỗi tiếng Việt.
- **Changelog:** [../changelog/sharpen.md](../changelog/sharpen.md)

## Logic chính

- Không phá huỷ: thông số lưu trong `Background.sharpen`, ảnh gốc giữ nguyên; kéo thanh trượt gộp thành 1
  bước undo (merge key `background_sharpen`).
- Unsharp Mask làm trên **độ sáng** `L = 0.299R + 0.587G + 0.114B`:
  `out = src + amount/100 × (L − GaussianBlur(L, σ = radius))`, cộng đều cho R, G, B → không tạo viền màu như
  làm nét từng kênh, và chỉ blur 1 mặt phẳng (nhanh gấp ~3 lần). Pixel có `|L − blur| < threshold` bị bỏ qua
  (giữ vùng mịn như da, bầu trời không bị nổi hạt). Alpha giữ nguyên. Blur dùng `backend::gaussian_blur_exact`.
- Khoảng: Amount 0–500 % (0 = tắt), Radius 0.1–10 px ảnh gốc (mặc định 1.0), Threshold 0–255 (mặc định 0).
- Xem trước thu nhỏ: σ = `radius × scale` (tỉ lệ bản xem trước) để canvas và file xuất trông như nhau; σ ≥ 0.3.
- Nút mức gợi ý: Nhẹ (60 %, 0.8 px, 2), Vừa (120 %, 1.2 px, 3), Mạnh (200 %, 1.6 px, 4); Đặt lại = tắt.
- Thứ tự ghép nền: Adjust (6 thanh, gồm Sharpness + Clarity của batch) → Unsharp Mask → xoay tự do.

## Lưu ý / giới hạn

- Chỉ áp cho ảnh nền (không áp cho layer ảnh chèn), giống Adjust.
- Ảnh rất lớn + radius lớn: mỗi lần đổi thông số chỉ tính trên bản xem trước (cạnh dài ≤ `PREVIEW_LONG_SIDE`);
  bản đầy đủ chỉ tính khi xuất.
- Chưa lưu vào file dự án `.pbep` (chưa port); `to_json` / `from_json` đã sẵn cho lúc đó.

## Test

- `rust/core/src/photo/effects.rs`: `unsharp_mask_increases_edge_contrast` (cạnh tối tối hơn, sáng sáng hơn,
  vùng phẳng giữ nguyên), `unsharp_mask_threshold_and_default`, `sharpen_settings_json_clamps`,
  `photo_adjust_uses_the_batch_algorithms` (có clarity).
- `rust/gui/tests/cpu_render.rs`: `render_applies_unsharp_mask_at_every_scale`.
- Thủ công: `cd rust && cargo run -p pbe-gui` → Sửa ảnh đơn → mở ảnh → panel phải "Tăng độ nét" → bấm Vừa,
  zoom 100 % so trước/sau (Ctrl+Z / Ctrl+Shift+Z), xuất JPEG kiểm tra file giống canvas.
