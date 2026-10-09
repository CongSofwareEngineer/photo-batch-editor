//! Test phần **CPU** của GUI (không mở cửa sổ): framebuffer, ghép tài liệu photo editor,
//! thu nhỏ ảnh xem trước. Đây là chỗ quyết định pixel người dùng thấy và pixel xuất ra file,
//! nên phải có test; phần vẽ widget của egui không test ở đây.

use pbe_core::photo::effects::PhotoAdjust;
use pbe_core::photo::RgbaImage;
use pbe_gui::fb::Framebuffer;
use pbe_gui::photo::doc::{BlurLayer, Document, Layer};
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
