# Log: Sửa ảnh đơn (Photo editor)

- **Mục đích:** trang "Sửa ảnh đơn" kiểu Photoshop đơn giản: mở 1 ảnh, thêm layer chữ / ảnh /
  vùng làm mờ, cắt, xoay / lật, chỉnh màu, hoàn tác ≥ 30 bước, xuất JPEG/PNG, lưu file dự án
  `.pbep` (giữ layer) để mở lại sửa tiếp. Ảnh gốc không bao giờ bị ghi đè.
- **File:**
  - `core/photo/` (không import Qt): `history.py` (undo/redo snapshot), `geometry.py` (zoom 10–1600 %,
    zoom tại con trỏ, khung crop theo tỉ lệ, xoay 90° / lật toạ độ, cover_scale cho xoay tự do),
    `effects.py` (blur / pixelate NumPy-OpenCV, `PhotoAdjust` → `AdjustmentSettings`),
    `image_io.py` (đọc RGBA giữ trong suốt, ghi JPEG/PNG, thư mục `editor`), `collage.py` (xem collage.md)
  - `ui/photo/`: `document.py` (Document + TextLayer / ImageLayer / BlurLayer + Background),
    `render.py` (vẽ bằng QPainter, cache nền đã chỉnh + cache vùng mờ), `canvas.py` (zoom / pan),
    `tools.py` (Select, Move, Text, Crop, Blur), `panels.py` (bảng Layer + thuộc tính),
    `project_io.py` (`.pbep`), `photo_editor_view.py` (trang, menu trên, thanh trạng thái, export)
  - Dùng chung: `ui/main_window.py`, `ui/sidebar.py` (trang `photo`, Ctrl+2), `ui/icons.py` (icon công cụ),
    `ui/theme.qss` (mục "Photo / video editors"), `core/i18n_vi.py`

## Logic chính

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
- Phím chữ cái (V, M, C…) chỉ là phím tắt khi không gõ trong ô nhập; Ctrl+1 của trang Hàng loạt tạm tắt khi
  đang ở trang này (nhường cho zoom 100 %).

## Test

- `tests/test_photo_core.py`: history (40 bước, gộp key, giới hạn), zoom tại con trỏ, crop theo tỉ lệ,
  xoay/lật toạ độ, cover_scale, pixelate / blur, chỉnh ảnh = đúng pipeline batch, đọc RGBA, ghi JPEG/PNG,
  đường dẫn `editor`, đường dẫn tiếng Việt.
- `tests/test_photo_editor.py` (Qt offscreen): vẽ chữ tiếng Việt, blur / pixelate / cọ, ẩn layer, 35 bước undo,
  crop / xoay / lật dời layer, bản xem trước + bản đầy đủ chạy nền, `.pbep` lưu/mở giống hệt, zoom giữ điểm
  dưới con trỏ, kéo chọn / đổi cỡ (Shift) / xoay, crop 1:1 + Enter, công cụ làm mờ, mở ảnh + xuất vào `editor`.
- Thủ công: `dev.bat` → trang Sửa ảnh đơn (Ctrl+2): mở ảnh lớn (> 20 MP), Ctrl+lăn zoom, Space+kéo, thêm chữ có
  dấu, chèn PNG trong suốt, che mặt bằng pixelate, cắt 16:9, Ctrl+Z 30 lần, xuất JPEG/PNG, lưu và mở lại `.pbep`.

## Lịch sử thay đổi

- 2026-10-03: Tạo file log trước khi code (kế hoạch).
- 2026-10-03: Hoàn thành GĐ1–GĐ3: khung trang + canvas zoom/pan + layer + undo; chữ, crop, chèn ảnh, làm mờ /
  pixelate, chỉnh ảnh, xoay/lật, xuất JPEG/PNG, file dự án `.pbep`; mở từ tab hàng loạt bằng chuột phải.
  Thanh điều hướng thêm trang: Sửa hàng loạt (Ctrl+1), Sửa ảnh đơn (Ctrl+2), Sửa video (Ctrl+3),
  Cài đặt (Ctrl+4 — trước là Ctrl+2).
- 2026-10-03: Font mặc định của layer chữ theo hệ điều hành (`core.system.DEFAULT_FONT_FAMILY`: Segoe UI / Helvetica
  Neue trên Mac) — hỗ trợ macOS, xem macos.md.
- 2026-10-03: Sửa lỗi kiểu do Pyright báo (không đổi hành vi): `QPolygonF.at(i)` thay `poly[i]`, `enterEvent(QEnterEvent)`,
  `edit_layer(..., None, **{field: ...})` khi lật; `'PNG'` trong `project_io` giữ str (stub PySide6 sai) — xem code-format.md.
