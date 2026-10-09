# Port sang Rust

- **Mục đích:** Chuyển toàn bộ app (hiện viết Python + PyQt6) sang Rust, **giữ nguyên tính năng**. Làm
  theo giai đoạn để mỗi bước đều build được và `cargo test` xanh, tránh viết lại một lần rồi không chạy.
- **File:** mã Rust nằm trong thư mục `rust/` (Cargo workspace), không đụng tới code Python đang chạy cho tới
  khi phần Rust tương ứng đã xong và test khớp.
- **Changelog:** [../changelog/rust-port.md](../changelog/rust-port.md)

## Logic chính

- **Vì sao theo giai đoạn:** app là desktop PyQt6 ~20.500 dòng; GUI phải viết lại hoàn toàn (Qt không map
  1-1 sang Rust). Rủi ro lớn nhất nằm ở GUI và các phần phụ thuộc thư viện C (OpenCV, ONNX Runtime, PIL,
  FFmpeg). Vì vậy port `core/` (logic thuần, test được) trước; GUI chọn framework và làm sau.
- **Workspace Rust:** `rust/` chứa crate thư viện `pbe-core` (tương ứng `core/` của Python). Các crate GUI /
  video sẽ thêm ở giai đoạn sau.
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
| Photo/Video core | `core/photo/*`, `core/video/*` | — | ⬜ GĐ4 | effects, collage, geometry, history, ffmpeg, project |
| GUI | toàn bộ `ui/` | — | ⬜ GĐ5 | chọn framework (Slint / egui / Tauri) — quyết sau |

**GĐ1–3 đã hoàn tất:** `cargo test` xanh (64 test mặc định) + `cargo test --features sr` (thêm 1 test SR
ONNX). Mỗi module kèm test phản chiếu test Python.

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

## Lưu ý / giới hạn

- Khó khớp tuyệt đối pixel với OpenCV/PIL: các test số học của Python dùng sai số (vd. mean ≤ 1/255), bản
  Rust phải cài cùng công thức để lọt sai số đó, không kỳ vọng giống từng bit.
- `enhance` cần model ONNX `models/realesr-general-x4v3.onnx` (đã có trong repo) + runtime ONNX cho Rust.
- Toolchain: Rust stable (rustup). Linker dùng clang hệ thống (macOS). Build: `cargo build`, test: `cargo test`
  trong thư mục `rust/`.

## Test

- `cd rust && cargo test` — phải xanh cho các module đã port.
- Đối chiếu từng test Rust với test Python tương ứng trong `tests/` (vd. `tests/test_settings`→ không có,
  nằm trong `test_presets.py`/`test_image_size.py`; `tests/test_scanner.py`; `tests/test_auth.py`;
  `tests/test_i18n.py`; `tests/test_version.py`).
