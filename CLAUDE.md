# Quy tắc chính của dự án — Photo Batch Editor

File này là bộ quy tắc bắt buộc cho mọi phiên làm việc (người hoặc AI) trên dự án.

## Cấu trúc docs

- Mỗi chức năng có **một file log riêng** trong `docs/` (vd. `docs/login.md`,
  `docs/settings.md`, `docs/batch.md`…). **Không gộp nhiều chức năng vào một file.**
- `docs/log.md` chỉ là **mục lục**: bảng chức năng → file log, kèm mẫu file log.

## 1. Đọc `docs/` TRƯỚC khi code

- Trước khi làm bất kỳ tính năng / sửa lỗi / sửa logic nào, **luôn đọc `docs/log.md`** để tìm
  file log của chức năng liên quan, rồi **đọc các file log đó** để lấy dữ liệu, quyết định cũ
  và lưu ý trước khi viết code.
- Nếu thay đổi chạm nhiều chức năng → đọc file log của từng chức năng đó.
- Nếu docs mâu thuẫn với code hiện tại: tin code, rồi sửa lại docs cho đúng.

## 2. Làm tính năng mới → tạo file log riêng

- Tính năng mới (chưa thuộc chức năng nào đã có) → tạo file `docs/<ten-chuc-nang>.md` theo
  mẫu trong `docs/log.md`, và thêm một dòng vào bảng mục lục của `docs/log.md`.
- Tính năng mở rộng một chức năng đã có → ghi vào file log của chức năng đó.
- Ghi ngay trong lần làm đó, không để sau. Nội dung tối thiểu: mục đích, file/module bị ảnh
  hưởng, logic chính (đủ để người khác trong team hiểu), lưu ý / giới hạn, cách test.

## 3. Sửa logic → cập nhật file log có sẵn của chức năng đó

- Sửa **bất kỳ** logic nào cũng phải cập nhật file log của chức năng đó để team hiểu.
- **Không tạo file log mới cho mỗi lần sửa.** Thay vào đó:
  1. Sửa phần "Logic chính" / "Lưu ý" cho đúng hiện trạng, và
  2. Thêm một dòng vào "Lịch sử thay đổi" (ngày + sửa gì + vì sao).
- Sửa chạm nhiều chức năng → cập nhật file log của từng chức năng bị ảnh hưởng.

## 4. Điều kiện bắt buộc trước khi code (luồng chuẩn)

**Chưa có file log của chức năng thì KHÔNG được bắt đầu code.**

```
Nhận task (thêm / sửa tính năng)
        │
        ▼
Đọc docs/log.md (mục lục) → chức năng này đã có file log chưa?
        │
   ┌────┴─────────────────────────┐
   │ CÓ                            │ CHƯA CÓ
   ▼                               ▼
Đọc file log đó                 Tạo file docs/<ten-chuc-nang>.md TRƯỚC
→ lấy làm dữ liệu               (theo mẫu, ghi mục đích, file dự kiến
→ sửa tiếp trên nền đó           sửa, logic dự kiến) + thêm vào mục lục
   │                               │
   │                               ▼
   │                            Lấy file log vừa tạo làm dữ liệu
   └────┬─────────────────────────┘
        ▼
Đọc code liên quan → BẮT ĐẦU CODE → test (`pytest -q`)
        │
        ▼
Cập nhật lại file log cho đúng với code thực tế
(Logic chính, Lưu ý, Test, Lịch sử thay đổi)
+ README.md nếu ảnh hưởng tới người dùng / cách build
        │
        ▼
Xong (chỉ xong khi log đã khớp với code)
```

**Ví dụ: task "thêm / sửa tính năng đổi sáng tối (light/dark theme)"**

1. Mở `docs/log.md`, tìm chức năng giao diện sáng/tối.
2. **Nếu đã có** (vd. `docs/theme.md`): đọc hết file đó (logic hiện tại, file liên quan, lưu ý,
   lịch sử) → dùng làm dữ liệu → sửa tiếp trên nền đó.
3. **Nếu chưa có**: tạo `docs/theme.md` trước (mục đích, file dự kiến chạm như
   `ui/theme.qss`, `ui/main_window.py`, `ui/settings_view.py`; logic dự kiến; lưu ý) và thêm
   dòng "Giao diện sáng / tối → theme.md" vào mục lục → lấy file đó làm dữ liệu.
4. Sau đó mới đọc code và bắt đầu code.
5. Code xong → cập nhật `docs/theme.md` cho khớp với code thực tế + ghi "Lịch sử thay đổi".

## Ghi chú kỹ thuật nhanh

- Cấu trúc dự án, cách chạy, build, test: xem `README.md`.
- Chữ hiển thị trên UI phải dùng `tr("…")` và có bản dịch trong `core/i18n_vi.py`
  (`tests/test_i18n.py` sẽ fail nếu thiếu).
- `core/` không được import Qt; UI nằm trong `ui/`.
- Code style như ESLint (`pyproject.toml`): Ruff (nháy đơn, khoảng trắng, import) + Pyright (lỗi kiểu).
  Sửa xong chạy `lint.bat --check` (Mac: `./lint.sh --check`) — phải 0 lỗi.
- App chạy cả Windows 10/11 và macOS: khác biệt hệ điều hành đặt trong `core/system.py` / kiểm tra
  `sys.platform`; không hard-code `C:\…`, `explorer`, font Windows. Build: `build_windows.bat` / `build_mac.sh`.
