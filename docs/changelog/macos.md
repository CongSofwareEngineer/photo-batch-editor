# Changelog: Hỗ trợ macOS (MacBook)

> Hiện trạng logic: [../instruction/macos.md](../instruction/macos.md)

- 2026-10-03: Thêm hỗ trợ macOS (requirements marker, core/system.py, Finder, font, build_mac.sh,
  dev.sh, spec BUNDLE) — người dùng muốn chạy app trên MacBook.
- 2026-10-06: Tách file log cũ `docs/macos.md` thành `instruction/` (hiện trạng) + `changelog/` (lịch sử) — theo rule mới trong CLAUDE.md.
- 2026-10-09 | Sửa | Hỗ trợ macOS: viết lại theo Rust (`rust/core/src/system.rs`, `paths.rs`, `build_rust_mac.sh`, cargo-bundle) và bỏ `PhotoBatchEditor.spec` / `build_mac.sh` / `tools/make_icns.py` — vì mã Python đã bị xoá.
