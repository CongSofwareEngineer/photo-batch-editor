# Log: Chỉnh nhiều ảnh (batch)

- **Mục đích:** áp cùng một bộ chỉnh sửa cho cả thư mục ảnh.
- **File:** `core/batch.py`, `core/scanner.py`, `core/pipeline.py`, `core/io_utils.py`,
  `ui/prepare_view.py`, `ui/run_view.py`, `ui/results_view.py`, `ui/file_list.py`,
  `ui/image_viewer.py`, `ui/workers.py`

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

## Lịch sử thay đổi

- 2026-10-03: Ghi lại hiện trạng ban đầu.
- 2026-10-03: Thêm mục chuột phải "Mở trong Sửa ảnh đơn" (input + kết quả); đổi tên mục điều hướng thành
  "Sửa hàng loạt" — vì có thêm trang Sửa ảnh đơn / Sửa video (xem photo-editor.md, video-editor.md).
- 2026-10-03: Sửa lỗi kiểu do Pyright báo (không đổi hành vi): CLI `reconfigure` stdout qua `getattr`; danh sách kết
  quả / trình xem ảnh bỏ qua dòng không có `output_path` khi tạo thumbnail / preload — xem code-format.md.
