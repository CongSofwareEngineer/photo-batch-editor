//! Logic của trình sửa ảnh đơn (photo editor), không Qt / không GUI.
//! Port của gói Python `core/photo/` — giai đoạn 4 của `docs/instruction/rust-port.md`.

pub mod collage;
pub mod effects;
pub mod geometry;
pub mod history;
pub mod image_io;

/// Ảnh HWC 4 kênh uint8 (RGBA, chưa premultiply) — đơn vị làm việc của photo editor.
///
/// Bản Python dùng `np.ndarray` `(H, W, 4)` uint8; ở đây là một `Vec<u8>` liền nhau theo
/// thứ tự `R, G, B, A` để copy thẳng sang texture của GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub h: usize,
    pub w: usize,
    pub data: Vec<u8>, // h*w*4
}

impl RgbaImage {
    /// Ảnh trong suốt hoàn toàn `w × h`.
    pub fn new(h: usize, w: usize) -> Self {
        RgbaImage {
            h,
            w,
            data: vec![0; h * w * 4],
        }
    }

    pub fn from_vec(h: usize, w: usize, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), h * w * 4, "RgbaImage: độ dài data không khớp h*w*4");
        RgbaImage { h, w, data }
    }

    /// Ảnh đục một màu `(r, g, b, a)`.
    pub fn filled(h: usize, w: usize, color: [u8; 4]) -> Self {
        let mut data = Vec::with_capacity(h * w * 4);
        for _ in 0..h * w {
            data.extend_from_slice(&color);
        }
        RgbaImage { h, w, data }
    }

    #[inline]
    pub fn px(&self, y: usize, x: usize) -> [u8; 4] {
        let i = (y * self.w + x) * 4;
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }

    #[inline]
    pub fn set_px(&mut self, y: usize, x: usize, p: [u8; 4]) {
        let i = (y * self.w + x) * 4;
        self.data[i..i + 4].copy_from_slice(&p);
    }

    pub fn is_empty(&self) -> bool {
        self.h == 0 || self.w == 0
    }

    /// Cạnh dài (px) — kích thước dùng cho các tham số theo không gian.
    pub fn long_side(&self) -> usize {
        self.h.max(self.w)
    }

    /// Cắt một hình chữ nhật (tự kẹp vào trong ảnh). Vùng rỗng → ảnh 0×0.
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> RgbaImage {
        let x1 = (x + w).min(self.w);
        let y1 = (y + h).min(self.h);
        let (x0, y0) = (x.min(self.w), y.min(self.h));
        let (cw, ch) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        let mut out = RgbaImage::new(ch, cw);
        for oy in 0..ch {
            let src = ((y0 + oy) * self.w + x0) * 4;
            let dst = oy * cw * 4;
            out.data[dst..dst + cw * 4].copy_from_slice(&self.data[src..src + cw * 4]);
        }
        out
    }

    /// Dán `src` vào vị trí `(x, y)` (ghi đè pixel, không alpha blend). Phần ra ngoài bị bỏ.
    pub fn paste(&mut self, src: &RgbaImage, x: usize, y: usize) {
        for sy in 0..src.h {
            let dy = y + sy;
            if dy >= self.h {
                break;
            }
            let cw = src.w.min(self.w.saturating_sub(x));
            if cw == 0 {
                break;
            }
            let s = sy * src.w * 4;
            let d = (dy * self.w + x) * 4;
            self.data[d..d + cw * 4].copy_from_slice(&src.data[s..s + cw * 4]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_and_paste_clamp() {
        let img = RgbaImage::filled(10, 10, [1, 2, 3, 255]);
        let c = img.crop(8, 8, 5, 5);
        assert_eq!((c.h, c.w), (2, 2));
        assert_eq!(c.px(0, 0), [1, 2, 3, 255]);

        let mut dst = RgbaImage::new(4, 4);
        dst.paste(&RgbaImage::filled(3, 3, [9, 9, 9, 9]), 2, 2);
        assert_eq!(dst.px(3, 3), [9, 9, 9, 9]);
        assert_eq!(dst.px(1, 1), [0, 0, 0, 0]);
    }
}
