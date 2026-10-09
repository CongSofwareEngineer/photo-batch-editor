# Format / lint code (Rust: rustfmt + clippy)

- **Mục đích:** code Rust theo một chuẩn chung, format và bắt lỗi nhất quán giữa các máy.
- **File:** `rust/` (Cargo workspace), `.vscode/settings.json`, `.vscode/extensions.json`
- **Changelog:** [../changelog/code-format.md](../changelog/code-format.md)

## Logic chính

- Bản Python trước đây dùng **Ruff + Pyright** (rules trong `pyproject.toml`, chạy bằng `lint.bat` /
  `lint.sh`) — đã **bị xoá** cùng bản Python.
- Bản Rust dùng bộ công cụ chuẩn của Rust, không cần file cấu hình riêng (dùng mặc định):
  - **`cargo fmt`** — formatter (`rustfmt`), đổi định dạng code về chuẩn.
  - **`cargo clippy --all-targets`** — linter, bắt các đoạn code thừa / dễ sai.
- VS Code: dùng extension **rust-analyzer** (xem `.vscode/extensions.json`); format khi lưu nếu muốn.
- Build/test vẫn là nguồn kiểm tra chính: `cargo test` phải xanh.

## Lưu ý / giới hạn

- Không có `rustfmt.toml` / cấu hình clippy riêng — giữ mặc định.
- `cargo clippy` cần cài component: `rustup component add clippy rustfmt`.
- Thư mục loại trừ mặc định: `target/`.
- **Hiện trạng:** code Rust trong repo **chưa** sạch `cargo fmt` (cả `core` lẫn `gui`) — chạy `cargo fmt`
  sẽ tạo diff lớn. Cân nhắc format toàn bộ một lần rồi commit riêng.

## Test

- `cd rust && cargo test` — xanh (là nguồn kiểm tra chính).
- `cd rust && cargo fmt` — format code (hiện chưa chạy toàn bộ).
- `cd rust && cargo clippy --all-targets` — bắt lỗi/lint.
