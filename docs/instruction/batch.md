# Chỉnh nhiều ảnh (batch)

- **Mục đích:** áp cùng một bộ chỉnh sửa cho cả thư mục ảnh.
- **File:** `rust/core/src/batch.rs`, `rust/core/src/scanner.rs`, `rust/core/src/pipeline.rs`,
  `rust/core/src/io_utils.rs`, `rust/gui/src/batch.rs`
- **Changelog:** [../changelog/batch.md](../changelog/batch.md)
- **Ghi chú:** mã Python đã xoá; logic → `rust/core/src/batch.rs`; GUI 3 màn hình → `rust/gui/src/batch.rs`. Nhánh GPU không port (luôn chạy CPU). Chi tiết: [rust-port.md](rust-port.md).

## Logic chính

- Quét thư mục (kể cả thư mục con) → ghi kết quả ra thư mục anh em `<folder>_update`,
  giữ nguyên tên file và cấu trúc thư mục con. Ảnh gốc không bao giờ bị sửa.
- Xuất JPEG quality 100, 4:4:4.
- CPU: process pool. GPU: pipeline 3 giai đoạn (đọc → xử lý → ghi).
- Mỗi lần chạy ghi `process_log.txt` vào thư mục output.
- Có CLI: `python -m core.batch <folder> --preset ... --device gpu|cpu --if-exists overwrite`.
- Trang Results: danh sách kết quả + viewer before/after lớn.
- Chuột phải một ảnh (danh sách ẢNH ở trang chuẩn bị hoặc bảng Kết quả) → "Mở trong Sửa ảnh đơn"
  (`FileList.open_in_photo_editor`, `ResultsView.open_in_photo_editor` → `MainWindow.open_in_photo_editor`).
  Hành vi hàng loạt không đổi. Thanh bên gọi trang này là "Sửa hàng loạt" (Ctrl+1).

## Lưu ý / giới hạn

- Đọc ảnh hỗ trợ đường dẫn Unicode và xoay theo EXIF orientation (`core/io_utils.py`).

## Test

`tests/test_batch.py`, `tests/test_scanner.py`, `tests/test_io_utils.py`
