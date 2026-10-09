# Changelog: Format / lint / type check code (Ruff + Pyright) — "ESLint cho Python"

> Hiện trạng logic: [../instruction/code-format.md](../instruction/code-format.md)

- 2026-10-03: Thêm Ruff + format-on-save, format toàn bộ code một lần — người dùng muốn "eslint"
  tự format khi lưu.
- 2026-10-03: Mở rộng như ESLint — thêm rule khoảng trắng (preview), thứ tự import (`I`), nháy đơn
  (`quote-style = "single"`, đổi 4855 chuỗi), `UP/C4/SIM/PIE/RET/T10`; thêm Pyright (Pylance) báo
  error khi code sai, sửa 134 → 0 lỗi kiểu có sẵn (chủ yếu khai báo kiểu; `timeline.scroll` →
  `view_start` vì đè method `QWidget.scroll`); `lint` chạy cả pyright; build chạy lint trước test.
- 2026-10-06: Tách file log cũ `docs/code-format.md` thành `instruction/` (hiện trạng) + `changelog/` (lịch sử) — theo rule mới trong CLAUDE.md.
- 2026-10-09 | Sửa | Format / lint: `tools/gen_io_fixtures.py` cắt dòng `record(f'orient_{o}', …)` theo Ruff và bỏ `from PIL import Image` không dùng — vì `./lint.sh --check` đang báo 1 file cần format + 1 lỗi import (file này được thêm ở commit trước mà chưa chạy lint).
