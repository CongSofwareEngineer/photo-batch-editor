//! I/O: đọc ảnh khớp bản Python (fixture) + ghi JPEG đúng chất lượng/EXIF. Xem rust-port.md.

use std::io::Cursor;
use std::path::PathBuf;

use pbe_core::adjustments::ImageU8;
use pbe_core::io_utils::{read_image, write_jpeg, Pixels, EXIF_NOT_KEPT};
use serde_json::Value;

fn io_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("io")
}

fn manifest() -> Value {
    let text = std::fs::read_to_string(io_dir().join("io_manifest.json"))
        .expect("io_manifest.json — chạy: venv/bin/python tools/gen_io_fixtures.py");
    serde_json::from_str(&text).unwrap()
}

fn shape(m: &Value, key: &str) -> Vec<usize> {
    m["arrays"][key]["shape"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect()
}

fn expected_u8(key: &str) -> Vec<u8> {
    std::fs::read(io_dir().join(format!("{key}.bin"))).unwrap()
}

fn expected_u16(key: &str) -> Vec<u16> {
    expected_u8(key).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

fn mean_abs_u8(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(&x, &y)| (x as i32 - y as i32).abs() as f64).sum::<f64>() / a.len() as f64
}

#[test]
fn reads_lossless_exactly() {
    let m = manifest();
    for key in ["png_rgb", "png_alpha", "gray"] {
        let file = m["arrays"][key]["file"].as_str().unwrap();
        let loaded = read_image(&io_dir().join(file)).unwrap();
        let sh = shape(&m, key);
        match loaded.pixels {
            Pixels::U8(img) => {
                assert_eq!(vec![img.h, img.w, 3], sh, "{key} shape");
                assert_eq!(img.data, expected_u8(key), "{key} pixels");
            }
            _ => panic!("{key} phải là u8"),
        }
    }
}

#[test]
fn reads_tiff16_exactly() {
    let m = manifest();
    let loaded = read_image(&io_dir().join("tiff16.tif")).unwrap();
    let sh = shape(&m, "tiff16");
    match loaded.pixels {
        Pixels::U16(img) => {
            assert_eq!(vec![img.h, img.w, 3], sh);
            assert_eq!(img.data, expected_u16("tiff16"));
            assert_eq!(&img.data[0..3], &[65535, 32768, 1000]);
        }
        _ => panic!("tiff16 phải là u16"),
    }
}

#[test]
fn reads_orientation_matches_pillow_within_tolerance() {
    let m = manifest();
    for o in 1..=8 {
        let key = format!("orient_{o}");
        let file = m["arrays"][&key]["file"].as_str().unwrap();
        let loaded = read_image(&io_dir().join(file)).unwrap();
        let sh = shape(&m, &key);
        match loaded.pixels {
            Pixels::U8(img) => {
                assert_eq!(vec![img.h, img.w, 3], sh, "{key} shape (xoay sai?)");
                // JPEG decoder khác libjpeg vài mức; xoay đúng thì sai số trung bình nhỏ.
                let mean = mean_abs_u8(&img.data, &expected_u8(&key));
                assert!(mean <= 3.0, "{key} mean diff {mean} > 3 (xoay sai?)");
            }
            _ => panic!("{key} phải là u8"),
        }
    }
}

// --- Ghi JPEG -----------------------------------------------------------------------------

fn jpeg_sampling_all_444(bytes: &[u8]) -> bool {
    // tìm SOF0 (0xFFC0), đọc sampling từng component
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            break;
        }
        let m = bytes[i + 1];
        let len = ((bytes[i + 2] as usize) << 8) | bytes[i + 3] as usize;
        if m == 0xC0 || m == 0xC1 {
            let ncomp = bytes[i + 4 + 5] as usize;
            let mut off = i + 4 + 6;
            for _ in 0..ncomp {
                if bytes[off + 1] != 0x11 {
                    return false; // H=1,V=1 -> 4:4:4
                }
                off += 3;
            }
            return true;
        }
        if m == 0xDA {
            break;
        }
        i += 2 + len;
    }
    false
}

fn jpeg_quant_all_ones(bytes: &[u8]) -> bool {
    let mut i = 2;
    let mut seen = false;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            break;
        }
        let m = bytes[i + 1];
        let len = ((bytes[i + 2] as usize) << 8) | bytes[i + 3] as usize;
        if m == 0xDB {
            let mut off = i + 4;
            let end = i + 2 + len;
            while off < end {
                let pq = bytes[off] >> 4; // 0 = 8-bit
                off += 1;
                let n = if pq == 0 { 64 } else { 128 };
                for _ in 0..n {
                    if pq == 0 {
                        if bytes[off] != 1 {
                            return false;
                        }
                        off += 1;
                    } else {
                        off += 2;
                    }
                }
                seen = true;
            }
        }
        if m == 0xDA {
            break;
        }
        i += 2 + len;
    }
    seen
}

#[test]
fn writes_quality_100_444() {
    let loaded = read_image(&io_dir().join("png_rgb.png")).unwrap();
    let img = match loaded.pixels {
        Pixels::U8(i) => i,
        _ => unreachable!(),
    };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("o.jpg");
    let warnings = write_jpeg(&out, &img, None, None).unwrap();
    assert!(warnings.is_empty());
    let bytes = std::fs::read(&out).unwrap();
    assert!(jpeg_sampling_all_444(&bytes), "phải 4:4:4");
    assert!(jpeg_quant_all_ones(&bytes), "q100 -> bảng lượng tử toàn 1");
    // đọc lại, kích thước giữ nguyên, pixel gần giống (JPEG lossy)
    let back = read_image(&out).unwrap();
    assert_eq!((back.height(), back.width()), (img.h, img.w));
}

#[test]
fn writes_reset_orientation_and_keeps_make() {
    let loaded = read_image(&io_dir().join("orient_6.jpg")).unwrap();
    let img = match &loaded.pixels {
        Pixels::U8(i) => i.clone(),
        _ => unreachable!(),
    };
    assert!(loaded.exif.is_some(), "nguồn phải có EXIF");
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("r.jpg");
    let warnings = write_jpeg(&out, &img, loaded.exif.as_deref(), None).unwrap();
    assert!(warnings.is_empty());
    let bytes = std::fs::read(&out).unwrap();
    let exif = exif::Reader::new().read_from_container(&mut Cursor::new(&bytes)).unwrap();
    let orient = exif
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0));
    assert_eq!(orient, Some(1), "Orientation phải reset về 1");
    let make = exif
        .get_field(exif::Tag::Make, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string());
    assert_eq!(make.as_deref(), Some("\"TestCam\""), "giữ Make");
    let px = exif
        .get_field(exif::Tag::PixelXDimension, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0));
    let py = exif
        .get_field(exif::Tag::PixelYDimension, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0));
    assert_eq!((px, py), (Some(60), Some(90)), "pixel dims cập nhật");
}

#[test]
fn oversized_exif_falls_back() {
    // EXIF hợp lệ nhưng > 64 KB -> không giữ, cảnh báo EXIF not kept.
    let mut tiff = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]; // II, offset IFD0 = 8
    tiff.extend_from_slice(&[0x00, 0x00]); // count = 0
    tiff.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // next IFD = 0
    tiff.resize(70_000, 0);
    let img = ImageU8 { h: 20, w: 20, data: vec![128; 20 * 20 * 3] };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("big.jpg");
    let warnings = write_jpeg(&out, &img, Some(&tiff), None).unwrap();
    assert_eq!(warnings, vec![EXIF_NOT_KEPT.to_string()]);
    assert!(out.exists());
}
