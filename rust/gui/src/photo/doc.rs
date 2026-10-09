//! Tài liệu của photo editor: ảnh nền + layer, undo/redo bằng ảnh chụp trạng thái.
//! Port của `ui/photo/document.py`.
//!
//! Ảnh gốc **không bao giờ bị ghi**: tài liệu giữ pixel đã giải mã; mỗi phép sửa tạo layer
//! hoặc ảnh mới, nên một ảnh chụp ([`DocState`]) chỉ là các struct nhỏ + `Vec<u8>` dùng chung
//! qua `clone` (rẻ vì chỉ copy khi thật sự sửa pixel).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use pbe_core::photo::effects::PhotoAdjust;
use pbe_core::photo::geometry::{flip_point, normalize_angle, rotate90_point};
use pbe_core::photo::history::History;
use pbe_core::photo::RgbaImage;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn new_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Layer ảnh dán thêm (logo, ảnh ghép…).
#[derive(Debug, Clone)]
pub struct ImageLayer {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub img: RgbaImage,
    pub x: f64, // tâm, px ảnh
    pub y: f64,
    pub width: f64, // cỡ vẽ, px ảnh
    pub height: f64,
    pub rotation: f64,
    pub flip_h: bool,
    pub flip_v: bool,
}

/// Layer làm mờ / ô vuông hoá một vùng.
#[derive(Debug, Clone)]
pub struct BlurLayer {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub shape: String, // "rect" | "ellipse"
    pub mode: String,  // "blur" | "pixelate"
    pub strength: f64, // 1–100
    pub rect: (f64, f64, f64, f64),
}

#[derive(Debug, Clone)]
pub enum Layer {
    Image(ImageLayer),
    Blur(BlurLayer),
}

impl Layer {
    pub fn id(&self) -> u64 {
        match self {
            Layer::Image(l) => l.id,
            Layer::Blur(l) => l.id,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Layer::Image(_) => "image",
            Layer::Blur(_) => "blur",
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Layer::Image(l) => &l.name,
            Layer::Blur(l) => &l.name,
        }
    }

    pub fn visible(&self) -> bool {
        match self {
            Layer::Image(l) => l.visible,
            Layer::Blur(l) => l.visible,
        }
    }

    pub fn set_visible(&mut self, v: bool) {
        match self {
            Layer::Image(l) => l.visible = v,
            Layer::Blur(l) => l.visible = v,
        }
    }

    pub fn opacity(&self) -> f32 {
        match self {
            Layer::Image(l) => l.opacity,
            Layer::Blur(l) => l.opacity,
        }
    }

    pub fn set_opacity(&mut self, v: f32) {
        match self {
            Layer::Image(l) => l.opacity = v,
            Layer::Blur(l) => l.opacity = v,
        }
    }

    /// Hình chữ nhật bao (px ảnh) — dùng để chọn và vẽ khung chọn.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        match self {
            Layer::Image(l) => (
                l.x - l.width / 2.0,
                l.y - l.height / 2.0,
                l.width,
                l.height,
            ),
            Layer::Blur(l) => l.rect,
        }
    }

    pub fn translate(&mut self, dx: f64, dy: f64) {
        match self {
            Layer::Image(l) => {
                l.x += dx;
                l.y += dy;
            }
            Layer::Blur(l) => {
                l.rect.0 += dx;
                l.rect.1 += dy;
            }
        }
    }

    /// Di chuyển layer cùng canvas khi crop / xoay 90° / lật.
    pub fn map_points(
        &mut self,
        f: &dyn Fn(f64, f64) -> (f64, f64),
        rotate: f64,
        mirror: &str,
    ) {
        match self {
            Layer::Image(l) => {
                let (x, y) = f(l.x, l.y);
                l.x = x;
                l.y = y;
                if rotate != 0.0 {
                    l.rotation = normalize_angle(l.rotation + rotate);
                    if (rotate.abs() - 90.0).abs() < 1e-9 {
                        std::mem::swap(&mut l.width, &mut l.height);
                        l.rotation = normalize_angle(l.rotation - rotate);
                    }
                }
                match mirror {
                    "h" => {
                        l.flip_h = !l.flip_h;
                        l.rotation = normalize_angle(-l.rotation);
                    }
                    "v" => {
                        l.flip_v = !l.flip_v;
                        l.rotation = normalize_angle(-l.rotation);
                    }
                    _ => {}
                }
            }
            Layer::Blur(l) => {
                let (x, y, w, h) = l.rect;
                let (ax, ay) = f(x, y);
                let (bx, by) = f(x + w, y + h);
                l.rect = (ax.min(bx), ay.min(by), (bx - ax).abs(), (by - ay).abs());
            }
        }
    }
}

/// Ảnh nền + chỉnh sửa áp lên nó.
#[derive(Debug, Clone)]
pub struct Background {
    pub img: RgbaImage,
    pub adjust: PhotoAdjust,
    pub angle: f64, // xoay tự do (làm thẳng), độ; ảnh được phóng để phủ kín
    pub visible: bool,
}

/// Ảnh chụp toàn bộ tài liệu — đơn vị undo / redo.
#[derive(Debug, Clone)]
pub struct DocState {
    pub background: Background,
    pub layers: Vec<Layer>, // dưới → trên
}

pub struct Document {
    pub state: DocState,
    pub history: History<DocState>,
    pub source: Option<PathBuf>,
    pub name: String,
    pub selected: Option<u64>,
    pub modified: bool,
    /// Tăng mỗi lần pixel đổi — canvas dùng để biết khi nào phải vẽ lại.
    pub revision: u64,
}

impl Document {
    pub fn new(img: RgbaImage, source: Option<PathBuf>) -> Self {
        let name = source
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string());
        Document {
            state: DocState {
                background: Background {
                    img,
                    adjust: PhotoAdjust::default(),
                    angle: 0.0,
                    visible: true,
                },
                layers: vec![],
                },
            history: History::default(),
            source,
            name,
            selected: None,
            modified: false,
            revision: 1,
        }
    }

    pub fn size(&self) -> (usize, usize) {
        (self.state.background.img.w, self.state.background.img.h)
    }

    /// Ghi một bước undo (trạng thái **trước** thay đổi) rồi đánh dấu đã sửa.
    pub fn push_undo(&mut self, label: &str, merge_key: Option<&str>) {
        let snapshot = self.state.clone();
        self.history.push(label, snapshot, merge_key);
        self.modified = true;
        self.revision += 1;
    }

    pub fn undo(&mut self) -> bool {
        let current = self.state.clone();
        match self.history.undo(current) {
            Some(prev) => {
                self.state = prev;
                self.modified = true;
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        let current = self.state.clone();
        match self.history.redo(current) {
            Some(next) => {
                self.state = next;
                self.modified = true;
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    pub fn selected_layer(&self) -> Option<&Layer> {
        let id = self.selected?;
        self.state.layers.iter().find(|l| l.id() == id)
    }

    pub fn selected_layer_mut(&mut self) -> Option<&mut Layer> {
        let id = self.selected?;
        self.state.layers.iter_mut().find(|l| l.id() == id)
    }

    /// Layer trên cùng chứa điểm `(x, y)` (px ảnh).
    pub fn layer_at(&self, x: f64, y: f64) -> Option<u64> {
        self.state
            .layers
            .iter()
            .rev()
            .find(|l| {
                let (bx, by, bw, bh) = l.bounds();
                l.visible() && x >= bx && y >= by && x <= bx + bw && y <= by + bh
            })
            .map(|l| l.id())
    }

    pub fn add_image_layer(&mut self, img: RgbaImage, name: &str) {
        self.push_undo("Insert image", None);
        let (cw, ch) = self.size();
        // vừa trong 60 % canvas, giữ tỉ lệ
        let k = ((cw as f64 * 0.6) / img.w as f64)
            .min((ch as f64 * 0.6) / img.h as f64)
            .min(1.0);
        let id = new_id();
        self.state.layers.push(Layer::Image(ImageLayer {
            id,
            name: name.to_string(),
            visible: true,
            opacity: 1.0,
            width: img.w as f64 * k,
            height: img.h as f64 * k,
            img,
            x: cw as f64 / 2.0,
            y: ch as f64 / 2.0,
            rotation: 0.0,
            flip_h: false,
            flip_v: false,
        }));
        self.selected = Some(id);
    }

    pub fn add_blur_layer(&mut self, rect: (f64, f64, f64, f64), mode: &str, strength: f64) {
        self.push_undo("Blur area", None);
        let id = new_id();
        self.state.layers.push(Layer::Blur(BlurLayer {
            id,
            name: format!("Blur {id}"),
            visible: true,
            opacity: 1.0,
            shape: "rect".to_string(),
            mode: mode.to_string(),
            strength,
            rect,
        }));
        self.selected = Some(id);
    }

    pub fn delete_selected(&mut self) {
        let Some(id) = self.selected else { return };
        self.push_undo("Delete layer", None);
        self.state.layers.retain(|l| l.id() != id);
        self.selected = None;
    }

    pub fn duplicate_selected(&mut self) {
        let Some(id) = self.selected else { return };
        let Some(pos) = self.state.layers.iter().position(|l| l.id() == id) else {
            return;
        };
        self.push_undo("Duplicate layer", None);
        let mut copy = self.state.layers[pos].clone();
        let new = new_id();
        match &mut copy {
            Layer::Image(l) => {
                l.id = new;
                l.x += 24.0;
                l.y += 24.0;
            }
            Layer::Blur(l) => {
                l.id = new;
                l.rect.0 += 24.0;
                l.rect.1 += 24.0;
            }
        }
        self.state.layers.insert(pos + 1, copy);
        self.selected = Some(new);
    }

    pub fn move_selected(&mut self, delta: i64) {
        let Some(id) = self.selected else { return };
        let Some(pos) = self.state.layers.iter().position(|l| l.id() == id) else {
            return;
        };
        let new = pos as i64 + delta;
        if new < 0 || new >= self.state.layers.len() as i64 {
            return;
        }
        self.push_undo("Reorder layers", None);
        let l = self.state.layers.remove(pos);
        self.state.layers.insert(new as usize, l);
    }

    // --- Phép trên cả canvas ---------------------------------------------------------------

    /// Cắt canvas về `(x, y, w, h)` px ảnh; mọi layer dịch theo.
    pub fn crop(&mut self, x: usize, y: usize, w: usize, h: usize) {
        if w == 0 || h == 0 {
            return;
        }
        self.push_undo("Crop", None);
        self.state.background.img = self.state.background.img.crop(x, y, w, h);
        let (dx, dy) = (-(x as f64), -(y as f64));
        for l in &mut self.state.layers {
            l.translate(dx, dy);
        }
    }

    /// Xoay canvas 90°.
    pub fn rotate90(&mut self, clockwise: bool) {
        self.push_undo("Rotate 90°", None);
        let size = self.size();
        self.state.background.img = rotate90_image(&self.state.background.img, clockwise);
        let f = move |x: f64, y: f64| rotate90_point(x, y, size, clockwise);
        let rot = if clockwise { 90.0 } else { -90.0 };
        for l in &mut self.state.layers {
            l.map_points(&f, rot, "");
        }
    }

    /// Lật canvas.
    pub fn flip(&mut self, horizontal: bool) {
        self.push_undo("Flip", None);
        let size = self.size();
        self.state.background.img = flip_image(&self.state.background.img, horizontal);
        let f = move |x: f64, y: f64| flip_point(x, y, size, horizontal);
        let mirror = if horizontal { "h" } else { "v" };
        for l in &mut self.state.layers {
            l.map_points(&f, 0.0, mirror);
        }
    }
}

/// Xoay ảnh 90° (nguồn `(h, w)` → đích `(w, h)`).
pub fn rotate90_image(img: &RgbaImage, clockwise: bool) -> RgbaImage {
    let mut out = RgbaImage::new(img.w, img.h);
    for y in 0..img.h {
        for x in 0..img.w {
            let p = img.px(y, x);
            let (nx, ny) = if clockwise {
                (img.h - 1 - y, x)
            } else {
                (y, img.w - 1 - x)
            };
            out.set_px(ny, nx, p);
        }
    }
    out
}

pub fn flip_image(img: &RgbaImage, horizontal: bool) -> RgbaImage {
    let mut out = RgbaImage::new(img.h, img.w);
    for y in 0..img.h {
        for x in 0..img.w {
            let p = img.px(y, x);
            if horizontal {
                out.set_px(y, img.w - 1 - x, p);
            } else {
                out.set_px(img.h - 1 - y, x, p);
            }
        }
    }
    out
}
