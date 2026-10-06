# Log: Setting (preset) & trang Settings

- **Mục đích:** lưu và quản lý nhiều bộ chỉnh sửa để dùng lại.
- **File:** `core/presets.py`, `core/paths.py`, `presets_builtin/`, `ui/settings_view.py`,
  `ui/main_window.py`, `ui/sidebar.py`

## Logic chính

- Mở trang Settings từ sidebar (Ctrl+4; trước 03/10/2026 là Ctrl+2).
- Tạo setting: trống / từ chỉnh sửa hiện tại của Editor / từ template có sẵn / import `.json`.
- Sửa có live preview trên ảnh mẫu; đổi tên, nhân bản, export, xoá. Mọi thay đổi tự lưu.
- Chọn setting ở ô "Setting" của Editor (hoặc nút "Use in Editor") để áp dụng.
- Lưu trữ:
  - Preset người dùng: `%APPDATA%\PhotoBatchEditor\presets\*.json`
  - Trạng thái (setting cuối, thư mục, thiết bị, setting đang chọn, ảnh mẫu):
    `%APPDATA%\PhotoBatchEditor\state.json`
- Settings › General: chọn ngôn ngữ (xem `i18n.md`). Settings › Account: đổi mật khẩu
  (xem `login.md`).

## Lưu ý / giới hạn

- Preset người dùng và settings được giữ lại khi gỡ cài đặt.

## Test

`tests/test_presets.py`

## Lịch sử thay đổi

- 2026-10-03: Ghi lại hiện trạng ban đầu.
- 2026-10-03: Phím tắt trang Settings đổi Ctrl+2 → Ctrl+4 — vì thêm trang Sửa ảnh đơn (Ctrl+2) và Sửa video (Ctrl+3).
- 2026-10-03: `load_settings_json`: tên lấy từ `name` nếu là chuỗi, ngược lại tên file (viết lại cho Pyright, không đổi
  hành vi) — xem code-format.md.
