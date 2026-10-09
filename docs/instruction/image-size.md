# Image Size

- **Mục đích:** đổi kích thước ảnh giống hộp thoại Image Size của Photoshop.
- **File:** `rust/core/src/image_size.rs`, `rust/core/src/resize.rs`
- **Changelog:** [../changelog/image-size.md](../changelog/image-size.md)
- **Ghi chú:** mã Python (`core/image_size.py`) đã xoá; bản triển khai hiện tại là Rust.

## Logic chính

- Đổi kích thước với các phương pháp resample kiểu Photoshop.

## Test

`rust/core/src/image_size.rs` + `resize.rs` (`#[cfg(test)]`) + fixture trong `rust/core/tests/fixtures/` (bản port của `tests/test_image_size.py` cũ).
