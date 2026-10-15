# Tách nền (xoá phông, thay nền) trong Sửa ảnh đơn

- **Mục đích:** tách người khỏi phông nền bằng AI rồi để trong suốt / thay bằng màu (ảnh thẻ nền
  trắng, xanh, đỏ…) / thay bằng ảnh khác. Ghép áo, phụ kiện: dùng **Chèn ảnh** đặt ảnh PNG thành layer phía
  trên người (layer luôn nằm trên ảnh nền đã tách).
- **File:**
  - `rust/core/src/photo/matting.rs` (không UI, không ONNX): `Matte` (alpha 1 kênh, `crop` / `rotate90` /
    `flip` giống ảnh nền), `model_input_size` + `preprocess` (đầu vào model), `matte_from_model` (phóng matte
    + guided filter), `CutoutSettings` + `CUTOUT_SLIDERS` (Dịch viền, Làm mềm viền, Khử màu viền),
    `refined_alpha`, `apply_cutout`, `decontaminate`, `Backdrop` (trong suốt / màu / ảnh), `composite_backdrop`,
    `cover_resize`, `BACKDROP_COLORS` (màu ảnh thẻ), cọ `brush_dab` / `brush_line`, bộ lọc O(n): `box_mean`,
    `min/max_filter`, `guided_filter`.
  - `rust/core/src/bg_remove.rs` (feature `bgremove` của `pbe-core`, kéo `ort` / ONNX Runtime):
    `BackgroundRemover` (nạp model, `matte(img)`), `default_model_path`.
  - `models/modnet.onnx` (+ `models/LICENSE-MODNet.txt`).
  - GUI (`pbe-gui` luôn bật `pbe-core/bgremove`): `rust/gui/src/photo/doc.rs` (`Background.cutout`, matte đi
    theo crop / xoay 90° / lật), `render.rs` (áp matte + nền thay thế), `mod.rs` (menu Image › Tách nền, panel
    "Tách nền", thread nền `BgJob`, model dùng chung `REMOVER`, chế độ chỉnh vùng: `mask_controls`,
    `handle_mask_shortcuts`, `apply_mask_edit`), `mask_edit.rs` (`MaskEdit`: matte đang sửa, undo từng nét, ảnh phủ
    đỏ / xem kết quả), `render.rs` › `render_mask_base`, `rust/gui/src/fb.rs` (`checker_rect`: ô vuông chỗ trong
    suốt trên canvas).
- **Changelog:** [../changelog/background-removal.md](../changelog/background-removal.md)

## Logic chính

- **Model MODNet** (portrait matting, Apache-2.0; bản ONNX fp32 của `Xenova/modnet`, 25 MB): đầu vào
  `[1,3,H,W]` RGB chuẩn hoá `(x/255 − 0.5)/0.5`, cạnh ngắn 512 (cạnh dài ≤ 1024), hai cạnh làm tròn tới bội 32;
  đầu ra matte `[1,1,H,W]` 0…1. Pixel trong suốt của ảnh vào được ghép lên nền trắng trước. Tốc độ (release,
  Mac M-series): ~0.2 s ảnh nhỏ, ~1 s ảnh 8 MP (gồm phóng matte).
- **Phóng matte về cỡ ảnh** (`matte_from_model`): song tuyến, rồi nếu ảnh lớn hơn đầu vào model > 1.5 lần thì
  chạy guided filter (guide = độ sáng ảnh gốc, `r = max(2, round(1.5 × hệ số phóng))`, `eps = 1e-3`) để viền
  bám cạnh thật — **chỉ** trong dải viền chưa chắc chắn (0.02 < a < 0.98, giãn thêm `r` px); vùng chắc chắn giữ
  nguyên để không "rỉ" sang nền có hoạ tiết. Matte lưu u8 cùng cỡ ảnh, trong `Arc` (ảnh chụp undo rẻ).
- **Xem và sửa vùng chọn trước khi tách thật** (`mask_edit.rs`): AI chạy xong **chưa** áp dụng — editor vào chế
  độ chỉnh vùng: canvas hiện ảnh nền đã chỉnh màu (không xoay tự do, không layer, để pixel khớp matte), phần sẽ bị
  xoá phủ **đỏ** (đậm theo `1 − matte`, như Quick Mask) + **đường viền cyan** ở ranh giới matte 50 %; hoặc xem
  "Kết quả" (phần bị xoá thành trong suốt trên ô vuông). Người dùng tô cọ:
  - **+ Thêm vùng** (matte → giữ lại) / **− Bớt vùng** (matte → xoá); giữ **Alt** = cọ ngược lại; **X** đổi.
  - Cỡ cọ (px ảnh, mặc định cạnh dài / 25, phím **[ ]** ×/÷ 1.2), Độ cứng 0–100 % (lõi đặc = bán kính × độ cứng,
    ra mép mờ dần smoothstep). Nét kéo được nội suy (chấm cách ¼ bán kính) nên không đứt. Vòng tròn cỡ cọ + dấu
    +/− theo con trỏ.
  - Tô đồng thời vào matte cỡ thật (để xuất đúng) và bản cỡ xem trước (để vẽ lại nhanh).
  - **Ctrl+Z / ↶** hoàn tác từng nét (lưu hình bao vùng đã chạm, tối đa 30 nét), "Bỏ mọi nét tô" về lúc bắt
    đầu chỉnh, **Áp dụng / Enter** = tách thật (1 bước undo của tài liệu, giữ nền mới + tinh chỉnh viền nếu đã
    có), **Huỷ / Esc** = bỏ (không đổi tài liệu).
  - Trong lúc chỉnh: thanh công cụ dọc và phím chữ cái tắt, menu xoay / lật / cắt / Undo tài liệu tắt; Space /
    chuột giữa vẫn cuộn, lăn chuột / Ctrl+0 / Ctrl+1 vẫn zoom. Mở ảnh khác / đóng ảnh / đổi cỡ ảnh → tự huỷ.
  - Sau khi đã tách: nút **Sửa vùng chọn** mở lại chế độ này với matte hiện tại.
- **Lưu riêng, không phá ảnh gốc:** `Background.cutout = Some(Cutout { matte, settings, backdrop })`. Đổi nền,
  chỉnh viền, "Khôi phục nền gốc" (`cutout = None`) đều là 1 bước undo. Crop / xoay 90° / lật áp cho cả ảnh lẫn
  matte nên matte luôn cùng cỡ ảnh nền.
- **Vẽ** (`render.rs`, dùng chung cho canvas và xuất file): nền → Adjust → Unsharp Mask → **áp matte vào alpha**
  (`apply_cutout`: alpha = alpha cũ × matte đã tinh chỉnh) → xoay tự do → **vẽ lên nền thay thế**
  (`composite_backdrop`) → các layer. Nền thay thế vẽ sau khi xoay nên luôn phủ kín khung.
- **Tinh chỉnh viền** (đơn vị px ảnh gốc, xem trước thu nhỏ thì bán kính nhân theo tỉ lệ):
  - *Dịch viền* −10…10: âm = co (min filter, bỏ nền còn sót), dương = giãn (max filter).
  - *Làm mềm viền* 0…10: Gaussian gần đúng bằng 3 lần box filter.
  - *Khử màu nền ở viền* (mặc định bật, `decontaminate`): ước lượng màu phông cũ `B` quanh mỗi pixel từ các
    pixel chắc chắn là nền (a < 0.1) trong bán kính `cạnh dài / 60` (không có thì tìm ở bán kính ×4), rồi tách
    màu người `F = (I − (1 − a)·B) / a` cho pixel 0.01 ≤ a < 0.995. Ghép lên nền mới cho đúng
    `I + (1 − a)(N − B)` → hết quầng màu phông cũ quanh tóc.
- **Nền mới:** Trong suốt (canvas hiện ô vuông; xuất PNG giữ trong suốt, JPEG thì ghép lên trắng như cũ), 5 màu
  ảnh thẻ (Trắng, Xanh dương `#1E6FD9`, Xanh nhạt `#D6E8F7`, Đỏ `#D32F2F`, Xám `#C8C8C8`), màu tuỳ chọn
  (color picker, kéo màu gộp 1 bước undo), hoặc Ảnh… (phủ kín khung, cắt giữa, giữ tỉ lệ).
- **Chạy nền:** bấm "Tách nền" → thread riêng chạy model (lần đầu nạp model rồi giữ trong `REMOVER`), UI vẫn
  dùng được, thanh trạng thái hiện "Đang tách nền…". Khi xong: nếu đã mở ảnh khác (`doc_serial`) hoặc cỡ ảnh
  đổi (crop / xoay) trong lúc chạy thì bỏ kết quả và báo thử lại; nếu không → vào chế độ chỉnh vùng (ở trên).
  "Chạy lại" cũng qua bước chỉnh vùng, áp dụng thì giữ nền mới + tinh chỉnh viền.
- **Tìm model** (`default_model_path`): `<gốc dự án>/models/` (chạy từ mã nguồn / `PBE_APP_ROOT`), rồi
  `<thư mục exe>/models/` (Windows — `build_rust_windows.bat` chép vào), rồi
  `<exe>/../Resources/models/` (bundle `.app` macOS — `build_rust_mac.sh` chép vào).

## Lưu ý / giới hạn

- MODNet chuyên cho **chân dung người**; vật thể khác (sản phẩm, thú cưng) có thể tách không chuẩn.
- Chỉ áp cho ảnh nền, không áp cho layer ảnh chèn.
- Cọ sửa vùng chưa có "tự bám cạnh" (như Refine Edge của Photoshop) — muốn mép tóc mềm thì dùng độ cứng thấp.
- Mỗi nét cọ sao lưu cả matte cỡ thật một lần lúc bắt đầu nét (ảnh 24 MP ≈ 24 MB, giải phóng khi thả chuột).
- "Ghép áo khác" = chèn PNG áo có nền trong suốt bằng Chèn ảnh; app **không** tự sinh / thay quần áo bằng AI.
- Chưa có tách nền hàng loạt (tab batch).
- Mỗi lần vẽ lại (kéo layer, slider) đều tính lại tinh chỉnh viền + khử màu ở cỡ xem trước (≤ 1400 px) — đủ
  nhanh ở bản release; xuất file tính ở cỡ thật.
- License: MODNet Apache-2.0 — giữ `models/LICENSE-MODNet.txt` khi phát hành.

## Test

- `rust/core/src/photo/matting.rs` (`#[cfg(test)]`): cỡ đầu vào model, chuẩn hoá, matte crop / xoay / lật khớp
  ảnh, resize, `box_mean` = cách tính thô, dịch viền / làm mềm, áp alpha giữ alpha cũ, khử màu viền, nền màu /
  ảnh, guided filter làm viền sắc hơn.
- `rust/core/tests/bg_remove_onnx.rs` — `cargo test -p pbe-core --features bgremove --test bg_remove_onnx`
  (chạy model thật; bỏ qua nếu thiếu file model). Xem bằng mắt trên ảnh thật:
  `PBE_BG_SAMPLE=<ảnh> PBE_BG_OUT=<thư mục> cargo test --release -p pbe-core --features bgremove --test bg_remove_onnx -- --nocapture`
  → `matte.png`, `transparent.png`, `blue.jpg`, `blue_no_decontaminate.jpg`.
- `rust/gui/tests/cpu_render.rs`: `render_cutout_replaces_background_at_every_scale`,
  `cutout_matte_follows_crop_rotate_flip_and_undo`.
- `matting.rs`: `brush_adds_and_removes_with_soft_edge`, `brush_line_has_no_gaps`; `rust/gui/src/photo/mask_edit.rs`
  (`#[cfg(test)]`): tô / Alt / hoàn tác / bỏ nét, bản xem trước theo nét cỡ thật, ảnh phủ đỏ + viền + xem kết quả.
- Thủ công: `cargo run -p pbe-gui` → Sửa ảnh đơn → mở ảnh chân dung → panel "Tách nền" › Tách nền → kiểm tra
  vùng đỏ + viền, tô Thêm / Bớt (cả Alt, X, [ ]), Ctrl+Z, xem "Kết quả", Áp dụng (Enter) / Huỷ (Esc), "Sửa vùng
  chọn" sau khi đã tách → thử
  Trong suốt / các màu / màu tuỳ chọn / Ảnh…, kéo Dịch viền / Làm mềm viền, bật tắt Khử màu viền, Ctrl+Z, cắt /
  xoay sau khi tách, xuất PNG (trong suốt) và JPEG (nền màu).
