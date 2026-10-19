# Làm mịn da trong Sửa ảnh đơn

- **Mục đích:** làm mịn da ảnh chân dung (mụn, vết thâm, da sần) mà vẫn giữ mắt, tóc, viền rõ nét.
- **File:**
  - `rust/core/src/photo/skin.rs` (không UI): `SkinSmoothSettings` (+ `SKIN_SLIDERS`, `get` / `set` kẹp
    khoảng, `to_json` / `from_json`), `skin_probability`, `skin_mask`, `smooth_skin(img, settings, scale)`.
    Dùng `matting::guided_filter` / `box_mean` và `backend::gaussian_blur_exact`.
  - `rust/gui/src/photo/doc.rs`: `Background.skin`.
  - `rust/gui/src/photo/render.rs`: áp sau Adjust, trước Unsharp Mask (`adjusted_background`); kết quả
    nằm trong `BaseCache` nên kéo layer / tô Tem nhân bản không tính lại.
  - `rust/gui/src/photo/mod.rs`: mục "Làm mịn da" ở panel phải (`ui_skin_panel`), menu Image ›
    "Đặt lại chỉnh sửa" xoá cả làm mịn da.
  - `rust/core/src/i18n_vi_data.json`: chuỗi tiếng Việt.
- **Changelog:** [../changelog/skin-smoothing.md](../changelog/skin-smoothing.md)

## Logic chính

- Không phá huỷ: thông số lưu trong `Background.skin`, ảnh gốc giữ nguyên; kéo thanh trượt gộp 1 bước undo
  (merge key `background_skin`).
- Thanh trượt: **Độ mịn** 0–100 % (0 = tắt), **Bán kính** 1–30 px ảnh gốc (mặc định 8), **Giữ kết cấu da**
  0–100 % (mặc định 30); ô **Chỉ vùng da** (mặc định bật). Nút mức: Nhẹ (35 %, 6 px, 45 %), Vừa (60 %, 8 px,
  30 %), Mạnh (85 %, 12 px, 20 %); Đặt lại = tắt.
- Thuật toán `smooth_skin` (mỗi kênh R, G, B, thang 0…1):
  1. `q = guided_filter(p, p, r, eps)` — lọc giữ cạnh tự dẫn: vùng có độ lệch chuẩn nhỏ hơn ~`sd` bị làm
     phẳng, cạnh mạnh (mắt, môi, tóc) giữ lại. `sd = 0.03 + 0.07 × độ mịn`, `eps = sd²`; `r = bán kính × scale`.
     3 kênh chạy song song (`std::thread::scope`).
  2. Chi tiết rất nhỏ trên độ sáng: `fine = L − GaussianBlur(L, σ = max(0.6, r/5))` (lỗ chân lông).
  3. Mặt nạ da (nếu bật "Chỉ vùng da"): `skin_probability` theo YCbCr — Cb ∈ [77, 127], Cr ∈ [133, 173] (mép
     mềm ±8), nhân `smoothstep(30, 60, Y)` để bỏ vùng quá tối (tóc, bóng) — rồi làm mềm bằng `box_mean`
     bán kính `2r` để không lộ ranh giới.
  4. `out = p + độ_mịn × mặt_nạ × ((q + giữ_kết_cấu × fine) − p)`. Alpha giữ nguyên.
- Xem trước thu nhỏ: bán kính nhân `scale` → canvas và file xuất trông như nhau.
- Thứ tự ghép nền: Adjust → **Làm mịn da** → Unsharp Mask → tách nền → xoay tự do.

## Lưu ý / giới hạn

- Chỉ áp cho ảnh nền (không áp cho layer ảnh chèn), giống Adjust.
- Phát hiện da theo màu: vật có màu giống da (gỗ, cát, tường be) cũng bị làm mịn nhẹ; da rất tối / ánh đèn
  màu có thể không được nhận là da → tắt "Chỉ vùng da" để làm mịn cả ảnh.
- Không có cọ tô vùng làm mịn riêng (chỉ toàn ảnh / vùng da tự nhận).
- Tốc độ (release, Mac M-series): ~170 ms mỗi lần đổi thông số ở cỡ xem trước 1400 px; ~3–4 s cho ảnh 12 MP
  khi xuất.
- Chưa lưu vào `.pbep` (chưa port); `to_json` / `from_json` đã sẵn.

## Test

- `rust/core/src/photo/skin.rs` (`#[cfg(test)]`): `skin_colour_is_detected_and_others_are_not`,
  `smoothing_flattens_skin_and_keeps_the_edge` (da mịn hơn, cạnh da / tóc giữ, vùng tóc không đổi),
  `keep_texture_and_default`, `settings_json_clamps`.
- `rust/gui/tests/cpu_render.rs`: `render_applies_skin_smoothing_and_cache_matches`.
- Thủ công: `cd rust && cargo run -p pbe-gui` → Sửa ảnh đơn → mở ảnh chân dung → panel "Làm mịn da" → Vừa;
  zoom 100 % so trước / sau (Ctrl+Z), thử tắt "Chỉ vùng da", kéo "Giữ kết cấu da", xuất JPEG.
