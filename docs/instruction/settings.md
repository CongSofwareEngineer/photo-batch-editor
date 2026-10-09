# Setting (preset) & trang Settings

- **Mục đích:** lưu và quản lý nhiều bộ chỉnh sửa để dùng lại.
- **File:** `core/presets.py`, `core/paths.py`, `presets_builtin/`, `ui/settings_view.py`,
  `ui/main_window.py`, `ui/sidebar.py`
- **Changelog:** [../changelog/settings.md](../changelog/settings.md)
- **Bản Rust:** logic → `rust/core/src/{settings,presets}.rs`; trang Settings → `rust/gui/src/settings_view.rs` (dùng chung thư mục `presets` với bản Python). Chi tiết: [rust-port.md](rust-port.md).

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
