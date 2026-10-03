# Log: Format / lint / type check code (Ruff + Pyright) — "ESLint cho Python"

- **Mục đích:** code Python theo một chuẩn chung, báo lỗi ngay khi gõ như ESLint: cảnh báo dư khoảng
  trắng / import thừa / import sai thứ tự / nháy đôi, báo **error** khi code sai; lưu file là tự format
  (nháy đơn) và tự sửa.
- **File:** `pyproject.toml` (`[tool.ruff]`, `[tool.pyright]`), `.vscode/settings.json`,
  `.vscode/extensions.json`, `requirements-dev.txt` (ruff, pyright), `lint.bat`, `lint.sh`,
  `build_windows.bat`, `build_mac.sh` (bước Lint)

## Logic chính

- ESLint chỉ đọc JS/TS. Với Python dùng 2 công cụ:
  - **Ruff** = ESLint + Prettier: format và lint. Trong VS Code báo **vàng (warning)**; riêng lỗi cú pháp
    và tên chưa định nghĩa (`F821`) báo **đỏ (error)**.
  - **Pyright** (chính là Pylance của VS Code) = kiểm tra kiểu: báo **đỏ** khi gọi thuộc tính/hàm
    không tồn tại, sai số tham số, import không có, biến chưa gán, sai kiểu trả về. Sai kiểu tham số
    → vàng (stub PySide6 đôi khi sai).
- **Rule Ruff** (`select`): `E` + `W` (khoảng trắng, dòng trống, thụt lề — bật `preview` để có các rule
  khoảng trắng E1xx/E2xx/E3xx), `F` (import/biến thừa, tên chưa định nghĩa), `I` (thứ tự import),
  `Q` (nháy đơn), `B` (bug hay gặp), `UP`, `C4`, `SIM`, `PIE`, `RET`, `T10` (quên `breakpoint()`).
  Bỏ qua: `E501` (dòng dài — formatter tự xuống dòng), `SIM105/108/112`, `C408`, `RET504-506`, `B008`, `Q003`.
- **Nháy đơn:** `quote-style = "single"` + `Q000`. Ngoại lệ có chủ đích: chuỗi chứa `'` giữ `"…"` để
  không phải escape; docstring và chuỗi nhiều dòng giữ `"""` (chuẩn PEP 257 — formatter Ruff không cho đổi).
- **Import thừa / biến thừa** (`F401`, `F841`): chỉ cảnh báo, **không tự xóa khi lưu** (có thể bạn sắp dùng).
- **Pyright** (`basic` + override): tắt các cảnh báo "có thể là None" (`reportOptional*`) vì báo nhầm
  nhiều với Qt (widget gán sau trong `__init__`); `tools/export_onnx.py` bị loại (cần torch).
- **VS Code:** Ruff là formatter cho Python; khi lưu: `formatOnSave` + `source.fixAll.ruff` +
  `source.organizeImports.ruff`. Pylance `diagnosticMode: workspace` → tab Problems liệt kê lỗi của cả
  dự án. `.json` dùng formatter có sẵn.
- **Dòng lệnh:** `lint.bat` (Windows) / `./lint.sh` (Mac) = `ruff format` + `ruff check --fix` + `pyright`;
  `--check` = chỉ kiểm tra, exit code 1 nếu có lỗi.
- **Build:** `build_windows.bat` / `build_mac.sh` chạy `lint --check` trước pytest (bỏ qua cùng
  `--skip-tests`) → code sai / chưa format thì không build.

## Lưu ý / giới hạn

- Thư mục loại trừ: `venv`, `.venv`, `build`, `dist`, `installer_output`, `ffmpeg`, `models`.
- `*.spec` không phải `.py` nên không được format/lint.
- Không đặt tên script là `format.bat`: trên Windows `format` là lệnh FORMAT ổ đĩa.
- Stub PySide6 khai báo `QImage.save/loadFromData(format: bytes)` nhưng lúc chạy **phải** là `str`
  (`b'PNG'` lỗi ValueError) → giữ `'PNG'` kèm `# pyright: ignore[...]`. `QByteArray.data()` stub nói
  memoryview → bọc `bytes(...)`. `QPolygonF[i]` stub không có → dùng `.at(i)`.
- Pyright trên máy mới cần Node.js: gói pip `pyright` tự tải lần đầu (cần internet).
- Extension ESLint không làm gì với `.py`; `.vscode/extensions.json` đánh dấu không cần cho dự án.

## Test

- `lint.bat --check` → "already formatted", "All checks passed!", "0 errors" (mốc sạch 2026-10-03).
- `pytest -q` vẫn xanh sau khi format toàn bộ.
- Thủ công: thêm `import os` không dùng / khoảng trắng cuối dòng / `"abc"` → gạch vàng; gọi
  `Path('a').abc()` → gạch đỏ; Ctrl+S → nháy đơn, xóa khoảng trắng, sắp xếp import.

## Lịch sử thay đổi

- 2026-10-03: Thêm Ruff + format-on-save, format toàn bộ code một lần — người dùng muốn "eslint"
  tự format khi lưu.
- 2026-10-03: Mở rộng như ESLint — thêm rule khoảng trắng (preview), thứ tự import (`I`), nháy đơn
  (`quote-style = "single"`, đổi 4855 chuỗi), `UP/C4/SIM/PIE/RET/T10`; thêm Pyright (Pylance) báo
  error khi code sai, sửa 134 → 0 lỗi kiểu có sẵn (chủ yếu khai báo kiểu; `timeline.scroll` →
  `view_start` vì đè method `QWidget.scroll`); `lint` chạy cả pyright; build chạy lint trước test.
