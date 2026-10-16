# Changelog: Hỗ trợ macOS (MacBook)

> Hiện trạng logic: [../instruction/macos.md](../instruction/macos.md)

- 2026-10-03: Thêm hỗ trợ macOS (requirements marker, core/system.py, Finder, font, build_mac.sh,
  dev.sh, spec BUNDLE) — người dùng muốn chạy app trên MacBook.
- 2026-10-06: Tách file log cũ `docs/macos.md` thành `instruction/` (hiện trạng) + `changelog/` (lịch sử) — theo rule mới trong CLAUDE.md.
- 2026-10-09 | Sửa | Hỗ trợ macOS: viết lại theo Rust (`rust/core/src/system.rs`, `paths.rs`, `build_rust_mac.sh`, cargo-bundle) và bỏ `PhotoBatchEditor.spec` / `build_mac.sh` / `tools/make_icns.py` — vì mã Python đã bị xoá.
- 2026-10-16 | Sửa | Build macOS (`build_rust_mac.sh`): tăng `build` trong `version.json` mỗi lần build, ghi version vào `Info.plist`, chép `version.json` + `presets_builtin` vào `Contents/Resources` (trước thiếu nên `.app` dùng đường dẫn lúc compile), ký Developer ID + hardened runtime + notarize nếu có `PBE_MAC_SIGN_IDENTITY` / `PBE_NOTARY_PROFILE` (không có thì ad-hoc), xuất `installer_output/PhotoBatchEditor-X.Y.Z.build.dmg`; bỏ `resources` trong `[package.metadata.bundle]` (trỏ tới `README.txt` không tồn tại) — làm bản Mac giống bản Windows: version tăng mỗi build và có chữ ký để máy khác mở không bị chặn.
