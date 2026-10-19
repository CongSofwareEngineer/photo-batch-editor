# Tem nhân bản (Clone Stamp) trong Sửa ảnh đơn

- **Mục đích:** lấy một vùng của ảnh làm mẫu rồi tô mẫu đó lên vùng khác (xoá vết, che vật thừa, nhân bản
  chi tiết), như Clone Stamp của Photoshop.
- **File:**
  - `rust/gui/src/photo/doc.rs`: `Layer::Clone(CloneLayer)`, `CloneStroke` (offset, points, radius,
    hardness, opacity), `Document::add_clone_layer`; `bounds` / `translate` / `map_points` / `set_bounds` của
    layer nhân bản; `Layer::resizable()` (layer nhân bản không có nút góc).
  - `rust/gui/src/photo/render.rs`: `apply_clone_layer`.
  - `rust/gui/src/photo/mod.rs`: `Tool::Clone` (phím **S**, icon con dấu), `ui_clone_options`,
    `handle_clone_input`, `begin_clone_stroke`, `extend_clone_stroke`, vòng cọ + dấu điểm mẫu trong
    `paint_canvas`, phím `[ ]`.
  - Dùng chung: `pbe_core::photo::matting::dab_weight` (độ mềm mép cọ, giống cọ sửa vùng tách nền).
- **Changelog:** [../changelog/clone-stamp.md](../changelog/clone-stamp.md)

## Logic chính

- **Thao tác:** chọn công cụ Tem nhân bản (S) → **Alt + bấm** lên ảnh để chọn điểm mẫu → kéo chuột ở chỗ cần
  sửa: pixel lấy từ vị trí lệch `offset = điểm mẫu − điểm bắt đầu nét`. Chưa chọn điểm mẫu mà tô → báo
  "Giữ Alt + bấm lên ảnh để chọn vùng mẫu".
- **Căn chỉnh (Aligned)** (mặc định bật): độ lệch giữ nguyên giữa các nét (điểm mẫu đi theo cọ). Tắt: mỗi nét
  mới lại bắt đầu từ đúng điểm mẫu. Alt + bấm lần nữa = điểm mẫu mới (xoá độ lệch cũ).
- **Tuỳ chọn** (thanh tuỳ chọn): Cỡ cọ (px ảnh, mặc định cạnh dài / 20, `[ ]` ×/÷ 1.2), Độ cứng 0–100 %
  (lõi đặc = bán kính × độ cứng, ra mép mờ dần smoothstep), Độ trong suốt 1–100 %.
- **Lưu dạng vector, không sửa ảnh gốc:** mỗi nét = `CloneStroke` trong một layer "Clone N". Nét mới vào layer
  nhân bản đang chọn, nếu không có thì tạo layer mới trên cùng. Mỗi nét = 1 bước undo (merge key
  `clone_stroke`, ngắt khi nhả chuột); điểm chỉ được thêm khi chuột đi ≥ 5 % cỡ cọ.
- **Vẽ (`apply_clone_layer`)**, cho từng nét theo thứ tự: độ phủ = max trọng số các chấm cọ (cách nhau ¼ bán
  kính) dọc đường nét; chụp vùng nguồn (lệch `offset`) của **ảnh ghép bên dưới layer** trước khi vẽ (nét
  không tự lặp lại), lấy mẫu song tuyến, trộn cả 4 kênh theo `độ phủ × opacity nét × opacity layer`. Toạ độ,
  bán kính, độ lệch nhân `scale` → xem trước và xuất giống nhau.
- **Layer nhân bản** di chuyển được bằng Move (kéo / mũi tên), ẩn / hiện, opacity, nhân đôi, xoá, sắp xếp như
  layer khác; không có nút góc đổi cỡ. Crop / xoay 90° / lật: điểm của nét đi theo canvas, độ lệch nguồn được
  xoay / lật như một vectơ.
- Canvas: con trỏ chữ thập + vòng cọ; dấu **+** và vòng mờ ở điểm đang lấy mẫu (đi theo cọ khi đã có độ lệch).

## Lưu ý / giới hạn

- Lấy mẫu "ảnh hiện tại và bên dưới" (như Sample: Current & Below): layer nằm **trên** layer nhân bản không
  bị chép. Di chuyển layer bên dưới thì nội dung được chép cũng đổi theo (vì nét lưu dạng vector).
- Nguồn nằm ngoài ảnh → lấy pixel mép (kẹp biên); nguồn hoàn toàn ngoài ảnh → nét không vẽ.
- Chưa có Healing Brush (tự hoà màu / sáng với vùng xung quanh) — dùng độ cứng thấp + opacity < 100 % để mép
  tự nhiên hơn.
- Chưa lưu vào `.pbep` (chưa port).

## Test

- `rust/gui/tests/cpu_render.rs`: `clone_stamp_copies_the_sample_at_every_scale` (đúng màu nguồn ở tỉ lệ 1 và
  0.5, ngoài nét giữ nguyên, bounds, opacity), `clone_stroke_follows_flip_and_move`.
- Thủ công: `cd rust && cargo run -p pbe-gui` → Sửa ảnh đơn → mở ảnh → S → Alt + bấm vào vùng da / nền sạch →
  tô lên vết cần xoá; thử tắt Căn chỉnh, đổi cỡ bằng `[ ]`, Ctrl+Z từng nét, ẩn layer "Clone 1", lật ảnh, xuất.
