# Port sang Rust

- **Mục đích:** Chuyển toàn bộ app (hiện viết Python + PyQt6) sang Rust, **giữ nguyên tính năng**. Làm
  theo giai đoạn để mỗi bước đều build được và `cargo test` xanh, tránh viết lại một lần rồi không chạy.
- **File:** mã Rust nằm trong thư mục `rust/` (Cargo workspace: crate `core` = `pbe-core`, crate `gui` =
  `pbe-gui` + binary `photo-batch-editor`), không đụng tới code Python đang chạy cho tới khi phần Rust
  tương ứng đã xong và test khớp.
- **Changelog:** [../changelog/rust-port.md](../changelog/rust-port.md)

## Logic chính

- **Vì sao theo giai đoạn:** app là desktop PyQt6 ~20.500 dòng; GUI phải viết lại hoàn toàn (Qt không map
  1-1 sang Rust). Rủi ro lớn nhất nằm ở GUI và các phần phụ thuộc thư viện C (OpenCV, ONNX Runtime, PIL,
  FFmpeg). Vì vậy port `core/` (logic thuần, test được) trước; GUI chọn framework và làm sau.
- **Workspace Rust:** `rust/` chứa crate thư viện `pbe-core` (tương ứng `core/` của Python) và crate
  `pbe-gui` (tương ứng `ui/`): `pbe-gui` là **lib + bin** — lib để test được phần CPU (framebuffer, ghép
  tài liệu, xem trước), bin (`photo-batch-editor`) chỉ mở cửa sổ.
- **Chuẩn hành vi phải khớp:** chính là bộ `tests/` của Python. Mỗi module Rust đi kèm `#[cfg(test)]` phản
  chiếu các test tương ứng (cùng input → cùng kết quả trong sai số mà test Python cho phép).

### Thứ tự port core/ (dễ → khó)

| Nhóm | Module Python | Rust | Trạng thái | Ghi chú |
|---|---|---|---|---|
| Pure logic | `version`, `system`, `paths` | `version.rs`, `system.rs`, `paths.rs` | ✅ Xong (GĐ1) | JSON, hằng số, đường dẫn theo OS |
| Pure logic | `settings` | `settings.rs` | ✅ Xong (GĐ1) | SliderSpec, clamp, from_dict/to_dict, JSON preset |
| Pure logic | `scanner` | `scanner.rs` | ✅ Xong (GĐ1) | quét thư mục, đặt tên output, chống trùng tên |
| Crypto | `auth` | `auth.rs` | ✅ Xong (GĐ1) | PBKDF2-HMAC-SHA256, auth.json |
| i18n | `i18n`, `i18n_vi` | `i18n.rs`, `i18n_vi.rs` | ✅ Xong (GĐ1) | catalog EN→VI, format, regex MESSAGE_PATTERNS |
| Preset | `presets` | `presets.rs` | ✅ Xong (GĐ1) | phụ thuộc settings + i18n + paths |
| Image size | `image_size` (target/clamp + resample) | `image_size.rs`, `resize.rs` | ✅ Xong (GĐ1+2) | target/clamp/predict + resample pixel (bilinear/area/cubic/lanczos/nearest) |
| Numeric | `backend` (blur/box/down/up), `adjustments`, `pipeline` | `backend.rs`, `adjustments.rs`, `pipeline.rs` | ✅ Xong (GĐ2) | khớp OpenCV trong sai số test; impl thủ công |
| I/O | `io_utils` (decode/encode/EXIF) | `io_utils.rs` | ✅ Xong (GĐ3) | crate `image` + `jpeg-encoder` + `kamadak-exif`; đọc 8/16-bit, alpha, xoay EXIF; ghi JPEG q100 4:4:4 + EXIF(reset)+ICC |
| SR | `enhance` (ONNX) | `enhance.rs` (feature `sr`) | ✅ Xong (GĐ3) | crate `ort` (ONNX Runtime) + model realesr; khớp Python ≤ 2/255 |
| Batch | `batch` | `batch.rs` | ✅ Xong (GĐ3) | đa luồng CPU (thread pool), process_log, hủy; nhánh GPU không port (máy dev không có) |
| Photo core | `core/photo/*` | `photo/{mod,effects,geometry,history,collage,image_io}.rs` | ✅ Xong (GĐ4) | `RgbaImage`, blur/pixelate, zoom/crop/xoay, undo/redo, mẫu collage, đọc/ghi ảnh giữ alpha |
| Video core | `core/video/*` | `video/{mod,ffmpeg,project,export}.rs` | ✅ Xong (GĐ4) | tìm FFmpeg, parse probe, chạy kèm tiến độ/huỷ, timeline, dựng `-filter_complex` |
| GUI | toàn bộ `ui/` | crate `gui/` (egui + wgpu) | 🟡 Phần lớn (GĐ5) | xem bảng GĐ5 bên dưới |

**GĐ1–4 đã hoàn tất:** `cargo test` xanh (95 unit test + 26 integration test) + `cargo test --features sr`
(thêm 1 test SR ONNX). Mỗi module kèm test phản chiếu test Python.

- Dữ liệu i18n tiếng Việt **sinh tự động** từ `core/i18n_vi.py` → `rust/core/src/i18n_vi_data.json`
  (nhúng bằng `include_str!`), để khớp 100%.
- Số học ảnh được chứng minh tương đương bằng **fixture tham chiếu** sinh từ bản Python (NumPy/OpenCV/
  ONNX): chạy `venv/bin/python tools/gen_rust_fixtures.py` (backend + adjustments + pipeline) và
  `venv/bin/python tools/gen_io_fixtures.py` (ảnh + thư mục batch). Fixture nằm ở
  `rust/core/tests/fixtures/`. Test Rust nạp cùng input, tự tính, so với output Python trong sai số test.
- Super Resolution để sau feature `sr` (mặc định tắt) nên `core` không kéo ONNX runtime khi không cần:
  `cargo test --features sr`. `pipeline::process` nhận `Option<&dyn Upscaler>`; `enhance::SuperResolver`
  là cài đặt. Nhánh GPU (CuPy) và resample khớp-từng-bit với OpenCV cho cubic/lanczos là phần tinh chỉnh
  về sau (test Python của resample chỉ kiểm tra shape/khoảng giá trị).

### GĐ4 — `core/photo/` + `core/video/`

- **`photo::RgbaImage`** thay cho `np.ndarray (H, W, 4)` của Python: một `Vec<u8>` liền nhau theo thứ tự
  `R,G,B,A` (chưa premultiply) để copy thẳng sang texture GPU. Kèm `crop` / `paste` / `px` / `set_px`.
- **`photo::effects`**: `gaussian_blur` blur trên **màu đã premultiply** rồi un-premultiply (không tạo
  viền tối ở chỗ trong suốt), `pixelate` gióng ô theo lưới của **cả ảnh** (`origin`) nên di chuyển layer
  Blur không làm ô nhảy. `PhotoAdjust` (5 thanh trượt) map sang `AdjustmentSettings` → dùng **đúng** 15
  chỉnh sửa của trình chỉnh nhiều ảnh.
- **`backend::gaussian_blur_exact`** (mới): blur luôn ở độ phân giải gốc, khớp đúng
  `cv2.GaussianBlur(BORDER_REFLECT)`. `backend::gaussian_blur` cũ chạy sigma lớn trên bản thu nhỏ cho
  nhanh — chỉ phù hợp mặt nạ tông màu, **không** dùng cho hiệu ứng nhìn thấy.
- **`adjustments::apply_adjustments_planar_u8_ls`** (mới): ghi đè `long_side` để bán kính sharpening /
  clarity trông như nhau ở mọi cỡ xem trước (tương ứng tham số `long_side` của Python).
- **`photo::history::History<T>`** generic như bản Python, gộp các thay đổi cùng `key` trong 1 giây;
  `push_at(..., now)` để test tất định.
- **`video::ffmpeg`**: thứ tự tìm `ffmpeg` = `PBE_FFMPEG` → `<app>/ffmpeg/ffmpeg` → cạnh file thực thi →
  `PATH` (bỏ bước `imageio-ffmpeg` vì Rust không có gói đó). `run_ffmpeg` đọc `-progress pipe:1` trên
  luồng chính, log lỗi (25 dòng cuối) trên luồng riêng, và một luồng canh `CancelFlag` để `kill` tiến trình.
- **`video::export`** dựng `-filter_complex` **giống từng ký tự** với bản Python (test so khớp chuỗi), ghi
  ra `<tên>.tmp.<ext>` rồi đổi tên, từ chối ghi đè file nguồn.

### GĐ5 — GUI (`rust/gui/`)

- **Framework:** `eframe`/`egui` 0.36 với renderer **wgpu**. Theme `theme.rs` port `ui/theme.qss` (giữ y
  nguyên bảng màu "midnight") sang `egui::Visuals`.
- **Canvas = framebuffer CPU + texture wgpu** (`fb.rs`): canvas photo editor và khung xem trước video
  **không** vẽ bằng widget mà được ghép bằng CPU vào mảng RGBA rồi tải lên một texture wgpu. Nhờ vậy pixel
  trên màn hình đúng bằng pixel ghi ra file (cùng code `pbe_core::photo`), và zoom/pan dùng đúng
  `photo::geometry`. *Crate `pixels` không được dùng trực tiếp vì nó tự quản surface + swapchain của cửa
  sổ, không dùng chung cửa sổ với `eframe`; kiến trúc giữ nguyên (framebuffer CPU → wgpu).*
- **Thư mục dữ liệu dùng chung với bản Python:** `paths::app_data_dir()` (mới) =
  `%APPDATA%\PhotoBatchEditor` / `~/Library/Application Support/PhotoBatchEditor` /
  `$XDG_DATA_HOME/PhotoBatchEditor`, đúng `QStandardPaths.AppDataLocation` — nên `auth.json`, `state.json`
  và preset dùng chung giữa hai bản. (`local_appdata_dir()` vẫn là chỗ đệm.)

| Trang | File Python | File Rust | Trạng thái |
|---|---|---|---|
| Vỏ app + điều hướng + state.json | `ui/main_window.py`, `ui/sidebar.py` | `app.rs`, `sidebar.rs` | ✅ |
| Đăng nhập | `ui/login_view.py` | `login.rs` | ✅ |
| Chỉnh nhiều ảnh (3 màn hình) | `ui/prepare_view.py`, `run_view.py`, `results_view.py`, `file_list.py` | `batch.rs` | ✅ |
| Bảng 15 chỉnh sửa + SR + Image Size | `ui/adjustment_panel.py` | `adjust.rs` | ✅ |
| Xem trước trước/sau (luồng nền) | `ui/preview.py`, `ui/workers.py` | `preview.rs` | ✅ |
| Sửa ảnh đơn: canvas, zoom/pan, crop, xoay/lật, layer ảnh, layer blur, undo/redo, xuất | `ui/photo/*` | `photo/{mod,doc,render}.rs` | ✅ |
| Sửa video: timeline, cắt/xoá/đổi chỗ/trim, In/Out, tắt tiếng, nhạc, xuất kèm tiến độ/huỷ | `ui/video/*` | `video.rs` | ✅ |
| Settings: preset, tài khoản, ngôn ngữ | `ui/settings_view.py` | `settings_view.rs` | ✅ |
| **Layer chữ** (photo editor) + **vẽ chữ lên video** | `ui/photo/render.py`, `ui/video/text_render.py` | — | ⬜ còn thiếu |
| **Hộp thoại collage** (logic `collage.rs` đã có) | `ui/photo/collage_dialog.py` | — | ⬜ còn thiếu |
| **File dự án `.pbep`** | `ui/photo/project_io.py` | — | ⬜ còn thiếu |
| Phát video có tiếng trong app | `ui/video/player.py` | — | ⬜ còn thiếu (xem trước là khung tĩnh lấy bằng FFmpeg) |
| Trình xem ảnh toàn màn hình, thông báo khay, thumbnail danh sách | `ui/image_viewer.py`, `notifier.py`, `icons.py` | — | ⬜ còn thiếu |
| Chọn GPU / CPU, dò GPU | `ui/device_status.py` | — | ⬜ không port (nhánh GPU CuPy không port, xem GĐ3) |
| Live reload khi dev | `ui/devtools.py` | — | ⬜ không cần (`cargo run` đủ nhanh) |

## Lưu ý / giới hạn

- Khó khớp tuyệt đối pixel với OpenCV/PIL: các test số học của Python dùng sai số (vd. mean ≤ 1/255), bản
  Rust phải cài cùng công thức để lọt sai số đó, không kỳ vọng giống từng bit.
- `enhance` cần model ONNX `models/realesr-general-x4v3.onnx` (đã có trong repo) + runtime ONNX cho Rust.
- Toolchain: Rust stable (rustup). Linker dùng clang hệ thống (macOS). Build: `cargo build`, test: `cargo test`
  trong thư mục `rust/`.
- **Chữ (text) là phần chặn chính của GUI:** vẽ chữ có dấu tiếng Việt lên pixel cần một bộ rasterize font
  trong Rust (`cosmic-text` + font hệ thống). Vì vậy layer chữ của photo editor và việc vẽ chữ lên video
  **chưa** port; dữ liệu chữ của dự án video vẫn giữ (thêm / sửa / xoá được), chỉ không xuất ra video.
- GUI đọc / ghi cùng `auth.json`, `state.json`, thư mục `presets` với bản Python. Chạy **song song** hai bản
  cùng lúc có thể ghi đè state của nhau — nên đóng bản kia trước khi chạy bản này.
- Xem trước video là **khung tĩnh** lấy bằng `ffmpeg -ss <t> -frames:v 1` trên luồng nền (chưa phát có tiếng).
- Nhánh GPU (CuPy) không port → GUI Rust luôn chạy CPU; chưa có phần chọn thiết bị.

## Test

- `cd rust && cargo test` — phải xanh cho các module đã port.
- `cargo test -p pbe-gui` — test phần **CPU** của GUI (`gui/tests/cpu_render.rs`): framebuffer trộn alpha /
  kẹp biên / vẽ ảnh theo opacity, thu nhỏ ảnh xem trước, ghép tài liệu ở nửa tỉ lệ và tỉ lệ thật, chỉnh
  Brightness làm ảnh sáng hơn, layer blur chỉ ảnh hưởng trong khung của nó, ẩn layer / opacity layer,
  crop + xoay 90° + lật dời layer theo canvas, undo/redo trả lại đúng pixel, xoay tự do phủ kín khung.
  Phần vẽ widget của egui không test tự động.
- `cargo test --test video_export` — xuất video thật bằng FFmpeg (probe, cắt + nhạc + overlay, tắt tiếng,
  huỷ không để lại file). Tự bỏ qua khi máy không có FFmpeg.
- Chạy thử GUI: `cd rust && cargo run -p pbe-gui`.
- Đối chiếu từng test Rust với test Python tương ứng trong `tests/` (vd. `tests/test_settings`→ không có,
  nằm trong `test_presets.py`/`test_image_size.py`; `tests/test_scanner.py`; `tests/test_auth.py`;
  `tests/test_i18n.py`; `tests/test_version.py`).
