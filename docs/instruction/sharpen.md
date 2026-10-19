# Tăng độ nét (Sharpen) trong Sửa ảnh đơn

- **Mục đích:** cho phép làm ảnh nét hơn kiểu Photoshop ngay trong trang Sửa ảnh đơn: ngoài thanh "Độ nét"
  cũ (Sharpening Amount của batch) có thêm **Unsharp Mask** (Amount / Radius / Threshold như
  Filter › Sharpen › Unsharp Mask của Photoshop), **Clarity** (tương phản cục bộ, cùng thuật toán batch) và
  **Làm nét bằng AI** (Real-ESRGAN dựng lại chi tiết cho ảnh mềm / hơi mờ, giữ nguyên kích thước).
- **File:**
  - `rust/core/src/photo/effects.rs`: `SharpenSettings` (+ `SHARPEN_SLIDERS`, `get` / `set` kẹp khoảng,
    `to_json` / `from_json`), `unsharp_mask(img, settings, scale)`; `PhotoAdjust.clarity` (thanh thứ 6
    của `ADJUST_SLIDERS`, map sang `AdjustmentSettings.clarity`).
  - `rust/gui/src/photo/doc.rs`: `Background.sharpen`.
  - `rust/gui/src/photo/render.rs`: áp Unsharp Mask sau Adjust, trước xoay tự do.
  - `rust/gui/src/photo/mod.rs`: mục "Tăng độ nét (Unsharp Mask)" ở panel phải (`ui_sharpen_panel`),
    menu Image › "Đặt lại chỉnh sửa" xoá cả độ nét.
  - Làm nét bằng AI: `rust/core/src/photo/effects.rs` › `ai_sharpen`, `AI_SHARPEN_TILE` / `AI_SHARPEN_PAD`
    (dùng trait `pipeline::Upscaler`, cài đặt là `enhance::SuperResolver` — feature `sr` của `pbe-core`, GUI
    luôn bật); `rust/gui/src/photo/mod.rs` › `ui_ai_sharpen`, `start_ai_sharpen`, `poll_sr_job`, `SrJob`,
    model dùng chung `RESOLVER`, menu Image › "Làm nét bằng AI". Model `models/realesr-general-x4v3.onnx`
    (đã có, build script chép cả thư mục `models/`).
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
- Thứ tự ghép nền: Adjust (6 thanh, gồm Sharpness + Clarity của batch) → Làm mịn da → Unsharp Mask → xoay tự do.

### Làm nét bằng AI

- Mục "Làm nét bằng AI" (dưới Unsharp Mask) hoặc menu Image: thanh **Mức độ AI** 10–100 % (mặc định 100), nút
  **Làm nét bằng AI**, dòng ước lượng thời gian (~10 s / MP). Chạy ở thread nền (`SrJob`), thanh trạng thái +
  panel hiện % tiến độ, nút **Huỷ**.
- `ai_sharpen`: chia ảnh thành ô 480 px (+ lề 16 px mỗi bên, kẹp trong ảnh), mỗi ô phóng ×4 bằng model
  (Real-ESRGAN dựng lại chi tiết, khử nhiễu) rồi **thu về đúng cỡ cũ** bằng trung bình khối 4×4, chỉ giữ phần
  lõi ô → ảnh nét hơn mà kích thước không đổi, bộ nhớ ~60 MB / ô bất kể cỡ ảnh. Trộn `gốc + mức × (AI − gốc)`;
  alpha giữ nguyên. `SuperResolver` tạo với tile 512 để mỗi ô chạy model một lần.
- **Phá huỷ có undo:** kết quả thay `Background.img` (1 bước undo "AI sharpen"); Adjust / mịn da / Unsharp vẫn áp
  lên ảnh mới. Nếu đã mở ảnh khác hoặc cỡ ảnh đổi trong lúc chạy → bỏ kết quả, báo thử lại.

## Lưu ý / giới hạn

- Chỉ áp cho ảnh nền (không áp cho layer ảnh chèn), giống Adjust.
- Ảnh rất lớn + radius lớn: mỗi lần đổi thông số chỉ tính trên bản xem trước (cạnh dài ≤ `PREVIEW_LONG_SIDE`);
  bản đầy đủ chỉ tính khi xuất.
- Chưa lưu vào file dự án `.pbep` (chưa port); `to_json` / `from_json` đã sẵn cho lúc đó.
- Làm nét bằng AI chậm trên CPU: ~10 s / MP (Mac M-series, ~3.6 lõi) → ảnh 12 MP ≈ 2 phút; lần đầu thêm thời
  gian nạp model. Hợp với ảnh mềm / hơi mờ / nhiễu; ảnh rung mạnh hoặc mất nét nặng thì AI không cứu được.
  Mô hình có thể làm mịn kết cấu rất nhỏ (da, vải) — giảm "Mức độ AI" nếu thấy giả.

## Test

- `rust/core/src/photo/effects.rs`: `unsharp_mask_increases_edge_contrast` (cạnh tối tối hơn, sáng sáng hơn,
  vùng phẳng giữ nguyên), `unsharp_mask_threshold_and_default`, `sharpen_settings_json_clamps`,
  `photo_adjust_uses_the_batch_algorithms` (có clarity).
- `rust/gui/tests/cpu_render.rs`: `render_applies_unsharp_mask_at_every_scale`.
- `rust/core/src/photo/effects.rs`: `ai_sharpen_keeps_size_alpha_and_blends` (model giả: ghép ô, trộn, huỷ).
- `rust/core/tests/sr_onnx.rs`: `ai_sharpen_with_the_real_model` — `cargo test --release -p pbe-core --features sr
  --test sr_onnx -- --nocapture` (model thật, 2 ô, cạnh dốc hơn, in thời gian; bỏ qua nếu thiếu model).
- Thủ công: `cd rust && cargo run -p pbe-gui` → Sửa ảnh đơn → mở ảnh → panel phải "Tăng độ nét" → bấm Vừa,
  zoom 100 % so trước/sau (Ctrl+Z / Ctrl+Shift+Z), xuất JPEG kiểm tra file giống canvas.
