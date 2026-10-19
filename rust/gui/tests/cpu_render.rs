//! Test phần **CPU** của GUI (không mở cửa sổ): framebuffer, ghép tài liệu photo editor,
//! thu nhỏ ảnh xem trước. Đây là chỗ quyết định pixel người dùng thấy và pixel xuất ra file,
//! nên phải có test; phần vẽ widget của egui không test ở đây.

use pbe_core::photo::effects::PhotoAdjust;
use pbe_core::photo::matting::{Backdrop, Cutout, Matte};
use pbe_core::photo::skin::SkinSmoothSettings;
use pbe_core::photo::RgbaImage;
use pbe_gui::fb::Framebuffer;
use pbe_gui::photo::doc::{BlurLayer, CloneStroke, Document, Layer, BACKGROUND_ID};
use pbe_gui::photo::render;
use pbe_gui::preview;

fn checker(h: usize, w: usize) -> RgbaImage {
    let mut img = RgbaImage::new(h, w);
    for y in 0..h {
        for x in 0..w {
            let v = if (x / 8 + y / 8) % 2 == 0 { 230 } else { 30 };
            img.set_px(y, x, [v, (x * 3 % 256) as u8, (y * 7 % 256) as u8, 255]);
        }
    }
    img
}

fn rgb_std(img: &RgbaImage) -> f64 {
    let vals: Vec<f64> = (0..img.h * img.w)
        .flat_map(|i| (0..3).map(move |c| (i, c)))
        .map(|(i, c)| img.data[i * 4 + c] as f64)
        .collect();
    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
    (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64).sqrt()
}

#[test]
fn framebuffer_blends_and_clips() {
    let mut fb = Framebuffer::new(10, 10);
    fb.clear([0, 0, 0, 255]);
    // nửa trong suốt trắng trên nền đen -> xám
    fb.fill_rect(0.0, 0.0, 4.0, 4.0, [255, 255, 255, 128]);
    let i = 0;
    assert!((120..=135).contains(&fb.data[i]), "trộn alpha sai: {}", fb.data[i]);
    // ra ngoài khung không được panic và không ghi đâu khác
    fb.fill_rect(-50.0, -50.0, 5.0, 5.0, [255, 0, 0, 255]);
    fb.fill_rect(100.0, 100.0, 5.0, 5.0, [255, 0, 0, 255]);
    assert_eq!(fb.data.len(), 10 * 10 * 4);
}

#[test]
fn framebuffer_draw_image_centres_and_scales() {
    let img = RgbaImage::filled(4, 4, [10, 200, 30, 255]);
    let mut fb = Framebuffer::new(20, 20);
    fb.clear([0, 0, 0, 255]);
    fb.draw_image(&img, 5.0, 5.0, 10.0, 10.0, 1.0);
    let inside = (10 * 20 + 10) * 4;
    assert_eq!(&fb.data[inside..inside + 3], &[10, 200, 30]);
    let outside = (20 + 1) * 4;
    assert_eq!(&fb.data[outside..outside + 3], &[0, 0, 0]);
}

#[test]
fn framebuffer_draw_image_at_half_opacity() {
    let img = RgbaImage::filled(4, 4, [255, 255, 255, 255]);
    let mut fb = Framebuffer::new(8, 8);
    fb.clear([0, 0, 0, 255]);
    fb.draw_image(&img, 0.0, 0.0, 8.0, 8.0, 0.5);
    assert!((120..=135).contains(&fb.data[0]), "opacity sai: {}", fb.data[0]);
}

#[test]
fn preview_downscale_keeps_aspect_and_never_enlarges() {
    let img = checker(600, 900);
    let small = preview::downscale_to(&img, 300);
    assert_eq!((small.w, small.h), (300, 200));
    // không phóng to
    let same = preview::downscale_to(&img, 5000);
    assert_eq!((same.w, same.h), (900, 600));
}

#[test]
fn render_at_half_scale_matches_canvas_size() {
    let doc = Document::new(checker(400, 600), None);
    let half = render::render(&doc.state, 0.5);
    assert_eq!((half.w, half.h), (300, 200));
    let full = render::render(&doc.state, 1.0);
    assert_eq!((full.w, full.h), (600, 400));
}

#[test]
fn render_applies_background_adjust() {
    let mut doc = Document::new(checker(80, 120), None);
    let plain = render::render(&doc.state, 1.0);
    doc.state.background.adjust = PhotoAdjust {
        brightness: 60.0,
        ..Default::default()
    };
    let bright = render::render(&doc.state, 1.0);
    let mean = |img: &RgbaImage| {
        img.data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0] as f64 + p[1] as f64 + p[2] as f64)
            .sum::<f64>()
            / (img.h * img.w) as f64
    };
    assert!(mean(&bright) > mean(&plain), "Brightness không làm ảnh sáng hơn");
}

#[test]
fn render_blur_layer_smooths_only_its_rect() {
    let mut doc = Document::new(checker(100, 100), None);
    doc.add_blur_layer((10.0, 10.0, 40.0, 40.0), "blur", 80.0);
    let out = render::render(&doc.state, 1.0);
    let inside = out.crop(14, 14, 32, 32);
    let outside = out.crop(60, 60, 32, 32);
    let base = render::render(&Document::new(checker(100, 100), None).state, 1.0);
    let (blurred_std, base_std) = (rgb_std(&inside), rgb_std(&base.crop(14, 14, 32, 32)));
    assert!(
        blurred_std < base_std * 0.85,
        "trong vùng chưa mờ: {blurred_std:.1} so với {base_std:.1}"
    );
    assert_eq!(outside.data, base.crop(60, 60, 32, 32).data, "ngoài vùng bị đổi");
}

#[test]
fn render_image_layer_respects_visibility_and_opacity() {
    let mut doc = Document::new(RgbaImage::filled(60, 60, [0, 0, 0, 255]), None);
    doc.add_image_layer(RgbaImage::filled(20, 20, [255, 255, 255, 255]), "logo");
    let shown = render::render(&doc.state, 1.0);
    let centre = shown.px(30, 30);
    assert!(centre[0] > 200, "layer không được vẽ: {centre:?}");

    if let Some(l) = doc.state.layers.first_mut() {
        l.set_visible(false);
    }
    let hidden = render::render(&doc.state, 1.0);
    assert_eq!(hidden.px(30, 30), [0, 0, 0, 255]);

    if let Some(l) = doc.state.layers.first_mut() {
        l.set_visible(true);
        l.set_opacity(0.5);
    }
    let half = render::render(&doc.state, 1.0);
    let c = half.px(30, 30)[0];
    assert!((110..=145).contains(&c), "opacity layer sai: {c}");
}

#[test]
fn crop_rotate_flip_move_layers_with_the_canvas() {
    let mut doc = Document::new(checker(200, 300), None);
    doc.add_blur_layer((100.0, 50.0, 40.0, 30.0), "blur", 50.0);

    doc.crop(50, 20, 200, 150);
    assert_eq!(doc.size(), (200, 150));
    let Layer::Blur(BlurLayer { rect, .. }) = &doc.state.layers[0] else {
        panic!("phải là blur layer");
    };
    assert_eq!(*rect, (50.0, 30.0, 40.0, 30.0));

    doc.rotate90(true);
    assert_eq!(doc.size(), (150, 200));
    doc.flip(true);
    assert_eq!(doc.size(), (150, 200));
    // layer vẫn nằm trong canvas sau mọi phép trên
    let (x, y, w, h) = doc.state.layers[0].bounds();
    assert!(x >= -1.0 && y >= -1.0 && x + w <= 151.0 && y + h <= 201.0, "layer ra ngoài: {x},{y},{w},{h}");
}

#[test]
fn undo_redo_restores_pixels() {
    let mut doc = Document::new(checker(60, 60), None);
    let before = render::render(&doc.state, 1.0);
    doc.add_blur_layer((5.0, 5.0, 30.0, 30.0), "pixelate", 70.0);
    let after = render::render(&doc.state, 1.0);
    assert_ne!(before.data, after.data);
    assert!(doc.undo());
    assert_eq!(render::render(&doc.state, 1.0).data, before.data);
    assert!(doc.redo());
    assert_eq!(render::render(&doc.state, 1.0).data, after.data);
}

#[test]
fn rotate_cover_fills_the_frame() {
    let img = RgbaImage::filled(100, 160, [200, 100, 50, 255]);
    let out = render::rotate_cover(&img, 12.0);
    assert_eq!((out.w, out.h), (160, 100));
    // không còn pixel trong suốt ở góc (ảnh được phóng để phủ kín)
    for (y, x) in [(0, 0), (0, out.w - 1), (out.h - 1, 0), (out.h - 1, out.w - 1)] {
        assert_eq!(out.px(y, x)[3], 255, "góc ({y},{x}) bị hở");
    }
}

#[test]
fn render_applies_unsharp_mask_at_every_scale() {
    use pbe_core::photo::effects::SharpenSettings;
    let mut doc = Document::new(checker(80, 120), None);
    let plain = render::render(&doc.state, 1.0);
    doc.state.background.sharpen = SharpenSettings {
        amount: 200.0,
        radius: 2.0,
        threshold: 0.0,
    };
    let sharp = render::render(&doc.state, 1.0);
    assert!(rgb_std(&sharp) > rgb_std(&plain));
    // xem trước nửa tỉ lệ vẫn làm nét (bán kính thu theo tỉ lệ)
    let half_plain = render::render(&Document::new(checker(80, 120), None).state, 0.5);
    assert!(rgb_std(&render::render(&doc.state, 0.5)) > rgb_std(&half_plain));
}

#[test]
fn layer_set_bounds_resizes_image_and_blur_layers() {
    let mut doc = Document::new(checker(100, 100), None);
    doc.add_image_layer(RgbaImage::filled(10, 20, [255, 0, 0, 255]), "logo");
    let l = doc.selected_layer_mut().unwrap();
    l.set_bounds(10.0, 20.0, 40.0, 30.0);
    assert_eq!(l.bounds(), (10.0, 20.0, 40.0, 30.0));
    doc.add_blur_layer((0.0, 0.0, 10.0, 10.0), "blur", 40.0);
    let b = doc.selected_layer_mut().unwrap();
    b.set_bounds(5.0, 6.0, 0.0, 7.0); // cỡ tối thiểu 1 px
    assert_eq!(b.bounds(), (5.0, 6.0, 1.0, 7.0));
}

/// Matte "người" = nửa trái của ảnh `h × w`.
fn left_half_matte(h: usize, w: usize) -> Matte {
    let data = (0..h * w).map(|i| if i % w < w / 2 { 255 } else { 0 }).collect();
    Matte::new(h, w, data)
}

#[test]
fn render_cutout_replaces_background_at_every_scale() {
    let mut doc = Document::new(RgbaImage::filled(40, 80, [200, 100, 50, 255]), None);
    let mut cut = Cutout::new(left_half_matte(40, 80));
    cut.settings.decontaminate = false;
    cut.backdrop = Backdrop::Color([0, 0, 255]);
    doc.state.background.cutout = Some(cut);
    for scale in [1.0, 0.5] {
        let out = render::render(&doc.state, scale);
        let (h, w) = (out.h, out.w);
        assert_eq!(out.px(h / 2, 2), [200, 100, 50, 255], "người giữ nguyên");
        assert_eq!(out.px(h / 2, w - 3), [0, 0, 255, 255], "nền được thay");
    }
    // nền trong suốt → alpha 0 ở phông cũ (xuất PNG giữ trong suốt)
    if let Some(c) = &mut doc.state.background.cutout {
        c.backdrop = Backdrop::Transparent;
    }
    let out = render::render(&doc.state, 1.0);
    assert_eq!(out.px(20, 79)[3], 0);
    assert_eq!(out.px(20, 0)[3], 255);
}

#[test]
fn cutout_matte_follows_crop_rotate_flip_and_undo() {
    let mut doc = Document::new(RgbaImage::filled(40, 80, [200, 100, 50, 255]), None);
    let mut cut = Cutout::new(left_half_matte(40, 80));
    cut.settings.decontaminate = false;
    doc.state.background.cutout = Some(cut);
    let matte_size = |d: &Document| {
        let m = &d.state.background.cutout.as_ref().unwrap().matte;
        (m.w, m.h)
    };
    doc.rotate90(true);
    assert_eq!(matte_size(&doc), doc.size());
    // xoay phải: nửa trái (người) thành nửa trên
    let out = render::render(&doc.state, 1.0);
    assert_eq!(out.px(2, 20)[3], 255);
    assert_eq!(out.px(77, 20)[3], 0);
    doc.flip(false); // lật dọc: người xuống nửa dưới
    let out = render::render(&doc.state, 1.0);
    assert_eq!(out.px(77, 20)[3], 255);
    doc.crop(0, 40, 40, 40); // chỉ còn phần người
    assert_eq!(matte_size(&doc), (40, 40));
    assert!(render::render(&doc.state, 1.0).data.chunks(4).all(|p| p[3] == 255));
    // undo hết → về ảnh gốc, matte gốc
    while doc.undo() {}
    assert_eq!(matte_size(&doc), (80, 40));
}

// --- Thứ tự layer: nền đưa lên trên / xuống dưới layer khác ---------------------------------

#[test]
fn background_can_move_above_layers_and_back() {
    let mut doc = Document::new(RgbaImage::filled(40, 80, [200, 100, 50, 255]), None);
    doc.add_image_layer(RgbaImage::filled(10, 10, [0, 255, 0, 255]), "a");
    doc.add_image_layer(RgbaImage::filled(10, 10, [0, 0, 255, 255]), "b");
    let (a, b) = (doc.state.layers[0].id(), doc.state.layers[1].id());
    assert_eq!(doc.state.stack_ids(), vec![BACKGROUND_ID, a, b]);
    // mặc định: layer b (trên cùng) thấy ở giữa
    assert_eq!(render::render(&doc.state, 1.0).px(20, 40), [0, 0, 255, 255]);

    // đưa nền lên trên cùng → nền đục che hết layer
    doc.selected = Some(BACKGROUND_ID);
    doc.move_selected_to_end(true);
    assert_eq!(doc.state.stack_ids(), vec![a, b, BACKGROUND_ID]);
    assert_eq!(doc.state.bg_index, 2);
    assert_eq!(render::render(&doc.state, 1.0).px(20, 40), [200, 100, 50, 255]);

    // nền trong suốt một nửa (đã tách nền) → layer bên dưới lộ ra ở phần đã xoá
    let mut cut = Cutout::new(left_half_matte(40, 80));
    cut.settings.decontaminate = false;
    doc.state.background.cutout = Some(cut);
    let out = render::render(&doc.state, 1.0);
    assert_eq!(out.px(20, 41), [0, 0, 255, 255], "layer dưới nền phải lộ ở phần trong suốt");
    assert_eq!(out.px(20, 5), [200, 100, 50, 255]);
    doc.state.background.cutout = None;

    // opacity nền 50 % khi nằm trên layer
    doc.state.background.opacity = 0.5;
    let c = render::render(&doc.state, 1.0).px(20, 40);
    // nền [200,100,50] 50 % trên layer xanh [0,0,255]
    assert!((95..=105).contains(&c[0]) && (148..=156).contains(&c[2]), "trộn opacity sai: {c:?}");
    doc.state.background.opacity = 1.0;

    // một bậc xuống: nền nằm giữa a và b → b thấy, a bị che
    doc.move_selected(-1);
    assert_eq!(doc.state.stack_ids(), vec![a, BACKGROUND_ID, b]);
    assert_eq!(render::render(&doc.state, 1.0).px(20, 40), [0, 0, 255, 255]);

    // undo trả lại thứ tự cũ
    assert!(doc.undo());
    assert_eq!(doc.state.stack_ids(), vec![a, b, BACKGROUND_ID]);
}

#[test]
fn reorder_delete_duplicate_keep_background_slot() {
    let mut doc = Document::new(RgbaImage::filled(20, 20, [9, 9, 9, 255]), None);
    for n in ["a", "b", "c"] {
        doc.add_image_layer(RgbaImage::filled(4, 4, [255, 0, 0, 255]), n);
    }
    let ids: Vec<u64> = doc.state.layers.iter().map(|l| l.id()).collect();
    // đổi chỗ a lên trên b (ví dụ "ảnh 1 đè lên ảnh 2")
    doc.selected = Some(ids[0]);
    doc.move_selected(1);
    assert_eq!(doc.state.stack_ids(), vec![BACKGROUND_ID, ids[1], ids[0], ids[2]]);
    // kéo thả: c xuống dưới nền
    assert!(doc.move_to(ids[2], 0));
    assert_eq!(doc.state.stack_ids(), vec![ids[2], BACKGROUND_ID, ids[1], ids[0]]);
    assert_eq!(doc.state.bg_index, 1);
    // nhân đôi layer dưới nền → bản sao vẫn dưới nền
    doc.selected = Some(ids[2]);
    doc.duplicate_selected();
    assert_eq!(doc.state.bg_index, 2);
    // xoá layer dưới nền → nền tụt xuống
    doc.delete_selected();
    doc.selected = Some(ids[2]);
    doc.delete_selected();
    assert_eq!(doc.state.bg_index, 0);
    assert_eq!(doc.state.stack_ids(), vec![BACKGROUND_ID, ids[1], ids[0]]);
    // nền không xoá / nhân đôi được
    doc.selected = Some(BACKGROUND_ID);
    doc.delete_selected();
    doc.duplicate_selected();
    assert_eq!(doc.state.layers.len(), 2);
}

// --- Tem nhân bản ---------------------------------------------------------------------------

/// Nửa trái đỏ, nửa phải xanh.
fn two_tone(h: usize, w: usize) -> RgbaImage {
    let mut img = RgbaImage::new(h, w);
    for y in 0..h {
        for x in 0..w {
            img.set_px(y, x, if x < w / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] });
        }
    }
    img
}

#[test]
fn clone_stamp_copies_the_sample_at_every_scale() {
    let mut doc = Document::new(two_tone(60, 120), None);
    let id = doc.add_clone_layer();
    if let Some(Layer::Clone(l)) = doc.state.layers.iter_mut().find(|l| l.id() == id) {
        // tô ở bên phải (xanh), lấy mẫu lệch 60 px sang trái (đỏ)
        l.strokes.push(CloneStroke {
            offset: (-60.0, 0.0),
            points: vec![(80.0, 30.0), (100.0, 30.0)],
            radius: 8.0,
            hardness: 1.0,
            opacity: 1.0,
        });
    }
    for scale in [1.0, 0.5] {
        let out = render::render(&doc.state, scale);
        let s = |v: f64| (v * scale) as usize;
        assert_eq!(out.px(s(30.0), s(90.0)), [255, 0, 0, 255], "nét phải lấy màu đỏ (scale {scale})");
        assert_eq!(out.px(s(5.0), s(90.0)), [0, 0, 255, 255], "ngoài nét giữ nguyên");
        assert_eq!(out.px(s(30.0), s(10.0)), [255, 0, 0, 255]);
    }
    // ẩn layer / opacity
    let (b, _, w, _) = doc.state.layers[0].bounds();
    assert!((b - 72.0).abs() < 1e-9 && (w - 36.0).abs() < 1e-9);
    doc.state.layers[0].set_opacity(0.5);
    let c = render::render(&doc.state, 1.0).px(30, 90);
    assert!((120..=135).contains(&c[0]) && (120..=135).contains(&c[2]), "{c:?}");
}

#[test]
fn clone_stroke_follows_flip_and_move() {
    let mut doc = Document::new(two_tone(60, 120), None);
    let id = doc.add_clone_layer();
    if let Some(Layer::Clone(l)) = doc.state.layers.iter_mut().find(|l| l.id() == id) {
        l.strokes.push(CloneStroke {
            offset: (-60.0, 0.0),
            points: vec![(90.0, 30.0)],
            radius: 6.0,
            hardness: 1.0,
            opacity: 1.0,
        });
    }
    doc.flip(true); // đỏ sang phải, nét sang trái, độ lệch đổi chiều
    let Layer::Clone(l) = &doc.state.layers[0] else { panic!() };
    assert_eq!(l.strokes[0].offset, (60.0, 0.0));
    assert_eq!(render::render(&doc.state, 1.0).px(30, 30), [255, 0, 0, 255]);
    doc.state.layers[0].translate(0.0, 10.0);
    let Layer::Clone(l) = &doc.state.layers[0] else { panic!() };
    assert_eq!(l.strokes[0].points[0], (30.0, 40.0));
}

// --- Làm mịn da + bộ nhớ đệm nền -----------------------------------------------------------

#[test]
fn render_applies_skin_smoothing_and_cache_matches() {
    // da có nhiễu
    let mut img = RgbaImage::new(40, 40);
    for y in 0..40 {
        for x in 0..40 {
            let j = ((x * 37 + y * 91) % 21) as i32 - 10;
            let v = |c: i32| (c + j).clamp(0, 255) as u8;
            img.set_px(y, x, [v(224), v(172), v(140), 255]);
        }
    }
    let red_std = |img: &RgbaImage| {
        let v: Vec<f64> = img.data.as_chunks::<4>().0.iter().map(|p| p[0] as f64).collect();
        let m = v.iter().sum::<f64>() / v.len() as f64;
        (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    };
    let mut doc = Document::new(img, None);
    let before = red_std(&render::render(&doc.state, 1.0));
    doc.state.background.skin = SkinSmoothSettings {
        amount: 100.0,
        radius: 4.0,
        detail: 0.0,
        skin_only: true,
    };
    let smooth = render::render(&doc.state, 1.0);
    let after = red_std(&smooth);
    assert!(after < before * 0.6, "da chưa mịn hơn: {before:.2} → {after:.2}");

    let mut cache = render::BaseCache::default();
    assert_eq!(render::render_cached(&doc.state, 1.0, &mut cache), smooth);
    // đổi layer không đổi nền → vẫn đúng; đổi thông số → tính lại
    doc.add_image_layer(RgbaImage::filled(4, 4, [0, 0, 0, 255]), "x");
    assert_eq!(render::render_cached(&doc.state, 1.0, &mut cache), render::render(&doc.state, 1.0));
    doc.state.background.skin.amount = 0.0;
    assert_eq!(render::render_cached(&doc.state, 1.0, &mut cache), render::render(&doc.state, 1.0));
}

// --- Nhiều layer kiểu Photoshop -------------------------------------------------------------

#[test]
fn new_layers_go_right_above_the_selection() {
    let mut doc = Document::new(RgbaImage::filled(100, 100, [9, 9, 9, 255]), None);
    for n in ["1", "2", "3", "4"] {
        doc.add_image_layer(RgbaImage::filled(10, 10, [255, 0, 0, 255]), n);
    }
    let ids: Vec<u64> = doc.state.layers.iter().map(|l| l.id()).collect();
    // chọn ảnh 2 rồi chèn → nằm ngay trên ảnh 2, dưới ảnh 3
    doc.selected = Some(ids[1]);
    doc.add_image_layer(RgbaImage::filled(10, 10, [0, 255, 0, 255]), "5");
    let new = doc.selected.unwrap();
    assert_eq!(doc.state.stack_ids(), vec![BACKGROUND_ID, ids[0], ids[1], new, ids[2], ids[3]]);
    // chọn Nền → layer mới ngay trên Nền
    doc.selected = Some(BACKGROUND_ID);
    doc.add_image_layer(RgbaImage::filled(10, 10, [0, 0, 255, 255]), "6");
    assert_eq!(doc.state.stack_ids()[1], doc.selected.unwrap());
    // layer dưới Nền đang chọn → layer mới cũng dưới Nền
    assert!(doc.move_to(ids[3], 0));
    doc.selected = Some(ids[3]);
    doc.add_image_layer(RgbaImage::filled(10, 10, [0, 0, 255, 255]), "7");
    let s = doc.state.stack_ids();
    assert_eq!(&s[..3], &[ids[3], doc.selected.unwrap(), BACKGROUND_ID]);
    // đổi tên (1 bước undo), tên rỗng bị bỏ qua
    doc.rename_layer(ids[0], "  Logo  ");
    assert_eq!(doc.state.layers.iter().find(|l| l.id() == ids[0]).unwrap().name(), "Logo");
    doc.rename_layer(ids[0], " ");
    assert_eq!(doc.state.layers.iter().find(|l| l.id() == ids[0]).unwrap().name(), "Logo");
    assert!(doc.undo());
    assert_eq!(doc.state.layers.iter().find(|l| l.id() == ids[0]).unwrap().name(), "1");
}

#[test]
fn clicking_picks_the_top_layer_with_pixels() {
    let mut doc = Document::new(RgbaImage::filled(100, 100, [9, 9, 9, 255]), None);
    doc.add_image_layer(RgbaImage::filled(40, 40, [255, 0, 0, 255]), "red");
    let red = doc.selected.unwrap();
    // PNG trong suốt phủ lên, chỉ có pixel ở nửa trái
    let mut png = RgbaImage::new(40, 40);
    for y in 0..40 {
        for x in 0..20 {
            png.set_px(y, x, [0, 255, 0, 255]);
        }
    }
    doc.add_image_layer(png, "half");
    let half = doc.selected.unwrap();
    // cả hai cùng tâm (50, 50), cỡ 40 → nửa trái của PNG là x 30–50
    assert_eq!(doc.layer_at(35.0, 50.0), Some(half));
    assert_eq!(doc.layer_at(65.0, 50.0), Some(red), "chỗ trong suốt của PNG phải chọn layer dưới");
    assert_eq!(doc.layer_at(5.0, 5.0), None);
    // Nền đục đưa lên trên cùng → che hết, bấm không chọn được layer dưới
    doc.selected = Some(BACKGROUND_ID);
    doc.move_selected_to_end(true);
    assert_eq!(doc.layer_at(35.0, 50.0), None);
}
