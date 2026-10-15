//! Chỉnh vùng chọn trước khi tách nền thật: hiện vùng AI đã chọn (phần sẽ bị xoá phủ đỏ + đường
//! viền), tô cọ thêm / bớt, hoàn tác từng nét, rồi mới áp dụng. Xem
//! `docs/instruction/background-removal.md`.
//!
//! Matte sửa ở **cỡ thật** (để xuất file đúng) và song song ở cỡ xem trước (để vẽ lại nhanh).

use std::sync::Arc;

use pbe_core::photo::matting::{brush_line, Matte};
use pbe_core::photo::RgbaImage;

/// Số nét cọ hoàn tác được.
const UNDO_LIMIT: usize = 30;
/// Màu phủ phần sẽ bị xoá (như Quick Mask của Photoshop) và độ đậm tối đa.
const TINT: [f32; 3] = [255.0, 40.0, 40.0];
const TINT_STRENGTH: f32 = 0.55;
/// Đường viền vùng chọn.
const OUTLINE: [u8; 4] = [0, 230, 255, 255];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskView {
    /// Ảnh gốc, phần sẽ xoá phủ đỏ + đường viền.
    Overlay,
    /// Xem trước kết quả: phần sẽ xoá thành trong suốt.
    Result,
}

/// Hình chữ nhật `(x0, y0, x1, y1)` px của matte cỡ thật.
type PxRect = (usize, usize, usize, usize);

/// Một nét cọ để hoàn tác: vùng chữ nhật `(x0, y0, x1, y1)` của matte cỡ thật + pixel cũ.
struct UndoStep {
    rect: PxRect,
    before: Vec<u8>,
}

pub struct MaskEdit {
    pub w: usize,
    pub h: usize,
    full: Vec<u8>,
    /// Matte lúc bắt đầu chỉnh (kết quả AI hoặc vùng đã áp trước đó) — "Bỏ mọi nét tô".
    original: Arc<Vec<u8>>,
    /// Bản cỡ xem trước `(pw × ph)`; rỗng = cần tính lại từ `full`.
    preview: Vec<u8>,
    pw: usize,
    ph: usize,
    undo: Vec<UndoStep>,
    /// Bản sao matte lúc bắt đầu nét đang tô + hình bao đã chạm.
    stroke: Option<(Vec<u8>, PxRect)>,
    last: Option<(f64, f64)>,
    /// true = cọ thêm vào vùng giữ lại, false = bớt.
    pub add: bool,
    /// Đường kính cọ, px ảnh.
    pub size: f64,
    /// Độ cứng 0…1 (0 = mép rất mềm).
    pub hardness: f64,
    pub view: MaskView,
    /// Tài liệu đang chỉnh (đổi ảnh thì huỷ).
    pub doc_serial: u64,
    /// Ảnh vẽ lên canvas + khoá cache `(revision của composite, view)`.
    shown: Option<RgbaImage>,
    shown_key: Option<(u64, MaskView)>,
}

impl MaskEdit {
    pub fn new(matte: &Matte, doc_serial: u64) -> Self {
        MaskEdit {
            w: matte.w,
            h: matte.h,
            full: matte.data.as_ref().clone(),
            original: matte.data.clone(),
            preview: Vec::new(),
            pw: 0,
            ph: 0,
            undo: Vec::new(),
            stroke: None,
            last: None,
            add: true,
            size: (matte.w.max(matte.h) as f64 / 25.0)
                .clamp(10.0, 300.0)
                .round(),
            hardness: 0.6,
            view: MaskView::Overlay,
            doc_serial,
            shown: None,
            shown_key: None,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn is_modified(&self) -> bool {
        self.full != *self.original
    }

    fn invalidate(&mut self) {
        self.shown_key = None;
    }

    // --- Tô cọ ---------------------------------------------------------------------------

    /// Bắt đầu một nét tại `pt` (px ảnh). `invert` (giữ Alt) = đảo thêm / bớt.
    pub fn begin_stroke(&mut self, pt: (f64, f64), invert: bool) {
        self.end_stroke();
        self.stroke = Some((self.full.clone(), (usize::MAX, usize::MAX, 0, 0)));
        self.last = None;
        self.stroke_to(pt, invert);
    }

    pub fn stroke_to(&mut self, pt: (f64, f64), invert: bool) {
        if self.stroke.is_none() {
            return;
        }
        let add = self.add != invert;
        let radius = self.size / 2.0;
        let bb = brush_line(
            &mut self.full,
            self.h,
            self.w,
            self.last,
            pt,
            radius,
            self.hardness,
            add,
        );
        if let Some((_, sbb)) = &mut self.stroke {
            if bb.0 < bb.2 && bb.1 < bb.3 {
                *sbb = (
                    sbb.0.min(bb.0),
                    sbb.1.min(bb.1),
                    sbb.2.max(bb.2),
                    sbb.3.max(bb.3),
                );
            }
        }
        // Cùng nét ở bản xem trước (toạ độ và bán kính theo tỉ lệ).
        if !self.preview.is_empty() {
            let (kx, ky) = (
                self.pw as f64 / self.w as f64,
                self.ph as f64 / self.h as f64,
            );
            let scale = |p: (f64, f64)| (p.0 * kx, p.1 * ky);
            brush_line(
                &mut self.preview,
                self.ph,
                self.pw,
                self.last.map(scale),
                scale(pt),
                radius * kx,
                self.hardness,
                add,
            );
        }
        self.last = Some(pt);
        self.invalidate();
    }

    /// Kết thúc nét: lưu vùng đã chạm để hoàn tác.
    pub fn end_stroke(&mut self) {
        self.last = None;
        let Some((before, (x0, y0, x1, y1))) = self.stroke.take() else {
            return;
        };
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let mut region = Vec::with_capacity((x1 - x0) * (y1 - y0));
        for y in y0..y1 {
            region.extend_from_slice(&before[y * self.w + x0..y * self.w + x1]);
        }
        self.undo.push(UndoStep {
            rect: (x0, y0, x1, y1),
            before: region,
        });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
    }

    /// Hoàn tác nét cọ gần nhất.
    pub fn undo_stroke(&mut self) -> bool {
        self.end_stroke();
        let Some(step) = self.undo.pop() else {
            return false;
        };
        let (x0, y0, x1, y1) = step.rect;
        let cw = x1 - x0;
        for (row, y) in (y0..y1).enumerate() {
            self.full[y * self.w + x0..y * self.w + x1]
                .copy_from_slice(&step.before[row * cw..row * cw + cw]);
        }
        self.preview.clear();
        self.invalidate();
        true
    }

    /// Bỏ mọi nét tô, về vùng lúc bắt đầu chỉnh.
    pub fn reset(&mut self) {
        self.end_stroke();
        self.full = self.original.as_ref().clone();
        self.undo.clear();
        self.preview.clear();
        self.invalidate();
    }

    pub fn into_matte(mut self) -> Matte {
        self.end_stroke();
        Matte::new(self.h, self.w, self.full)
    }

    // --- Hiển thị ------------------------------------------------------------------------

    /// Ảnh để vẽ lên canvas, cùng cỡ `base` (ảnh nền đã chỉnh màu, chưa tách, ở cỡ xem trước).
    /// `rev` đổi khi `base` đổi. Chỉ tính lại khi matte / base / chế độ xem đổi.
    pub fn shown(&mut self, base: &RgbaImage, rev: u64) -> &RgbaImage {
        if (self.pw, self.ph) != (base.w, base.h) || self.preview.is_empty() {
            let m = Matte::new(self.h, self.w, self.full.clone());
            self.preview = m
                .resized(base.h, base.w)
                .iter()
                .map(|v| (v * 255.0).round() as u8)
                .collect();
            (self.pw, self.ph) = (base.w, base.h);
            self.shown_key = None;
        }
        let key = (rev, self.view);
        if self.shown_key != Some(key) || self.shown.is_none() {
            self.shown = Some(match self.view {
                MaskView::Overlay => overlay_image(base, &self.preview),
                MaskView::Result => result_image(base, &self.preview),
            });
            self.shown_key = Some(key);
        }
        self.shown.as_ref().expect("vừa tính")
    }
}

/// Phần sẽ bị xoá phủ đỏ (đậm theo `1 − matte`) + đường viền ở ranh giới matte = 50 %.
pub fn overlay_image(base: &RgbaImage, matte: &[u8]) -> RgbaImage {
    let (h, w) = (base.h, base.w);
    let mut out = base.clone();
    let inside = |i: usize| matte[i] >= 128;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let edge = (x > 0 && inside(i - 1) != inside(i))
                || (y > 0 && inside(i - w) != inside(i))
                || (x + 1 < w && inside(i + 1) != inside(i))
                || (y + 1 < h && inside(i + w) != inside(i));
            let p = &mut out.data[i * 4..i * 4 + 4];
            if edge {
                p.copy_from_slice(&OUTLINE);
                continue;
            }
            let k = TINT_STRENGTH * (1.0 - matte[i] as f32 / 255.0);
            if k > 0.0 {
                for c in 0..3 {
                    p[c] = (p[c] as f32 * (1.0 - k) + TINT[c] * k).round() as u8;
                }
                p[3] = p[3].max((255.0 * k) as u8);
            }
        }
    }
    out
}

/// Xem trước kết quả: alpha = alpha × matte (canvas vẽ ô vuông bên dưới).
pub fn result_image(base: &RgbaImage, matte: &[u8]) -> RgbaImage {
    let mut out = base.clone();
    for (i, &m) in matte.iter().enumerate() {
        let a = &mut out.data[i * 4 + 3];
        *a = ((*a as u32 * m as u32 + 127) / 255) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn half_matte(h: usize, w: usize) -> Matte {
        Matte::new(
            h,
            w,
            (0..h * w)
                .map(|i| if i % w < w / 2 { 255 } else { 0 })
                .collect(),
        )
    }

    #[test]
    fn paint_undo_reset() {
        let mut e = MaskEdit::new(&half_matte(20, 40), 1);
        e.size = 6.0;
        e.hardness = 1.0;
        // thêm ở bên phải (đang là nền)
        e.begin_stroke((30.0, 10.0), false);
        e.stroke_to((35.0, 10.0), false);
        e.end_stroke();
        assert!(e.is_modified() && e.can_undo());
        assert_eq!(e.full[10 * 40 + 32], 255);
        // Alt = bớt ở bên trái (đang là người)
        e.begin_stroke((5.0, 10.0), true);
        e.end_stroke();
        assert_eq!(e.full[10 * 40 + 5], 0);
        assert!(e.undo_stroke());
        assert_eq!(e.full[10 * 40 + 5], 255);
        assert!(e.undo_stroke());
        assert!(!e.is_modified());
        assert!(!e.undo_stroke());
        e.begin_stroke((30.0, 10.0), false);
        e.reset();
        assert!(!e.is_modified() && !e.can_undo());
    }

    #[test]
    fn preview_follows_full_resolution_strokes() {
        let mut e = MaskEdit::new(&half_matte(40, 80), 1);
        let base = RgbaImage::filled(20, 40, [100, 100, 100, 255]);
        let _ = e.shown(&base, 1); // tạo bản xem trước 40×20
        e.size = 10.0;
        e.hardness = 1.0;
        e.begin_stroke((60.0, 20.0), false);
        e.end_stroke();
        // điểm (60, 20) cỡ thật = (30, 10) cỡ xem trước
        assert_eq!(e.preview[10 * 40 + 30], 255);
        let into = e.into_matte();
        assert_eq!((into.h, into.w), (40, 80));
        assert_eq!(into.data[20 * 80 + 60], 255);
    }

    #[test]
    fn overlay_tints_removed_area_and_draws_outline() {
        let base = RgbaImage::filled(4, 8, [100, 100, 100, 255]);
        let matte: Vec<u8> = (0..32).map(|i| if i % 8 < 4 { 255 } else { 0 }).collect();
        let o = overlay_image(&base, &matte);
        assert_eq!(o.px(1, 1), [100, 100, 100, 255]); // giữ lại: không đổi
        let red = o.px(1, 6);
        assert!(red[0] > 150 && red[1] < 100, "{red:?}"); // sẽ xoá: đỏ
        assert_eq!(o.px(1, 3), OUTLINE); // ranh giới
        assert_eq!(o.px(1, 4), OUTLINE);
        let r = result_image(&base, &matte);
        assert_eq!(r.px(1, 1)[3], 255);
        assert_eq!(r.px(1, 6)[3], 0);
    }
}
