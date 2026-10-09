//! Đọc và xuất một ảnh đơn (**giữ** độ trong suốt, khác với bộ đọc của chỉnh nhiều ảnh).
//! Port của `core/photo/image_io.py`.

use std::path::{Path, PathBuf};

use image::DynamicImage;

use crate::io_utils::{self, ImageReadError, Pixels};
use crate::photo::RgbaImage;

/// Thư mục xuất mặc định, nằm cạnh ảnh gốc (không bao giờ ghi đè ảnh gốc).
pub const EDITOR_FOLDER: &str = "editor";

/// Định dạng xuất → phần mở rộng.
pub const EXPORT_FORMATS: [(&str, &str); 2] = [("jpeg", ".jpg"), ("png", ".png")];

/// Phần mở rộng hiện trong hộp thoại Mở ảnh.
pub const OPEN_FILTER_EXTS: [&str; 7] = [".jpg", ".jpeg", ".png", ".tif", ".tiff", ".webp", ".bmp"];

fn export_suffix(fmt: &str) -> &'static str {
    EXPORT_FORMATS
        .iter()
        .find(|(name, _)| *name == fmt)
        .map(|(_, ext)| *ext)
        .unwrap_or(".jpg")
}

/// RGBA uint8 `(H, W, 4)` đã xoay theo EXIF; độ trong suốt được giữ (PNG, WebP…).
///
/// Ảnh độ sâu bit cao (xám 16-bit, float) không có đường RGBA trực tiếp → đi qua
/// [`crate::io_utils::read_image`] (ghép alpha lên nền trắng) rồi gắn alpha đục,
/// giống nhánh dự phòng của bản Python.
pub fn load_rgba(path: &Path) -> Result<RgbaImage, ImageReadError> {
    let bytes = std::fs::read(path).map_err(|e| ImageReadError(format!("Cannot read file: {e}")))?;
    if bytes.is_empty() {
        return Err(ImageReadError("Empty file".into()));
    }
    let decoded =
        image::load_from_memory(&bytes).map_err(|e| ImageReadError(format!("Corrupt file: {e}")))?;
    let orientation = io_utils::read_orientation(&bytes);
    let img = match to_rgba(decoded) {
        Some(img) => img,
        None => return load_rgba_via_rgb_reader(path), // 16-bit xám / float
    };
    Ok(oriented(img, orientation))
}

fn oriented(img: RgbaImage, orientation: u16) -> RgbaImage {
    let (h, w, data) = io_utils::apply_orientation_vec(&img.data, img.h, img.w, 4, orientation);
    RgbaImage::from_vec(h, w, data)
}

/// `None` khi dạng ảnh không map trực tiếp sang RGBA 8-bit (xám 16-bit, float).
fn to_rgba(img: DynamicImage) -> Option<RgbaImage> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let data = match img {
        DynamicImage::ImageLuma8(_)
        | DynamicImage::ImageLumaA8(_)
        | DynamicImage::ImageRgb8(_)
        | DynamicImage::ImageRgba8(_) => img.to_rgba8().into_raw(),
        // 16-bit màu: lấy byte cao, như Pillow khi mở TIFF 16-bit thành RGB 8-bit.
        DynamicImage::ImageRgb16(src) => src
            .pixels()
            .flat_map(|p| [(p.0[0] >> 8) as u8, (p.0[1] >> 8) as u8, (p.0[2] >> 8) as u8, 255])
            .collect(),
        DynamicImage::ImageRgba16(src) => src
            .pixels()
            .flat_map(|p| {
                [
                    (p.0[0] >> 8) as u8,
                    (p.0[1] >> 8) as u8,
                    (p.0[2] >> 8) as u8,
                    (p.0[3] >> 8) as u8,
                ]
            })
            .collect(),
        _ => return None,
    };
    Some(RgbaImage::from_vec(h, w, data))
}

fn load_rgba_via_rgb_reader(path: &Path) -> Result<RgbaImage, ImageReadError> {
    let loaded = io_utils::read_image(path)?; // đã xoay theo EXIF
    let (h, w) = loaded.pixels.dims();
    let mut data = Vec::with_capacity(h * w * 4);
    match &loaded.pixels {
        Pixels::U8(img) => {
            for i in 0..h * w {
                data.extend_from_slice(&img.data[i * 3..i * 3 + 3]);
                data.push(255);
            }
        }
        Pixels::U16(img) => {
            for i in 0..h * w {
                for c in 0..3 {
                    data.push((img.data[i * 3 + c] >> 8) as u8);
                }
                data.push(255);
            }
        }
        Pixels::F32(img) => {
            for i in 0..h * w {
                for c in 0..3 {
                    data.push((img.data[i * 3 + c].clamp(0.0, 1.0) * 255.0).round_ties_even() as u8);
                }
                data.push(255);
            }
        }
    }
    Ok(RgbaImage::from_vec(h, w, data))
}

/// `<thư mục của ảnh>/editor` (hoặc `~/Pictures/editor` cho ảnh mới chưa có nguồn).
pub fn editor_dir(source: Option<&Path>) -> PathBuf {
    if let Some(src) = source {
        return src
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default()
            .join(EDITOR_FOLDER);
    }
    let home = home_dir();
    let pictures = home.join("Pictures");
    let base = if pictures.is_dir() { pictures } else { home };
    base.join(EDITOR_FOLDER)
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Đường dẫn xuất mặc định: cùng tên ảnh gốc, trong thư mục `editor`.
pub fn default_export_path(source: Option<&Path>, fmt: &str, fallback_name: &str) -> PathBuf {
    let stem = source
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or(fallback_name)
        .to_string();
    editor_dir(source).join(format!("{stem}{}", export_suffix(fmt)))
}

/// RGBA → RGB ghép lên một màu nền đặc (JPEG không có độ trong suốt).
pub fn flatten_on(img: &RgbaImage, color: [u8; 3]) -> crate::adjustments::ImageU8 {
    let n = img.h * img.w;
    let mut data = vec![0u8; n * 3];
    for i in 0..n {
        let a = img.data[i * 4 + 3] as f32 / 255.0;
        for c in 0..3 {
            let v = img.data[i * 4 + c] as f32 * a + color[c] as f32 * (1.0 - a);
            data[i * 3 + c] = v.round_ties_even().clamp(0.0, 255.0) as u8;
        }
    }
    crate::adjustments::ImageU8 { h: img.h, w: img.w, data }
}

/// Màu nền khi ghép ảnh trong suốt để xuất JPEG.
pub const FLATTEN_COLOR: [u8; 3] = [255, 255, 255];

/// Ghi `img` thành JPEG (`quality` 1–100) hoặc PNG, qua file `.tmp` rồi đổi tên.
///
/// Bảo vệ ảnh gốc là việc của người gọi (đường dẫn mặc định nằm trong thư mục `editor`).
pub fn save_image(path: &Path, img: &RgbaImage, fmt: &str, quality: u8) -> Result<PathBuf, ImageReadError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ImageReadError(e.to_string()))?;
    }
    let tmp = tmp_path_for(path);
    let encoded = if fmt == "png" {
        encode_png(img)
    } else {
        encode_jpeg(img, quality)
    };
    let result = encoded.and_then(|bytes| {
        std::fs::write(&tmp, &bytes).map_err(|e| ImageReadError(e.to_string()))?;
        std::fs::rename(&tmp, path).map_err(|e| ImageReadError(e.to_string()))
    });
    if tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|_| path.to_path_buf())
}

/// `<đường dẫn>.tmp` — cùng quy ước với `core.io_utils.tmp_path_for`.
pub fn tmp_path_for(out_path: &Path) -> PathBuf {
    let name = out_path.file_name().and_then(|n| n.to_str()).unwrap_or("out");
    out_path.with_file_name(format!("{name}.tmp"))
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, ImageReadError> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;

    let mut buf: Vec<u8> = vec![];
    PngEncoder::new_with_quality(&mut buf, CompressionType::Default, FilterType::Adaptive)
        .write_image(
            &img.data,
            img.w as u32,
            img.h as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| ImageReadError(format!("PNG encode failed: {e}")))?;
    Ok(buf)
}

fn encode_jpeg(img: &RgbaImage, quality: u8) -> Result<Vec<u8>, ImageReadError> {
    use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

    let q = quality.clamp(1, 100);
    let rgb = flatten_on(img, FLATTEN_COLOR);
    let mut buf: Vec<u8> = vec![];
    let mut encoder = Encoder::new(&mut buf, q);
    // Giống bản Python: 4:4:4 từ chất lượng 90 trở lên, dưới đó 4:2:0 cho file nhỏ.
    encoder.set_sampling_factor(if q >= 90 {
        SamplingFactor::R_4_4_4
    } else {
        SamplingFactor::R_4_2_0
    });
    encoder
        .encode(&rgb.data, img.w as u16, img.h as u16, ColorType::Rgb)
        .map_err(|e| ImageReadError(format!("JPEG encode failed: {e}")))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn checker(h: usize, w: usize) -> RgbaImage {
        let mut img = RgbaImage::new(h, w);
        for y in 0..h {
            for x in 0..w {
                let v = if (x / 4 + y / 4) % 2 == 0 { 230 } else { 40 };
                img.set_px(y, x, [v, (x * 3 % 256) as u8, (y * 5 % 256) as u8, 255]);
            }
        }
        img
    }

    #[test]
    fn export_paths_use_editor_folder() {
        // Giống test_export_paths_use_editor_folder.
        let src = PathBuf::from("/tmp/shoot/ảnh 01.jpg");
        assert_eq!(editor_dir(Some(&src)), PathBuf::from("/tmp/shoot/editor"));
        assert_eq!(
            default_export_path(Some(&src), "jpeg", "image"),
            PathBuf::from("/tmp/shoot/editor/ảnh 01.jpg")
        );
        assert_eq!(
            default_export_path(Some(&src), "png", "image").extension().unwrap(),
            "png"
        );
        assert_eq!(
            default_export_path(None, "jpeg", "collage").file_name().unwrap(),
            "collage.jpg"
        );
    }

    #[test]
    fn save_image_jpeg_and_png() {
        // Giống test_save_image_jpeg_and_png.
        let dir = tempfile::tempdir().unwrap();
        let mut img = checker(80, 120);
        for y in 0..10 {
            for x in 0..10 {
                let mut p = img.px(y, x);
                p[3] = 0;
                img.set_px(y, x, p);
            }
        }
        let png = dir.path().join("editor").join("x.png");
        save_image(&png, &img, "png", 92).unwrap();
        let back = load_rgba(&png).unwrap();
        assert_eq!(back, img, "PNG phải quay vòng không mất mát, kể cả alpha");

        let hi = dir.path().join("editor").join("hi.jpg");
        let lo = dir.path().join("editor").join("lo.jpg");
        save_image(&hi, &img, "jpeg", 95).unwrap();
        save_image(&lo, &img, "jpeg", 30).unwrap();
        let (hs, ls) = (
            std::fs::metadata(&hi).unwrap().len(),
            std::fs::metadata(&lo).unwrap().len(),
        );
        assert!(hs > ls, "q95 ({hs} B) phải lớn hơn q30 ({ls} B)");
        let flat = load_rgba(&hi).unwrap();
        let p = flat.px(2, 2);
        assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "góc trong suốt phải thành trắng");
        assert!(p[3] == 255, "JPEG không có alpha");

        let left: Vec<_> = std::fs::read_dir(dir.path().join("editor"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(left.is_empty(), "không được để lại file .tmp");
    }

    #[test]
    fn save_image_unicode_path() {
        // Giống test_save_image_unicode_path.
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("thư mục").join("ảnh.jpg");
        let img = RgbaImage::filled(10, 10, [10, 20, 30, 255]);
        assert_eq!(save_image(&out, &img, "jpeg", 92).unwrap(), out);
        assert!(out.exists());
    }

    #[test]
    fn load_rgba_keeps_transparency() {
        // Giống test_load_rgba_keeps_transparency: PNG có alpha giữ nguyên alpha.
        let dir = tempfile::tempdir().unwrap();
        let mut img = checker(40, 50);
        for y in 0..40 {
            let mut p = img.px(y, 0);
            p[3] = 0;
            img.set_px(y, 0, p);
        }
        let path = dir.path().join("a.png");
        save_image(&path, &img, "png", 92).unwrap();
        let back = load_rgba(&path).unwrap();
        assert_eq!((back.h, back.w), (40, 50));
        assert_eq!(back.px(0, 0)[3], 0);
        assert_eq!(back.px(0, 49)[3], 255);
    }

    #[test]
    fn flatten_on_white() {
        let mut img = RgbaImage::filled(2, 2, [0, 0, 0, 0]);
        img.set_px(0, 0, [0, 0, 0, 255]);
        let rgb = flatten_on(&img, FLATTEN_COLOR);
        assert_eq!(&rgb.data[0..3], &[0, 0, 0]); // đục: giữ màu
        assert_eq!(&rgb.data[3..6], &[255, 255, 255]); // trong suốt: nền trắng
    }
}
