//! Xem trước "trước / sau" cho trình chỉnh nhiều ảnh: nạp ảnh, thu nhỏ, áp 15 chỉnh sửa
//! trên **luồng nền** để UI không đứng. Port phần xem trước của `ui/preview.py` + `ui/workers.py`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use pbe_core::adjustments::{self, ImageU8};
use pbe_core::photo::image_io::load_rgba;
use pbe_core::photo::RgbaImage;
use pbe_core::settings::AdjustmentSettings;

/// Cạnh dài của ảnh xem trước (px). Lớn hơn mức này thì thu nhỏ trước khi chỉnh.
pub const PREVIEW_LONG_SIDE: usize = 1400;

/// Kết quả một lượt xem trước.
pub struct PreviewResult {
    pub generation: u64,
    pub path: PathBuf,
    pub before: RgbaImage,
    pub after: RgbaImage,
    /// Cỡ ảnh gốc (hiện trên UI).
    pub source_size: (usize, usize),
    pub error: Option<String>,
}

/// Bộ chạy xem trước: mỗi yêu cầu mới tăng `generation`, kết quả cũ bị bỏ.
pub struct PreviewWorker {
    tx: Sender<PreviewResult>,
    rx: Receiver<PreviewResult>,
    generation: Arc<AtomicU64>,
    pending: Option<(PathBuf, AdjustmentSettings)>,
    pub busy: bool,
}

impl Default for PreviewWorker {
    fn default() -> Self {
        let (tx, rx) = channel();
        PreviewWorker {
            tx,
            rx,
            generation: Arc::new(AtomicU64::new(0)),
            pending: None,
            busy: false,
        }
    }
}

impl PreviewWorker {
    /// Đặt yêu cầu mới (ghi đè yêu cầu chưa chạy).
    pub fn request(&mut self, path: &Path, settings: &AdjustmentSettings) {
        self.pending = Some((path.to_path_buf(), settings.clone()));
    }

    /// Gọi mỗi khung: chạy yêu cầu đang chờ (nếu rảnh) và trả kết quả mới nhất.
    pub fn poll(&mut self) -> Option<PreviewResult> {
        if !self.busy {
            if let Some((path, settings)) = self.pending.take() {
                let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
                let tx = self.tx.clone();
                self.busy = true;
                std::thread::spawn(move || {
                    let res = render(generation, &path, &settings);
                    let _ = tx.send(res);
                });
            }
        }
        let mut latest: Option<PreviewResult> = None;
        while let Ok(res) = self.rx.try_recv() {
            self.busy = false;
            let keep = res.generation >= self.generation.load(Ordering::SeqCst);
            if keep {
                latest = Some(res);
            }
        }
        latest
    }
}

fn render(generation: u64, path: &Path, settings: &AdjustmentSettings) -> PreviewResult {
    let empty = RgbaImage::new(0, 0);
    let src = match load_rgba(path) {
        Ok(img) => img,
        Err(e) => {
            return PreviewResult {
                generation,
                path: path.to_path_buf(),
                before: empty.clone(),
                after: empty,
                source_size: (0, 0),
                error: Some(e.to_string()),
            }
        }
    };
    let source_size = (src.w, src.h);
    let before = downscale_to(&src, PREVIEW_LONG_SIDE);
    let after = apply_settings(&before, settings, src.long_side());
    PreviewResult {
        generation,
        path: path.to_path_buf(),
        before,
        after,
        source_size,
        error: None,
    }
}

/// Thu nhỏ để cạnh dài ≤ `long_side` (trung bình khối; không phóng to).
pub fn downscale_to(img: &RgbaImage, long_side: usize) -> RgbaImage {
    if img.is_empty() || img.long_side() <= long_side {
        return img.clone();
    }
    let k = long_side as f64 / img.long_side() as f64;
    let w = ((img.w as f64 * k).round() as usize).max(1);
    let h = ((img.h as f64 * k).round() as usize).max(1);
    let mut out = RgbaImage::new(h, w);
    let (sx, sy) = (img.w as f64 / w as f64, img.h as f64 / h as f64);
    for oy in 0..h {
        let y0 = (oy as f64 * sy).floor() as usize;
        let y1 = (((oy + 1) as f64 * sy).ceil() as usize).min(img.h).max(y0 + 1);
        for ox in 0..w {
            let x0 = (ox as f64 * sx).floor() as usize;
            let x1 = (((ox + 1) as f64 * sx).ceil() as usize).min(img.w).max(x0 + 1);
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = img.px(y, x);
                    for c in 0..4 {
                        acc[c] += p[c] as u32;
                    }
                    n += 1;
                }
            }
            out.set_px(
                oy,
                ox,
                [
                    (acc[0] / n) as u8,
                    (acc[1] / n) as u8,
                    (acc[2] / n) as u8,
                    (acc[3] / n) as u8,
                ],
            );
        }
    }
    out
}

/// Áp 15 chỉnh sửa lên một ảnh RGBA; `long_side` là cạnh dài của **ảnh gốc** để bán kính
/// sharpening / clarity trông đúng ở cỡ xem trước.
pub fn apply_settings(img: &RgbaImage, settings: &AdjustmentSettings, long_side: usize) -> RgbaImage {
    if img.is_empty() {
        return img.clone();
    }
    let n = img.h * img.w;
    let mut rgb = ImageU8 {
        h: img.h,
        w: img.w,
        data: vec![0; n * 3],
    };
    for i in 0..n {
        rgb.data[i * 3..i * 3 + 3].copy_from_slice(&img.data[i * 4..i * 4 + 3]);
    }
    let planar = adjustments::apply_adjustments_planar_u8_ls(&rgb, settings, Some(long_side));
    let out = adjustments::planar_to_uint8(&planar);
    let mut res = RgbaImage::new(img.h, img.w);
    for i in 0..n {
        res.data[i * 4..i * 4 + 3].copy_from_slice(&out.data[i * 3..i * 3 + 3]);
        res.data[i * 4 + 3] = img.data[i * 4 + 3];
    }
    res
}
