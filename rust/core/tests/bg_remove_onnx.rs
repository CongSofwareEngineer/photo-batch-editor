//! Tách nền bằng MODNet thật (ONNX). Chạy: `cargo test -p pbe-core --features bgremove --test bg_remove_onnx`.
//!
//! Kiểm tra thủ công trên ảnh chân dung thật: đặt `PBE_BG_SAMPLE=<ảnh>` và `PBE_BG_OUT=<thư mục>`
//! → ghi `matte.png`, `transparent.png`, `blue.jpg` để xem bằng mắt.
#![cfg(feature = "bgremove")]

use std::path::PathBuf;

use pbe_core::bg_remove::{default_model_path, BackgroundRemover};
use pbe_core::photo::image_io::{load_rgba, save_image};
use pbe_core::photo::matting::{apply_cutout, composite_backdrop, Backdrop, Cutout};
use pbe_core::photo::RgbaImage;

#[test]
fn modnet_returns_matte_of_image_size() {
    if !default_model_path().is_file() {
        eprintln!(
            "skip: model not found at {}",
            default_model_path().display()
        );
        return;
    }
    let remover = BackgroundRemover::new(None).expect("load model");
    // ảnh tổng hợp không vuông, không chia hết 32: một "đầu + vai" tối trên nền sáng
    let (h, w) = (300, 217);
    let mut img = RgbaImage::filled(h, w, [235, 235, 240, 255]);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f64 - 108.0, y as f64 - 110.0);
            let head = dx * dx / 45.0f64.powi(2) + dy * dy / 60.0f64.powi(2) <= 1.0;
            let body = y > 180 && (dx.abs() < 40.0 + (y - 180) as f64);
            if head || body {
                img.set_px(y, x, [120, 90, 80, 255]);
            }
        }
    }
    let matte = remover.matte(&img).expect("run model");
    assert_eq!((matte.h, matte.w), (h, w));
    assert!(matte.data.iter().any(|&v| v > 0) || matte.data.iter().all(|&v| v == 0));

    // Ảnh thật (tuỳ chọn) để xem bằng mắt.
    let (Ok(sample), Ok(out)) = (std::env::var("PBE_BG_SAMPLE"), std::env::var("PBE_BG_OUT"))
    else {
        return;
    };
    let out = PathBuf::from(out);
    let img = load_rgba(std::path::Path::new(&sample)).expect("read sample");
    let t = std::time::Instant::now();
    let matte = remover.matte(&img).expect("run model");
    eprintln!("matte {}x{} in {:?}", img.w, img.h, t.elapsed());
    let gray = RgbaImage::from_vec(
        matte.h,
        matte.w,
        matte.data.iter().flat_map(|&v| [v, v, v, 255]).collect(),
    );
    save_image(&out.join("matte.png"), &gray, "png", 100).unwrap();
    let cutout = Cutout::new(matte);
    let mut person = img.clone();
    apply_cutout(&mut person, &cutout, 1.0);
    save_image(&out.join("transparent.png"), &person, "png", 100).unwrap();
    let blue = composite_backdrop(&person, &Backdrop::Color([0x1e, 0x6f, 0xd9]));
    save_image(&out.join("blue.jpg"), &blue, "jpeg", 92).unwrap();
    let mut raw = img.clone();
    let mut no_dc = cutout.clone();
    no_dc.settings.decontaminate = false;
    apply_cutout(&mut raw, &no_dc, 1.0);
    let blue_raw = composite_backdrop(&raw, &Backdrop::Color([0x1e, 0x6f, 0xd9]));
    save_image(
        &out.join("blue_no_decontaminate.jpg"),
        &blue_raw,
        "jpeg",
        92,
    )
    .unwrap();
}
