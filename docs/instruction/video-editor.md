# Sửa video (Video editor)

- **Mục đích:** trang "Sửa video": xem trước, cắt nhiều đoạn + ghép theo thứ tự, xóa đoạn giữa, tắt tiếng,
  chèn chữ (tiếng Việt), chèn nhạc (âm lượng riêng, fade out), xuất MP4 / MOV (giữ nguyên / 1080p / 720p) bằng
  FFmpeg chạy nền có % tiến trình và nút Hủy. Không ghi đè video gốc; mặc định lưu vào thư mục `editor`.
- **File:**
  - `core/video/` (không Qt): `ffmpeg.py` (tìm ffmpeg, `probe` đọc thông tin, `run_ffmpeg` có tiến trình / hủy),
    `project.py` (`VideoProject`, `Clip`, `TextOverlay`, `MusicTrack` + thao tác timeline),
    `export.py` (dựng lệnh `-filter_complex`, `export_video`, `output_size`)
  - `ui/video/`: `player.py` (QMediaPlayer + QVideoSink, phát theo timeline, kéo chữ), `timeline.py`,
    `panels.py` (Đoạn / Chữ / Âm thanh / Xuất), `text_render.py` (vẽ chữ bằng Qt → PNG),
    `video_editor_view.py` (trang + `ExportWorker` QThread)
  - `tools/fetch_ffmpeg.py`, `PhotoBatchEditor.spec`, `build_windows.bat`, `build_mac.sh`, `requirements.txt` (imageio-ffmpeg)
- **Changelog:** [../changelog/video-editor.md](../changelog/video-editor.md)

## Logic chính

- **FFmpeg:** tìm theo thứ tự `PBE_FFMPEG` → `ffmpeg/ffmpeg.exe` cạnh app (bản .exe: `_internal/ffmpeg/`) →
  binary của gói pip `imageio-ffmpeg` → `PATH`. Không thấy → báo lỗi rõ (`FFMPEG_NOT_FOUND`) khi mở / xuất.
  `build_windows.bat` / `build_mac.sh` chạy `tools/fetch_ffmpeg.py` để copy ffmpeg.exe (Mac: `ffmpeg`) vào `ffmpeg/` rồi spec đóng gói (gói
  `imageio_ffmpeg` bị exclude để không kèm 2 lần). Tiến trình ffmpeg chạy không hiện cửa sổ console.
- **Probe:** `ffmpeg -i` → parse thời lượng, kích thước, fps, có tiếng không, góc xoay (video điện thoại
  xoay 90° → đổi rộng/cao vì ffmpeg tự xoay khi xuất).
- **Mô hình:** `clips` = danh sách đoạn giữ lại theo *giây của video gốc*, phát lần lượt = timeline.
  Chữ và playhead dùng *giây timeline*. `locate(t)` ↔ `timeline_time(i, src)`.
  Thao tác: `split_at` (cắt đôi, mỗi phần ≥ 0.1 s), `delete_clip` (không xóa đoạn cuối cùng), `move_clip`,
  `trim_clip`, `add_clip` (thêm một khoảng của video gốc vào cuối → cho phép lặp / ghép nhiều đoạn),
  `keep_range` / `delete_range` theo điểm In / Out trên timeline (xóa đoạn giữa).
- **Undo:** `History[VideoProject]` (deepcopy snapshot, 50 bước, gộp theo key khi kéo/gõ). Thao tác có thể
  không làm gì (vd. cắt sát mép) chỉ ghi bước khi thực sự thay đổi (`apply`).
- **Xem trước:** khung hình từ `QVideoSink` vẽ lên widget (để vẽ + kéo chữ trên hình). Khi phát hết một đoạn
  thì nhảy tới đầu đoạn kế tiếp. Nhạc phát bằng QMediaPlayer thứ 2, đồng bộ lại khi lệch > 300 ms, fade out mô
  phỏng bằng âm lượng. Âm lượng xem trước tối đa 100 %.
- **Timeline:** thước thời gian, hàng Video (các đoạn, kéo mép = cắt bớt), Chữ (kéo khối = dời thời gian, kéo
  mép = đổi bắt đầu/kết thúc), Nhạc; vùng In/Out tô vàng; playhead đỏ. Ctrl+lăn = zoom, lăn = cuộn.
- **Phím tắt:** Space phát/dừng, Ctrl+O mở, Ctrl+Z / Ctrl+Y; khi focus ở vùng xem trước / timeline:
  S cắt, I / O đánh dấu, Del xóa đoạn (hoặc chữ đang chọn), ← / → 1 khung hình, Shift ← / → 1 giây, Home / End.
- **Chữ:** vẽ bằng `ui/photo/render.paint_text` (cùng code với editor ảnh): xem trước vẽ theo tỉ lệ khung hiển
  thị; khi xuất, mỗi chữ được vẽ thành PNG trong suốt đúng độ phân giải đầu ra → ffmpeg `overlay` với
  `enable='between(t,a,b)'`. Vị trí lưu = tâm chữ theo tỉ lệ khung (0..1) nên giữ đúng khi đổi độ phân giải;
  cỡ chữ = px theo chiều cao video gốc.
- **Lệnh xuất (`build_args`):** mỗi đoạn `trim`/`atrim` + `setpts` → `concat`; `scale` (cạnh ngắn = 1080 / 720,
  không phóng to, số chẵn) + `setsar=1`; overlay chữ; `format=yuv420p`. Âm thanh: gốc × volume (bỏ nếu tắt
  tiếng / không có tiếng) trộn `amix` với nhạc (`-ss offset`, `atrim` tới hết video hoặc hết nhạc, `volume`,
  `afade` out). libx264 CRF 20 preset fast, AAC 192k, fps cố định (`-fps_mode cfr`), MP4 thêm `+faststart`.
- **Chạy nền:** `ExportWorker` (QThread) → `export_video` ghi ra `<tên>.tmp.<ext>` rồi rename khi thành công;
  hủy = terminate ffmpeg + xóa file tạm → không bao giờ để lại file hỏng. Tiến trình từ `-progress pipe:1`
  (`out_time_us` / thời lượng). Trong lúc xuất: không đổi ngôn ngữ, thoát app phải xác nhận hủy.

## Lưu ý / giới hạn

- Chữ giữ nguyên giây timeline khi cắt / sắp xếp lại đoạn (không tự dời theo đoạn).
- Nhạc luôn bắt đầu ở giây 0 của timeline ("Bắt đầu từ giây" là vị trí trong file nhạc).
- Xem trước nhảy giữa các đoạn có thể trễ ~1 khung; file xuất thì chính xác.
- FFmpeg trong `imageio-ffmpeg` là bản GPL (có libx264): khi phát hành phải kèm thông tin license
  (`ffmpeg/README.txt` được tạo tự động).

## Test

- `tests/test_video.py`: parse probe (video xoay, file chỉ có tiếng, lỗi), tìm ffmpeg (thiếu → thông báo,
  `PBE_FFMPEG`), split / delete / move / trim / add / keep_range / delete_range qua nhiều đoạn, `output_size`,
  lệnh ffmpeg (trim, concat, scale, overlay, amix, afade, -ss, tắt tiếng), từ chối ghi đè gốc; với ffmpeg thật:
  probe, xuất MOV có cắt + chữ + nhạc (thời lượng đúng, có tiếng, tiến trình tới 100 %), MP4 tắt tiếng, hủy
  không để lại file, PNG chữ đúng vị trí ở độ phân giải đầu ra, trang video mở / cắt / undo / thêm chữ.
- Thủ công: `dev.bat` → Sửa video (Ctrl+3): mở video điện thoại, phát, cắt 2 lần xóa đoạn giữa, kéo đổi thứ tự,
  thêm chữ có dấu và kéo trên hình, thêm nhạc mp3 bắt đầu từ giây 30 + fade, xuất 720p MP4 và MOV, bấm Hủy giữa chừng.
