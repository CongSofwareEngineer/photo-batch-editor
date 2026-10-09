//! Framebuffer CPU + đưa lên GPU qua texture của egui/wgpu.
//!
//! Canvas của photo editor và khung xem trước của video **không** vẽ bằng widget của egui:
//! chúng được **ghép bằng CPU** vào một mảng RGBA (kiểu crate `pixels`) rồi tải lên một
//! texture wgpu mà egui vẽ ra màn hình. Nhờ vậy:
//!
//! * mọi pixel đúng y như lúc xuất file (cùng code `pbe_core::photo`), không phụ thuộc cách
//!   egui/GPU nội suy;
//! * zoom / pan / xoay dùng đúng [`pbe_core::photo::geometry`] như bản PySide6.
//!
//! Ghi chú về `pixels`: crate `pixels` tự quản lý surface + swapchain của cửa sổ, nên không
//! dùng chung cửa sổ với `eframe` được. Ở đây giữ **đúng kiến trúc đó** (framebuffer CPU →
//! texture wgpu) nhưng để `eframe` sở hữu surface, nên bỏ phụ thuộc crate `pixels`.

use egui::{Color32, Rect, TextureHandle, TextureOptions};
use pbe_core::photo::RgbaImage;

/// Mảng RGBA tuyến tính (chưa premultiply), gốc trên-trái.
pub struct Framebuffer {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

impl Framebuffer {
    pub fn new(w: usize, h: usize) -> Self {
        Framebuffer {
            w,
            h,
            data: vec![0; w * h * 4],
        }
    }

    /// Đổi cỡ (nội dung bị xoá) nếu khác cỡ hiện tại.
    pub fn resize(&mut self, w: usize, h: usize) {
        if (self.w, self.h) != (w, h) {
            self.w = w;
            self.h = h;
            self.data.clear();
            self.data.resize(w * h * 4, 0);
        }
    }

    pub fn clear(&mut self, color: [u8; 4]) {
        for px in self.data.chunks_exact_mut(4) {
            px.copy_from_slice(&color);
        }
    }

    /// Nền ô vuông kiểu Photoshop (chỗ ảnh trong suốt).
    pub fn checkerboard(&mut self, cell: usize, a: [u8; 4], b: [u8; 4]) {
        let cell = cell.max(1);
        for y in 0..self.h {
            for x in 0..self.w {
                let c = if (x / cell + y / cell) % 2 == 0 { a } else { b };
                let i = (y * self.w + x) * 4;
                self.data[i..i + 4].copy_from_slice(&c);
            }
        }
    }

    #[inline]
    fn blend(&mut self, x: usize, y: usize, src: [u8; 4]) {
        if src[3] == 0 || x >= self.w || y >= self.h {
            return;
        }
        let i = (y * self.w + x) * 4;
        if src[3] == 255 {
            self.data[i..i + 4].copy_from_slice(&src);
            return;
        }
        let a = src[3] as u32;
        for c in 0..3 {
            let dst = self.data[i + c] as u32;
            self.data[i + c] = ((src[c] as u32 * a + dst * (255 - a)) / 255) as u8;
        }
        let da = self.data[i + 3] as u32;
        self.data[i + 3] = (a + da * (255 - a) / 255).min(255) as u8;
    }

    /// Tô một hình chữ nhật (toạ độ framebuffer, có thể ra ngoài).
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) {
        let x0 = x.floor().max(0.0) as usize;
        let y0 = y.floor().max(0.0) as usize;
        let x1 = ((x + w).ceil().max(0.0) as usize).min(self.w);
        let y1 = ((y + h).ceil().max(0.0) as usize).min(self.h);
        for yy in y0..y1 {
            for xx in x0..x1 {
                self.blend(xx, yy, color);
            }
        }
    }

    /// Viền một hình chữ nhật, dày `thickness` px.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, thickness: f32, color: [u8; 4]) {
        let t = thickness.max(1.0);
        self.fill_rect(x, y, w, t, color);
        self.fill_rect(x, y + h - t, w, t, color);
        self.fill_rect(x, y, t, h, color);
        self.fill_rect(x + w - t, y, t, h, color);
    }

    /// Vẽ `img` vào hình chữ nhật đích `(dx, dy, dw, dh)` (toạ độ framebuffer).
    ///
    /// Thu nhỏ (`dw < img.w`) lấy mẫu trung bình khối để không bị răng cưa (như xem trước
    /// của Photoshop); phóng to dùng nearest để thấy rõ từng pixel ở zoom cao.
    pub fn draw_image(&mut self, img: &RgbaImage, dx: f32, dy: f32, dw: f32, dh: f32, opacity: f32) {
        if img.is_empty() || dw <= 0.0 || dh <= 0.0 {
            return;
        }
        let x0 = dx.floor().max(0.0) as usize;
        let y0 = dy.floor().max(0.0) as usize;
        let x1 = ((dx + dw).ceil().max(0.0) as usize).min(self.w);
        let y1 = ((dy + dh).ceil().max(0.0) as usize).min(self.h);
        let sx = img.w as f32 / dw;
        let sy = img.h as f32 / dh;
        let box_sample = sx > 1.2 || sy > 1.2;
        let op = opacity.clamp(0.0, 1.0);
        for py in y0..y1 {
            for px in x0..x1 {
                let u = (px as f32 + 0.5 - dx) * sx;
                let v = (py as f32 + 0.5 - dy) * sy;
                let mut c = if box_sample {
                    sample_box(img, u, v, sx, sy)
                } else {
                    sample_nearest(img, u, v)
                };
                c[3] = (c[3] as f32 * op).round() as u8;
                self.blend(px, py, c);
            }
        }
    }

    /// `ColorImage` để tải lên GPU.
    pub fn to_color_image(&self) -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied([self.w, self.h], &self.data)
    }
}

#[inline]
fn sample_nearest(img: &RgbaImage, u: f32, v: f32) -> [u8; 4] {
    let x = (u.floor().max(0.0) as usize).min(img.w - 1);
    let y = (v.floor().max(0.0) as usize).min(img.h - 1);
    img.px(y, x)
}

/// Trung bình khối `sx × sy` pixel nguồn — tương đương `INTER_AREA` khi thu nhỏ.
fn sample_box(img: &RgbaImage, u: f32, v: f32, sx: f32, sy: f32) -> [u8; 4] {
    let hx = (sx / 2.0).max(0.5);
    let hy = (sy / 2.0).max(0.5);
    let x0 = ((u - hx).floor().max(0.0) as usize).min(img.w - 1);
    let y0 = ((v - hy).floor().max(0.0) as usize).min(img.h - 1);
    let x1 = (((u + hx).ceil().max(0.0) as usize).min(img.w)).max(x0 + 1);
    let y1 = (((v + hy).ceil().max(0.0) as usize).min(img.h)).max(y0 + 1);
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
    if n == 0 {
        return sample_nearest(img, u, v);
    }
    [
        (acc[0] / n) as u8,
        (acc[1] / n) as u8,
        (acc[2] / n) as u8,
        (acc[3] / n) as u8,
    ]
}

/// Framebuffer + texture GPU của nó; chỉ tải lên khi nội dung thay đổi.
pub struct Canvas {
    pub fb: Framebuffer,
    tex: Option<TextureHandle>,
    name: &'static str,
    dirty: bool,
}

impl Canvas {
    pub fn new(name: &'static str) -> Self {
        Canvas {
            fb: Framebuffer::new(1, 1),
            tex: None,
            name,
            dirty: true,
        }
    }

    /// Chuẩn bị framebuffer cho một khung mới cỡ `(w, h)` px vật lý.
    pub fn begin(&mut self, w: usize, h: usize) -> &mut Framebuffer {
        self.fb.resize(w.max(1), h.max(1));
        self.dirty = true;
        &mut self.fb
    }

    /// Tải framebuffer lên GPU (nếu cần) và vẽ vào `rect`.
    pub fn paint(&mut self, ui: &egui::Ui, rect: Rect) {
        if self.dirty || self.tex.is_none() {
            let img = self.fb.to_color_image();
            match &mut self.tex {
                Some(t) => t.set(img, TextureOptions::NEAREST),
                None => self.tex = Some(ui.ctx().load_texture(self.name, img, TextureOptions::NEAREST)),
            }
            self.dirty = false;
        }
        if let Some(t) = &self.tex {
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(t.id(), rect, uv, Color32::WHITE);
        }
    }
}
