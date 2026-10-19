//! Tài liệu của photo editor: ảnh nền + layer, undo/redo bằng ảnh chụp trạng thái.
//! Port của `ui/photo/document.py`.
//!
//! Ảnh gốc **không bao giờ bị ghi**: tài liệu giữ pixel đã giải mã; mỗi phép sửa tạo layer
//! hoặc ảnh mới, nên một ảnh chụp ([`DocState`]) chỉ là các struct nhỏ + `Vec<u8>` dùng chung
//! qua `clone` (rẻ vì chỉ copy khi thật sự sửa pixel).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use pbe_core::photo::effects::{PhotoAdjust, SharpenSettings};
use pbe_core::photo::geometry::{flip_point, normalize_angle, rotate90_point};
use pbe_core::photo::history::History;
use pbe_core::photo::matting::Cutout;
use pbe_core::photo::skin::SkinSmoothSettings;
use pbe_core::photo::RgbaImage;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Id của dòng "Nền" trong bảng Layer / ngăn xếp vẽ (id layer thật bắt đầu từ 1).
pub const BACKGROUND_ID: u64 = 0;

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
    /// `Arc`: ảnh chụp undo chỉ tăng bộ đếm tham chiếu, không copy pixel.
    pub img: Arc<RgbaImage>,
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

/// Một nét của Tem nhân bản: tô pixel lấy từ chỗ lệch `offset` (nguồn − đích) dọc đường `points`.
/// Xem docs/instruction/clone-stamp.md.
#[derive(Debug, Clone)]
pub struct CloneStroke {
    pub offset: (f64, f64),
    pub points: Vec<(f64, f64)>,
    pub radius: f64,   // px ảnh
    pub hardness: f64, // 0…1
    pub opacity: f32,  // 0…1
}

/// Layer Tem nhân bản: các nét dạng vector, lấy mẫu từ ảnh ghép **bên dưới** layer.
#[derive(Debug, Clone)]
pub struct CloneLayer {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub strokes: Vec<CloneStroke>,
}

#[derive(Debug, Clone)]
pub enum Layer {
    Image(ImageLayer),
    Blur(BlurLayer),
    Clone(CloneLayer),
}

impl Layer {
    pub fn id(&self) -> u64 {
        match self {
            Layer::Image(l) => l.id,
            Layer::Blur(l) => l.id,
            Layer::Clone(l) => l.id,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Layer::Image(_) => "image",
            Layer::Blur(_) => "blur",
            Layer::Clone(_) => "clone",
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Layer::Image(l) => &l.name,
            Layer::Blur(l) => &l.name,
            Layer::Clone(l) => &l.name,
        }
    }

    pub fn visible(&self) -> bool {
        match self {
            Layer::Image(l) => l.visible,
            Layer::Blur(l) => l.visible,
            Layer::Clone(l) => l.visible,
        }
    }

    pub fn set_visible(&mut self, v: bool) {
        match self {
            Layer::Image(l) => l.visible = v,
            Layer::Blur(l) => l.visible = v,
            Layer::Clone(l) => l.visible = v,
        }
    }

    pub fn opacity(&self) -> f32 {
        match self {
            Layer::Image(l) => l.opacity,
            Layer::Blur(l) => l.opacity,
            Layer::Clone(l) => l.opacity,
        }
    }

    pub fn set_opacity(&mut self, v: f32) {
        match self {
            Layer::Image(l) => l.opacity = v,
            Layer::Blur(l) => l.opacity = v,
            Layer::Clone(l) => l.opacity = v,
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
            Layer::Clone(l) => {
                let mut bb = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for st in &l.strokes {
                    for &(x, y) in &st.points {
                        bb = (
                            bb.0.min(x - st.radius),
                            bb.1.min(y - st.radius),
                            bb.2.max(x + st.radius),
                            bb.3.max(y + st.radius),
                        );
                    }
                }
                if bb.0 > bb.2 {
                    (0.0, 0.0, 0.0, 0.0)
                } else {
                    (bb.0, bb.1, bb.2 - bb.0, bb.3 - bb.1)
                }
            }
        }
    }

    /// Điểm `(x, y)` (px ảnh) có "trúng" layer không — như Auto-Select của Photoshop: layer ảnh chỉ
    /// tính chỗ có pixel (alpha > 10 %), bỏ qua phần trong suốt của PNG; vùng blur theo hình dạng;
    /// layer nhân bản theo nét tô.
    pub fn hit(&self, x: f64, y: f64) -> bool {
        let (bx, by, bw, bh) = self.bounds();
        match self {
            Layer::Image(l) => {
                if l.width <= 0.0 || l.height <= 0.0 || l.img.is_empty() {
                    return false;
                }
                // nghịch đảo của render::draw_transformed
                let a = l.rotation.to_radians();
                let (dx, dy) = (x - l.x, y - l.y);
                let lx = dx * a.cos() + dy * a.sin();
                let ly = -dx * a.sin() + dy * a.cos();
                let mut u = (lx + l.width / 2.0) / l.width * l.img.w as f64;
                let mut v = (ly + l.height / 2.0) / l.height * l.img.h as f64;
                if l.flip_h {
                    u = l.img.w as f64 - u;
                }
                if l.flip_v {
                    v = l.img.h as f64 - v;
                }
                if u < 0.0 || v < 0.0 || u >= l.img.w as f64 || v >= l.img.h as f64 {
                    return false;
                }
                l.img.px(v as usize, u as usize)[3] > 25
            }
            Layer::Blur(l) => {
                if !(x >= bx && y >= by && x <= bx + bw && y <= by + bh) {
                    return false;
                }
                if l.shape == "ellipse" && bw > 0.0 && bh > 0.0 {
                    let nx = (x - bx) / bw * 2.0 - 1.0;
                    let ny = (y - by) / bh * 2.0 - 1.0;
                    return nx * nx + ny * ny <= 1.0;
                }
                true
            }
            Layer::Clone(l) => l.strokes.iter().any(|st| {
                st.points.windows(2).any(|w| {
                    seg_dist((x, y), w[0], w[1]) <= st.radius
                }) || st.points.first().is_some_and(|p| seg_dist((x, y), *p, *p) <= st.radius)
            }),
        }
    }

    pub fn set_name(&mut self, name: &str) {
        match self {
            Layer::Image(l) => l.name = name.to_string(),
            Layer::Blur(l) => l.name = name.to_string(),
            Layer::Clone(l) => l.name = name.to_string(),
        }
    }

    /// Có nút góc đổi cỡ không (layer nhân bản chỉ di chuyển được).
    pub fn resizable(&self) -> bool {
        !matches!(self, Layer::Clone(_))
    }

    /// Đặt lại hình chữ nhật bao (px ảnh) — dùng khi kéo nút góc để đổi cỡ.
    pub fn set_bounds(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let (w, h) = (w.max(1.0), h.max(1.0));
        match self {
            Layer::Image(l) => {
                l.x = x + w / 2.0;
                l.y = y + h / 2.0;
                l.width = w;
                l.height = h;
            }
            Layer::Blur(l) => l.rect = (x, y, w, h),
            Layer::Clone(_) => {
                let (bx, by, _, _) = self.bounds();
                self.translate(x - bx, y - by);
            }
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
            Layer::Clone(l) => {
                for st in &mut l.strokes {
                    for p in &mut st.points {
                        p.0 += dx;
                        p.1 += dy;
                    }
                }
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
            Layer::Clone(l) => {
                for st in &mut l.strokes {
                    // độ lệch nguồn là một vectơ: xoay / lật theo canvas, crop thì giữ nguyên
                    if let Some(&(px, py)) = st.points.first() {
                        let (ax, ay) = f(px, py);
                        let (bx, by) = f(px + st.offset.0, py + st.offset.1);
                        st.offset = (bx - ax, by - ay);
                    }
                    for p in &mut st.points {
                        *p = f(p.0, p.1);
                    }
                }
            }
        }
    }
}

/// Khoảng cách từ `p` tới đoạn `a–b`.
fn seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let len2 = vx * vx + vy * vy;
    let t = if len2 > 0.0 {
        (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((p.0 - a.0 - t * vx).powi(2) + (p.1 - a.1 - t * vy).powi(2)).sqrt()
}

/// Ảnh nền + chỉnh sửa áp lên nó.
#[derive(Debug, Clone)]
pub struct Background {
    /// `Arc`: ảnh chụp undo chỉ tăng bộ đếm tham chiếu, không copy pixel.
    pub img: Arc<RgbaImage>,
    pub adjust: PhotoAdjust,
    pub skin: SkinSmoothSettings, // làm mịn da (xem docs/instruction/skin-smoothing.md)
    pub sharpen: SharpenSettings, // Unsharp Mask (xem docs/instruction/sharpen.md)
    pub angle: f64, // xoay tự do (làm thẳng), độ; ảnh được phóng để phủ kín
    pub visible: bool,
    /// Độ trong suốt khi nền được đưa lên trên layer khác (`DocState::bg_index > 0`).
    pub opacity: f32,
    /// Tách nền: matte + tinh chỉnh viền + nền thay thế (xem docs/instruction/background-removal.md).
    /// Matte luôn cùng cỡ `img` (crop / xoay 90° / lật áp cho cả hai).
    pub cutout: Option<Cutout>,
}

/// Ảnh chụp toàn bộ tài liệu — đơn vị undo / redo.
#[derive(Debug, Clone)]
pub struct DocState {
    pub background: Background,
    pub layers: Vec<Layer>, // dưới → trên
    /// Số layer vẽ **dưới** nền (0 = nền ở dưới cùng như Photoshop). Đổi bằng ▲ ▼ trên dòng "Nền".
    pub bg_index: usize,
}

impl DocState {
    /// `bg_index` đã kẹp vào số layer.
    pub fn bg_pos(&self) -> usize {
        self.bg_index.min(self.layers.len())
    }

    /// Thứ tự vẽ dưới → trên gồm cả nền ([`BACKGROUND_ID`]).
    pub fn stack_ids(&self) -> Vec<u64> {
        let b = self.bg_pos();
        let mut ids: Vec<u64> = self.layers[..b].iter().map(Layer::id).collect();
        ids.push(BACKGROUND_ID);
        ids.extend(self.layers[b..].iter().map(Layer::id));
        ids
    }

    /// Xếp lại layer theo `ids` (dưới → trên, gồm [`BACKGROUND_ID`]).
    fn set_stack(&mut self, ids: &[u64]) {
        let mut old = std::mem::take(&mut self.layers);
        for &id in ids {
            if id == BACKGROUND_ID {
                self.bg_index = self.layers.len();
            } else if let Some(i) = old.iter().position(|l| l.id() == id) {
                self.layers.push(old.remove(i));
            }
        }
        self.layers.extend(old);
    }
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
                    img: Arc::new(img),
                    adjust: PhotoAdjust::default(),
                    skin: SkinSmoothSettings::default(),
                    sharpen: SharpenSettings::default(),
                    angle: 0.0,
                    visible: true,
                    opacity: 1.0,
                    cutout: None,
                },
                layers: vec![],
                bg_index: 0,
            },
            history: History::default(),
            source,
            name,
            selected: None,
            modified: false,
            revision: 1,
        }
    }

    /// Tài liệu mới chưa có file nguồn (vd. ảnh ghép): đặt tên và đánh dấu chưa lưu.
    pub fn new_untitled(img: RgbaImage, name: &str) -> Self {
        let mut doc = Document::new(img, None);
        doc.name = name.to_string();
        doc.modified = true;
        doc
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

    /// Layer hiện trên cùng có pixel tại `(x, y)` (px ảnh) — xem [`Layer::hit`]. Layer nằm dưới nền
    /// đục (và nền không trong suốt tại điểm đó) bị che nên không chọn được bằng chuột.
    pub fn layer_at(&self, x: f64, y: f64) -> Option<u64> {
        let b = self.state.bg_pos();
        let above = self.state.layers[b..].iter().rev().find(|l| l.visible() && l.hit(x, y));
        if let Some(l) = above {
            return Some(l.id());
        }
        let bg = &self.state.background;
        let covered = bg.visible && bg.cutout.is_none() && bg.opacity >= 1.0 && {
            let (xi, yi) = (x.floor(), y.floor());
            xi >= 0.0
                && yi >= 0.0
                && (xi as usize) < bg.img.w
                && (yi as usize) < bg.img.h
                && bg.img.px(yi as usize, xi as usize)[3] > 25
        };
        if covered {
            return None;
        }
        self.state.layers[..b]
            .iter()
            .rev()
            .find(|l| l.visible() && l.hit(x, y))
            .map(|l| l.id())
    }

    /// Thêm layer **ngay trên** mục đang chọn (layer hoặc nền), như Photoshop; không chọn gì → trên
    /// cùng. Chọn layer mới. (Không ghi undo — hàm gọi ghi.)
    fn insert_layer(&mut self, layer: Layer) {
        let id = layer.id();
        let b = self.state.bg_pos();
        let sel_pos = self
            .selected
            .and_then(|sel| self.state.layers.iter().position(|l| l.id() == sel));
        let pos = match (self.selected, sel_pos) {
            (Some(BACKGROUND_ID), _) => b, // ngay trên nền
            (_, Some(p)) => p + 1,
            _ => self.state.layers.len(),
        };
        // layer đang chọn nằm dưới nền → layer mới cũng nằm dưới nền
        if sel_pos.is_some_and(|p| p < b) {
            self.state.bg_index = b + 1;
        }
        self.state.layers.insert(pos, layer);
        self.selected = Some(id);
    }

    /// Đổi tên layer `id` (tên rỗng → giữ tên cũ). 1 bước undo.
    pub fn rename_layer(&mut self, id: u64, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if self.state.layers.iter().any(|l| l.id() == id && l.name() != name) {
            self.push_undo("Rename layer", None);
            if let Some(l) = self.state.layers.iter_mut().find(|l| l.id() == id) {
                l.set_name(name);
            }
        }
    }

    pub fn add_image_layer(&mut self, img: RgbaImage, name: &str) {
        self.push_undo("Insert image", None);
        let (cw, ch) = self.size();
        // vừa trong 60 % canvas, giữ tỉ lệ
        let k = ((cw as f64 * 0.6) / img.w as f64)
            .min((ch as f64 * 0.6) / img.h as f64)
            .min(1.0);
        let id = new_id();
        self.insert_layer(Layer::Image(ImageLayer {
            id,
            name: name.to_string(),
            visible: true,
            opacity: 1.0,
            width: img.w as f64 * k,
            height: img.h as f64 * k,
            img: Arc::new(img),
            x: cw as f64 / 2.0,
            y: ch as f64 / 2.0,
            rotation: 0.0,
            flip_h: false,
            flip_v: false,
        }));
    }

    pub fn add_blur_layer(&mut self, rect: (f64, f64, f64, f64), mode: &str, strength: f64) {
        self.push_undo("Blur area", None);
        let id = new_id();
        self.insert_layer(Layer::Blur(BlurLayer {
            id,
            name: format!("Blur {id}"),
            visible: true,
            opacity: 1.0,
            shape: "rect".to_string(),
            mode: mode.to_string(),
            strength,
            rect,
        }));
    }

    /// Thêm một layer Tem nhân bản rỗng ngay trên mục đang chọn (chưa ghi undo — nét đầu tiên ghi) → id.
    pub fn add_clone_layer(&mut self) -> u64 {
        let id = new_id();
        let n = self
            .state
            .layers
            .iter()
            .filter(|l| matches!(l, Layer::Clone(_)))
            .count();
        self.insert_layer(Layer::Clone(CloneLayer {
            id,
            name: format!("Clone {}", n + 1),
            visible: true,
            opacity: 1.0,
            strokes: vec![],
        }));
        id
    }

    pub fn delete_selected(&mut self) {
        let Some(id) = self.selected else { return };
        let Some(pos) = self.state.layers.iter().position(|l| l.id() == id) else {
            return;
        };
        self.push_undo("Delete layer", None);
        self.state.layers.remove(pos);
        if pos < self.state.bg_pos() {
            self.state.bg_index = self.state.bg_pos() - 1;
        }
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
            Layer::Clone(l) => {
                l.id = new;
            }
        }
        if let Layer::Clone(_) = copy {
            copy.translate(24.0, 24.0);
        }
        if pos < self.state.bg_pos() {
            self.state.bg_index = self.state.bg_pos() + 1;
        }
        self.state.layers.insert(pos + 1, copy);
        self.selected = Some(new);
    }

    /// Đưa mục `id` (layer hoặc [`BACKGROUND_ID`]) tới vị trí `target` trong thứ tự vẽ dưới → trên
    /// (gồm cả nền). 1 bước undo; `false` nếu không đổi.
    pub fn move_to(&mut self, id: u64, target: usize) -> bool {
        let mut ids = self.state.stack_ids();
        let Some(pos) = ids.iter().position(|&i| i == id) else {
            return false;
        };
        let target = target.min(ids.len() - 1);
        if target == pos {
            return false;
        }
        self.push_undo("Reorder layers", None);
        let item = ids.remove(pos);
        ids.insert(target, item);
        self.state.set_stack(&ids);
        true
    }

    /// Đưa mục đang chọn (layer hoặc nền) lên (`delta > 0`) / xuống một bậc.
    pub fn move_selected(&mut self, delta: i64) {
        let Some(id) = self.selected else { return };
        let Some(pos) = self.state.stack_ids().iter().position(|&i| i == id) else {
            return;
        };
        let new = pos as i64 + delta;
        if new < 0 {
            return;
        }
        self.move_to(id, new as usize);
    }

    /// Đưa mục đang chọn lên trên cùng (`top`) hoặc xuống dưới cùng.
    pub fn move_selected_to_end(&mut self, top: bool) {
        let Some(id) = self.selected else { return };
        let target = if top { usize::MAX } else { 0 };
        self.move_to(id, target);
    }

    // --- Phép trên cả canvas ---------------------------------------------------------------

    /// Cắt canvas về `(x, y, w, h)` px ảnh; mọi layer dịch theo.
    pub fn crop(&mut self, x: usize, y: usize, w: usize, h: usize) {
        if w == 0 || h == 0 {
            return;
        }
        self.push_undo("Crop", None);
        self.state.background.img = Arc::new(self.state.background.img.crop(x, y, w, h));
        if let Some(c) = &mut self.state.background.cutout {
            c.matte = c.matte.crop(x, y, w, h);
        }
        let (dx, dy) = (-(x as f64), -(y as f64));
        for l in &mut self.state.layers {
            l.translate(dx, dy);
        }
    }

    /// Xoay canvas 90°.
    pub fn rotate90(&mut self, clockwise: bool) {
        self.push_undo("Rotate 90°", None);
        let size = self.size();
        self.state.background.img = Arc::new(rotate90_image(&self.state.background.img, clockwise));
        if let Some(c) = &mut self.state.background.cutout {
            c.matte = c.matte.rotate90(clockwise);
        }
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
        self.state.background.img = Arc::new(flip_image(&self.state.background.img, horizontal));
        if let Some(c) = &mut self.state.background.cutout {
            c.matte = c.matte.flip(horizontal);
        }
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
