# Sửa ảnh đơn (Photo editor)

- **Mục đích:** trang "Sửa ảnh đơn" kiểu Photoshop đơn giản: mở 1 ảnh, thêm layer chữ / ảnh /
  vùng làm mờ, cắt, xoay / lật, chỉnh màu, hoàn tác ≥ 30 bước, xuất JPEG/PNG, lưu file dự án
  `.pbep` (giữ layer) để mở lại sửa tiếp. Ảnh gốc không bao giờ bị ghi đè.
- **File:**
  - `rust/core/src/photo/` (không UI): `history.rs` (undo/redo snapshot), `geometry.rs` (zoom 10–1600 %,
    zoom tại con trỏ, khung crop theo tỉ lệ, xoay 90° / lật toạ độ, cover_scale cho xoay tự do),
    `effects.rs` (blur / pixelate, `PhotoAdjust` → `AdjustmentSettings`),
    `image_io.rs` (đọc RGBA giữ trong suốt, ghi JPEG/PNG, thư mục `editor`), `collage.rs` (xem collage.md)
  - `rust/gui/src/photo/`: `doc.rs` (Document + ImageLayer / BlurLayer / CloneLayer + Background,
    `DocState::bg_index` / `stack_ids`, `Document::move_to` / `move_selected` / `move_selected_to_end`,
    `BACKGROUND_ID`, `Layer::set_bounds`, `Document::new_untitled`), `render.rs` (ghép CPU: nền → Adjust →
    Làm mịn da → Unsharp Mask → tách nền → xoay tự do, chèn vào ngăn layer tại `bg_index`; `BaseCache`),
    `mod.rs` (bố cục Photoshop: menu, options bar, toolbox dọc, status bar; zoom / pan; công cụ Move,
    Crop, Blur, Tem nhân bản, Hand, Zoom; nút góc đổi cỡ; panel Thuộc tính / Tách nền / Adjust / Làm mịn da /
    Độ nét / Làm nét AI / Layer / Xuất; menu chuột phải Sắp xếp),
    `collage.rs` (cửa sổ cắt ghép — xem collage.md)
  - Làm mịn da: [skin-smoothing.md](skin-smoothing.md); Tem nhân bản: [clone-stamp.md](clone-stamp.md);
    Unsharp Mask + Làm nét bằng AI: [sharpen.md](sharpen.md).
  - Tách nền / thay nền: `rust/core/src/photo/matting.rs`, `rust/core/src/bg_remove.rs`,
    `rust/gui/src/photo/mask_edit.rs` (chế độ chỉnh vùng chọn bằng cọ trước khi tách) — xem
    [background-removal.md](background-removal.md)
  - Dùng chung: `rust/gui/src/app.rs`, `sidebar.rs` (trang photo), `theme.rs`, `rust/core/src/i18n*`
- **Changelog:** [../changelog/photo-editor.md](../changelog/photo-editor.md)
- **Ghi chú:** mã Python (`core/photo/`, `ui/photo/`) đã xoá; logic → `rust/core/src/photo/`; GUI → `rust/gui/src/photo/` (egui + framebuffer wgpu). **Chưa** port: layer chữ, file dự án `.pbep`. Chi tiết: [rust-port.md](rust-port.md).

## Logic chính

> Mục **"Bản Rust hiện tại"** ngay dưới đây là hiện trạng code. Các mục sau đó mô tả bản Python cũ (đã xoá),
> giữ làm chuẩn hành vi cho phần chưa port (chữ, `.pbep`).

### Bản Rust hiện tại (`rust/gui/src/photo/mod.rs`)

- **Bố cục kiểu Photoshop** (thứ tự panel egui: menu → options → status → toolbox → panel phải → canvas):
  - **Thanh menu** (trên cùng): File (Mở ảnh Ctrl+O, Ảnh ghép mới…, Chèn ảnh, Xuất Ctrl+E, Đóng ảnh),
    Edit (Hoàn tác Ctrl+Z, Làm lại Ctrl+Shift+Z, Nhân đôi layer Ctrl+J, Xoá layer Del, Sắp xếp: Đưa lên trên
    cùng Ctrl+Shift+] / lên một lớp Ctrl+] / xuống một lớp Ctrl+[ / xuống dưới cùng Ctrl+Shift+[), Image (xoay
    trái / phải 90°, lật ngang / dọc, Cắt, Tách nền, Làm nét bằng AI, Đặt lại chỉnh sửa), View (phóng to / thu nhỏ, Vừa màn hình Ctrl+0,
    100 % Ctrl+1); tên ảnh (• = chưa lưu); bên phải: ↶ ↷ + nút Xuất.
  - **Thanh tuỳ chọn** (dưới menu, như options bar): icon công cụ đang chọn + tuỳ chọn riêng: Move (Chèn
    ảnh, cỡ layer đang chọn, gợi ý), Crop (tỉ lệ Tự do / 1:1 / 4:3 / 16:9 / 9:16, Áp dụng / Huỷ, cỡ vùng cắt),
    Blur (Làm mờ / Ô vuông, Chữ nhật / Elip, Độ mạnh), Tem nhân bản (cỡ cọ, độ cứng, độ trong suốt, Căn chỉnh),
    Hand / Zoom (+ / −, 100 %, Vừa màn hình).
  - **Thanh công cụ dọc** (trái, 48 px, hằng `TOOLBAR`): Move V, Crop C | Blur B, Tem nhân bản S, Chèn ảnh I,
    Collage L | Hand H, Zoom Z. Icon vẽ bằng vector (`paint_icon`) nên không phụ thuộc font; tooltip "Tên (phím)".
    Chèn ảnh / Collage là nút hành động (không phải công cụ). Chưa mở ảnh thì chỉ Collage bấm được.
  - **Panel phải**: Thuộc tính (opacity, độ mạnh / kiểu blur của layer chọn; chọn dòng "Nền" → độ trong suốt
    của nền), Tách nền (AI xoá phông, nền
    trong suốt / màu ảnh thẻ / ảnh, tinh chỉnh viền — xem [background-removal.md](background-removal.md)), Adjust (6 thanh: sáng, tương
    phản, bão hoà, nhiệt độ, Sharpness, Clarity + Làm thẳng −45…45°), Tăng độ nét (Unsharp Mask — xem
    [sharpen.md](sharpen.md)), Làm mịn da ([skin-smoothing.md](skin-smoothing.md)), Làm nét bằng AI, Layer
    (xem "Thứ tự layer" bên dưới), Xuất (JPEG chất lượng / PNG).
  - **Thanh trạng thái** (dưới): % zoom, cỡ ảnh, toạ độ X/Y px ảnh dưới con trỏ, số layer.
- **Nhiều layer chồng nhau (như Photoshop):** số layer không giới hạn.
  - **Chèn:** File › Chèn ảnh / nút I chọn được **nhiều ảnh một lần** (mỗi ảnh một layer, lệch nhau 3 % cạnh dài
    để thấy hết); hoặc **kéo thả file** từ Finder / Explorer vào cửa sổ (`handle_dropped_files`: chưa mở ảnh → file
    đầu làm nền, còn lại thành layer; đã mở → tất cả thành layer; khi đang kéo hiện lớp phủ "Thả để …").
  - Layer mới (ảnh, blur, nhân bản) chèn **ngay trên mục đang chọn** (`Document::insert_layer`); chọn Nền → ngay
    trên Nền; layer đang chọn nằm dưới Nền → layer mới cũng dưới Nền; không chọn gì → trên cùng.
  - **Bấm chọn trên canvas theo pixel** (`Layer::hit`, như Auto-Select): layer ảnh chỉ "trúng" chỗ alpha > 10 %
    (bỏ qua phần trong suốt của PNG, có tính xoay / lật), blur theo chữ nhật / elip, nhân bản theo nét. Layer nằm
    dưới Nền đục bị che → không chọn bằng chuột (chọn trong bảng Layer).
  - Bảng Layer: tiêu đề kèm số layer, mỗi dòng có **ảnh thu nhỏ** (texture 64 px, cache theo con trỏ `Arc` ảnh —
    `layer_thumb`; blur / nhân bản hiện icon), **nhấp đúp tên để đổi tên** (Enter / bấm ra ngoài = lưu, Esc =
    huỷ; `rename_layer`, 1 bước undo; Nền không đổi tên).
- **Thứ tự layer (đổi chỗ ảnh đè lên nhau):** bảng Layer liệt kê trên → dưới **gồm cả dòng "Nền"** (chữ
  nghiêng) ở đúng vị trí của nó. Chọn một dòng (cả Nền) rồi bấm ⏶ lên trên cùng / ▲ lên một lớp / ▼ xuống một lớp
  / ⏷ xuống dưới cùng, hoặc **kéo thả** tên dòng (vạch cyan chỉ chỗ thả), hoặc Ctrl+] / Ctrl+[ (thêm Shift = trên
  cùng / dưới cùng), hoặc chuột phải trên canvas (công cụ Move: chọn layer dưới con trỏ, chỗ trống = Nền) →
  Sắp xếp / Nhân đôi / Xoá. Mỗi lần đổi = 1 bước undo.
  - Nền không phải layer trong `layers`: vị trí của nó là `DocState::bg_index` (số layer vẽ **dưới** nền,
    0 = dưới cùng như cũ). `stack_ids()` trả về thứ tự dưới → trên với `BACKGROUND_ID = 0`; `move_to(id, vị trí)`
    xếp lại `layers` + `bg_index`. Xoá / nhân đôi layer nằm dưới nền tự chỉnh `bg_index`. Nền không xoá / nhân đôi
    được.
  - Vẽ (`render::compose`): nền dưới cùng + đục (mặc định) → dùng thẳng ảnh nền làm canvas như trước; nếu không
    → canvas trong suốt, vẽ các layer dưới nền, trộn ảnh nền (đã tách nền / xoay / nền thay thế) với
    `Background.opacity`, rồi vẽ các layer trên. Ví dụ ghép: tách nền người (nền Trong suốt) → chèn ảnh phong
    cảnh → đưa ảnh đó xuống dưới Nền. Nền đục nằm trên layer khác thì che hết — bảng Layer nhắc tách nền hoặc
    giảm độ trong suốt.
- **Bộ nhớ / tốc độ:** ảnh nền và ảnh của layer chèn giữ trong `Arc<RgbaImage>` → mỗi bước undo chỉ tăng bộ đếm
  tham chiếu (trước đây copy cả ảnh mỗi lần kéo). `BaseCache` giữ ảnh nền đã Adjust / mịn da / Unsharp ở cỡ xem
  trước (khoá = con trỏ `Arc` ảnh + thông số + tỉ lệ) → kéo layer, tô Tem nhân bản không tính lại bộ lọc.
- **Công cụ trên canvas:** Move: bấm = chọn layer trên cùng tại điểm (chỗ trống = bỏ chọn), kéo layer = di
  chuyển, kéo 4 nút góc trắng = đổi cỡ (góc đối diện đứng yên, Shift giữ tỉ lệ; `resize_from_corner`), kéo chỗ
  trống = cuộn, mũi tên nhích 1 px / Shift 10 px (layer nhân bản không có nút góc). Tem nhân bản: Alt + bấm =
  điểm mẫu, kéo = tô (xem [clone-stamp.md](clone-stamp.md)). Crop: kéo khung (tối phần ngoài + lưới 1/3), Enter áp dụng,
  Esc huỷ. Blur: kéo khung ≥ 4 px → tạo layer Blur. Hand: kéo = cuộn. Zoom: bấm = phóng to tại điểm bấm, Alt +
  bấm = thu nhỏ. Mọi công cụ: Space + kéo hoặc chuột giữa = cuộn; lăn chuột = zoom tại con trỏ.
  Con trỏ chuột đổi theo công cụ (Grab, ZoomIn/Out, Crosshair, Resize ở nút góc).
- **Phím tắt:** chữ cái chọn công cụ chỉ khi không gõ trong ô nhập (`egui_wants_keyboard_input`) và không giữ
  Ctrl/Cmd; khi cửa sổ Collage mở thì editor bỏ qua mọi phím. Kéo / đổi cỡ / nhích / slider dùng merge key nên
  mỗi thao tác = 1 bước undo.
- **Canvas** hiện ô vuông xám / trắng (như Photoshop) ở chỗ ảnh trong suốt (PNG, nền đã tách).
- **Ảnh ghép**: kết quả cửa sổ Collage mở thành tài liệu mới tên "collage" (`Document::new_untitled`,
  đánh dấu chưa lưu); xuất mặc định `collage.jpg` trong thư mục ảnh gần nhất.

### Bản Python cũ (chuẩn hành vi)

- **Bố cục:** trên = Mở / Lưu dự án / Xuất / Hoàn tác / Làm lại + menu "Thêm" (mở dự án, lưu thành,
  tạo ảnh ghép, đóng ảnh); trái = thanh công cụ dọc; giữa = canvas; phải = thuộc tính (đổi theo công cụ /
  layer đang chọn) + bảng Layer; dưới = % zoom, nút −/+ / Vừa khung / 100 %, kích thước ảnh, toạ độ con trỏ.
- **Document:** `DocState` = `Background` (QImage gốc ARGB32 premultiplied + `PhotoAdjust` + góc xoay tự do +
  hiện/ẩn) + list layer (dưới → trên). QImage chia sẻ copy-on-write, không bao giờ sửa tại chỗ → snapshot rẻ.
  Mọi thao tác gọi `doc.checkpoint(label, key)` *trước* khi đổi; cùng `key` trong 1 giây được gộp làm 1 bước
  (kéo slider, gõ chữ). Giới hạn 50 bước. Thao tác kéo chuột chỉ ghi 1 bước ở lần di chuyển đầu tiên.
- **Vẽ (`render.py`):** painter đặt theo toạ độ ảnh; canvas và export dùng chung `Renderer.paint`.
  - Nền: `BaseCache` giữ bản xem trước (cạnh dài ≤ 1600 px, tính ngay khi kéo slider) và bản đầy đủ
    (tính nền trong QThreadPool, xong thì vẽ lại). Khi zoom nhỏ dùng bản xem trước → mượt với ảnh lớn;
    chỉ vẽ phần ảnh đang hiện; zoom ≥ 200 % tắt làm mịn (thấy rõ pixel như Photoshop).
  - Chữ: `QPainterPath.addText` (đúng tiếng Việt có dấu), viền = stroke ×2 vẽ dưới phần tô; khi trong suốt
    < 100 % có viền thì vẽ qua buffer để không lộ viền bên trong.
  - Ảnh chèn: transform = dịch tâm + xoay + lật, vẽ `SmoothPixmapTransform`.
  - Vùng mờ: lấy ảnh ghép của các layer *bên dưới* trong vùng (có lề cho blur), áp blur/pixelate NumPy, cắt theo
    mask (chữ nhật / elip / nét cọ), cache theo chữ ký các layer bên dưới. Độ phân giải vùng mờ theo zoom
    (1/8, 1/4, 1/2, 1) → kéo thả nhanh; export luôn 100 %. Pixelate canh lưới theo toạ độ tuyệt đối.
- **Zoom / pan (`canvas.py`):** `screen = offset + image × scale`. Ctrl + lăn chuột = zoom tại con trỏ
  (×1.2 mỗi nấc, 10–1600 %); lăn = cuộn, Shift + lăn = cuộn ngang; Space + kéo hoặc kéo chuột giữa = pan;
  Ctrl+0 = vừa khung (phóng to cả ảnh nhỏ như Photoshop), Ctrl+1 = 100 %, Ctrl++ / Ctrl+- = bước zoom.
  Mở ảnh: vừa khung nhưng không phóng quá 100 %.
- **Công cụ** (phím tắt khi focus ở trang): V Chọn (chọn layer trên cùng tại điểm bấm, kéo = di chuyển,
  góc = đổi cỡ – Shift giữ tỉ lệ, nút tròn = xoay – Shift bước 15°, mũi tên nhích 1 px / Shift 10 px, Del xóa),
  M Di chuyển, C Cắt (kéo khung, tỉ lệ Tự do / Như gốc / 1:1 / 4:3 / 16:9 / 9:16, Enter cắt, Esc hủy),
  T Chữ (bấm chỗ trống → hộp nhập nhiều dòng; double-click chữ để sửa; font, cỡ px, màu, đậm/nghiêng, viền,
  độ trong suốt, góc xoay), I Chèn ảnh (PNG giữ nền trong suốt, đặt giữa, tối đa 50 % khung), B Làm mờ
  (chữ nhật / elip / cọ; kiểu blur / pixelate; độ mờ 1–100 tỉ lệ theo cỡ ảnh; cỡ cọ, phím [ ]),
  A Chỉnh ảnh (độ sáng, tương phản, bão hòa, nhiệt độ màu, độ nét = đúng thuật toán của tab hàng loạt qua
  `core.pipeline.apply_adjustments`), R Xoay (trái/phải 90°, lật ngang/dọc, xoay tự do −45…45°),
  L Cắt ghép layout (collage.md).
- **Crop / xoay 90° / lật** áp cho cả khung: ảnh nền được cắt/xoay, các layer dời / xoay theo. Nếu nền có góc
  xoay tự do thì "nướng" góc vào ảnh trước khi cắt. Xoay tự do chỉ xoay ảnh nền và phóng to để không hở góc.
- **Layer:** danh sách trên → dưới, ô đánh dấu = hiện/ẩn, double-click = đổi tên, nút lên / xuống / nhân đôi /
  xóa. "Nền" luôn ở dưới cùng, chỉ ẩn/hiện được. Tên layer chữ = dòng đầu nội dung.
- **Xuất (Ctrl+E):** hộp thoại JPEG (chất lượng 1–100, mặc định 92; ≥ 90 dùng 4:4:4) hoặc PNG (giữ trong suốt);
  mặc định `<thư mục ảnh>/editor/<tên>.jpg`. Từ chối ghi đè ảnh gốc, hỏi trước khi thay file đã có.
  Ảnh ghép được vẽ trên GUI thread (QPainter), ghi file chạy nền (Task). JPEG: nền trong suốt → trắng.
- **File dự án `.pbep`:** ZIP gồm `project.json` + `background.png` + `images/<id>.png`. Giữ chữ (sửa được),
  ảnh chèn (độ phân giải gốc), vùng mờ (nét cọ dạng vector), thông số chỉnh màu và góc xoay. Ảnh gốc nằm trong
  file nên không phụ thuộc file nguồn. Lưu qua `.tmp` + rename.
- **Mở từ tab hàng loạt:** chuột phải ảnh trong danh sách ẢNH (Prepare) hoặc trong bảng Kết quả → "Mở trong
  Sửa ảnh đơn". Kéo thả: chưa có ảnh → mở; đã có ảnh → chèn thành layer; `.pbep` → mở dự án.
- **Đổi ngôn ngữ** (cửa sổ dựng lại): document được chuyển sang cửa sổ mới (`take_document` / `adopt_document`).
- **Thoát / mở ảnh khác khi có thay đổi chưa lưu/xuất:** hỏi Lưu dự án / Bỏ thay đổi / Hủy.

## Lưu ý / giới hạn

- Chỉnh ảnh chỉ áp cho ảnh nền (không áp cho ảnh chèn). Vùng mờ làm mờ mọi layer bên dưới nó.
- Lật khung: chữ vẫn đọc được (chỉ đổi vị trí và góc), ảnh chèn bị lật thật.
- Ảnh 16-bit được đọc qua bộ đọc của batch rồi chuyển 8-bit (editor làm việc 8-bit RGBA).
- Bộ nhớ: mỗi bước undo chỉ giữ tham chiếu QImage; crop/xoay nhiều lần trên ảnh rất lớn sẽ giữ nhiều bản ảnh.
- Phím chữ cái (V, C, B, I, L, H, Z) chỉ là phím tắt khi không gõ trong ô nhập; Ctrl+1 của trang Hàng loạt tạm
  tắt khi đang ở trang này (nhường cho zoom 100 %).
- Bản Rust: nút góc đổi cỡ dùng hình bao **chưa xoay** của layer; chưa có nút xoay layer bằng chuột (bản Python có).
- Bản Rust: Space + kéo cần focus ở canvas; nếu một nút đang được focus thì Space có thể bấm nút đó.
- Bản Rust: menu chuột phải trên canvas chỉ có ở công cụ Move. Chưa chọn nhiều layer cùng lúc (Shift / Ctrl +
  bấm), chưa có nhóm layer / chế độ hoà trộn (blend mode).

## Test

- `rust/core/src/photo/*.rs` (`#[cfg(test)]`): history (gộp key, giới hạn), zoom tại con trỏ, crop theo tỉ
  lệ, xoay/lật toạ độ, cover_scale, pixelate / blur, chỉnh ảnh = đúng pipeline batch, đọc RGBA, ghi
  JPEG/PNG, đường dẫn `editor` — bản port của `tests/test_photo_core.py` cũ.
- `rust/gui/tests/cpu_render.rs` (`cargo test -p pbe-gui`): framebuffer trộn alpha, ghép tài liệu, crop +
  xoay 90° + lật, undo/redo, `layer_set_bounds_resizes_image_and_blur_layers`, Unsharp Mask ở mọi tỉ lệ,
  `background_can_move_above_layers_and_back` (nền lên trên / giữa, lộ layer ở phần đã tách nền, opacity nền,
  undo), `reorder_delete_duplicate_keep_background_slot`, `new_layers_go_right_above_the_selection` (+ đổi tên),
  `clicking_picks_the_top_layer_with_pixels`, Tem nhân bản, làm mịn da + `BaseCache`.
- `rust/gui/src/photo/mod.rs` (`#[cfg(test)]`): `resize_from_corner` giữ góc đối diện, Shift giữ tỉ lệ.
- Thủ công: `cargo run -p pbe-gui` → trang Sửa ảnh đơn: thử từng nút thanh công cụ dọc + phím V C B I L H Z,
  menu File / Edit / Image / View, kéo nút góc layer (có / không Shift), công cụ Zoom (bấm, Alt + bấm);
  mở ảnh lớn (> 20 MP), lăn zoom, Space+kéo,
  chèn PNG trong suốt, che mặt bằng pixelate, cắt 16:9, Ctrl+Z nhiều lần, xuất JPEG/PNG; chèn nhiều ảnh một lần
  (hộp chọn file + kéo thả từ Finder), nhấp đúp đổi tên, bấm vào chỗ trong suốt của PNG phải chọn layer dưới; đổi
  ảnh nào đè lên ảnh nào (▲ ▼, kéo thả, Ctrl+] / Ctrl+[, chuột phải), tách nền rồi kéo ảnh chèn xuống dưới Nền.
  (Layer chữ và `.pbep` chưa port.)
