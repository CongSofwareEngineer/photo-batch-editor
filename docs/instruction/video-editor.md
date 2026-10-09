# Sửa video (Video editor)

- **Mục đích:** trang "Sửa video": xem trước, cắt nhiều đoạn + ghép theo thứ tự, xóa đoạn giữa, tắt tiếng,
  chèn chữ (tiếng Việt), chèn nhạc (âm lượng riêng, fade out), xuất MP4 / MOV (giữ nguyên / 1080p / 720p) bằng
  FFmpeg chạy nền có % tiến trình và nút Hủy. Không ghi đè video gốc; mặc định lưu vào thư mục `editor`.
- **File:**
  - `rust/core/src/video/` (không UI): `ffmpeg.rs` (tìm ffmpeg, `probe` đọc thông tin, `run_ffmpeg` có tiến trình / hủy),
    `project.rs` (`VideoProject`, `Clip`, `TextOverlay`, `MusicTrack` + thao tác timeline),
    `export.rs` (dựng lệnh `-filter_complex`, `export_video`, `output_size`)
  - `rust/gui/src/video.rs` (trang Sửa video: timeline, panels, xem trước khung tĩnh)
  - `ffmpeg/` (binary); build: `build_rust_windows.bat`, `build_rust_mac.sh`
- **Changelog:** [../changelog/video-editor.md](../changelog/video-editor.md)
- **Ghi chú:** mã Python (`core/video/`, `ui/video/`) đã xoá; logic → `rust/core/src/video/`, GUI → `rust/gui/src/video.rs`. **Chưa** port: vẽ chữ lên video, phát video có tiếng (xem trước là khung tĩnh lấy bằng FFmpeg). Chi tiết: [rust-port.md](rust-port.md).

## Logic chính

- **FFmpeg (Rust):** tìm theo thứ tự `PBE_FFMPEG` → `ffmpeg/ffmpeg` cạnh app (`<app>/ffmpeg/`) → cạnh
  file thực thi → `PATH` (bỏ bước `imageio-ffmpeg` — bản Python cũ đã xoá). Không thấy → báo lỗi rõ
  (`FFMPEG_NOT_FOUND`) khi mở / xuất. `run_ffmpeg` đọc tiến trình từ `-progress pipe:1`, log lỗi (25 dòng
  cuối) trên luồng riêng, và `kill` tiến trình khi hủy.
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

- **Xem trước (Rust):** khung **tĩnh** lấy bằng `ffmpeg -ss <t> -frames:v 1` trên luồng nền (bản Python cũ
  phát có tiếng bằng QMediaPlayer — **chưa** port). Nhạc/chữ vẫn lưu được trong dự án.

## Lưu ý / giới hạn

- Chữ giữ nguyên giây timeline khi cắt / sắp xếp lại đoạn (không tự dời theo đoạn).
- Nhạc luôn bắt đầu ở giây 0 của timeline ("Bắt đầu từ giây" là vị trí trong file nhạc).
- FFmpeg khi phát hành phải kèm thông tin license (bản dùng libx264 là GPL).

## Test

- `rust/core/src/video/*.rs` (`#[cfg(test)]`): probe / project / timeline / lệnh ffmpeg — bản port của
  `tests/test_video.py` cũ.
- `rust/core/tests/video_export.rs` (`cargo test --test video_export`): xuất video thật bằng FFmpeg
  (probe, cắt + nhạc + overlay, tắt tiếng, hủy không để lại file). Tự bỏ qua khi máy không có FFmpeg.
- Thủ công: `cargo run -p pbe-gui` → Sửa video: mở video điện thoại, cắt 2 lần xóa đoạn giữa, kéo đổi thứ
  tự, thêm nhạc mp3, xuất 720p MP4/MOV, bấm Hủy giữa chừng.
