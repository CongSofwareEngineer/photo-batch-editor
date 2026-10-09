# Đăng nhập (Login)

- **Mục đích:** yêu cầu đăng nhập cục bộ khi mở app.
- **File:** `rust/core/src/auth.rs`, `rust/gui/src/login.rs`, `rust/gui/src/sidebar.rs`, `rust/gui/src/settings_view.rs`
- **Changelog:** [../changelog/login.md](../changelog/login.md)
- **Ghi chú:** mã Python (`core/auth.py`, `ui/login_view.py`…) đã xoá; logic → `rust/core/src/auth.rs`; trang đăng nhập → `rust/gui/src/login.rs`. Chi tiết: [rust-port.md](rust-port.md).

## Logic chính

- Lần đầu mở app: tài khoản mặc định `admin` / `admin`. Đổi trong Settings › Account.
- Mật khẩu lưu dạng hash PBKDF2 có salt trong `%APPDATA%\PhotoBatchEditor\auth.json`
  (gồm user name, hash, cờ "keep me signed in").
- "Keep me signed in" → lần sau mở thẳng app, bỏ qua màn đăng nhập.
- "Sign out" ở sidebar → yêu cầu nhập lại mật khẩu.
- Màn đăng nhập có chọn ngôn ngữ.

## Lưu ý / giới hạn

- Xoá `auth.json` để reset tài khoản về `admin` / `admin`.

## Test

`tests/test_auth.py`
