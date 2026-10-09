//! Đọc ảnh, xoay theo EXIF, ghi JPEG. Port của `core/io_utils.py`.
//!
//! Đường dẫn có thể chứa ký tự Unicode (tiếng Việt). Dùng crate `image` để giải mã
//! (PNG/JPEG/BMP/TIFF/WebP, giữ 8/16-bit), `kamadak-exif` để đọc Orientation, `jpeg-encoder`
//! để ghi JPEG chất lượng 100, lấy mẫu 4:4:4, kèm EXIF (Orientation về 1) và ICC.
//!
//! GĐ3b còn lại: giải mã CMYK JPEG, lấy ICC từ chunk iCCP của PNG.

use std::io::Cursor;
use std::path::Path;

use image::DynamicImage;

use crate::adjustments::{ImageF32, ImageU16, ImageU8};

pub const EXIF_NOT_KEPT: &str = "EXIF not kept";
const TAG_ORIENTATION: u16 = 0x0112;
const TAG_EXIF_IFD: u16 = 0x8769;
const TAG_PIXEL_X: u16 = 0xA002;
const TAG_PIXEL_Y: u16 = 0xA003;

#[derive(Debug)]
pub struct ImageReadError(pub String);

impl std::fmt::Display for ImageReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for ImageReadError {}

/// Pixel đã giải mã, giữ nguyên độ sâu bit như bản Python (để LUT trong pipeline khớp).
pub enum Pixels {
    U8(ImageU8),
    U16(ImageU16),
    F32(ImageF32),
}

impl Pixels {
    pub fn dims(&self) -> (usize, usize) {
        match self {
            Pixels::U8(i) => (i.h, i.w),
            Pixels::U16(i) => (i.h, i.w),
            Pixels::F32(i) => (i.h, i.w),
        }
    }
}

pub struct LoadedImage {
    pub pixels: Pixels,
    pub exif: Option<Vec<u8>>, // phần TIFF sau "Exif\0\0"
    pub icc: Option<Vec<u8>>,
}

impl LoadedImage {
    pub fn width(&self) -> usize {
        self.pixels.dims().1
    }
    pub fn height(&self) -> usize {
        self.pixels.dims().0
    }
}

// --- Xoay theo EXIF (khớp numpy của apply_orientation) ------------------------------------

/// Trả về `(out_h, out_w, map)` với `map(oi, oj) -> (iy, ix)`.
#[allow(clippy::type_complexity)]
fn orientation_map(
    h: usize,
    w: usize,
    orientation: u16,
) -> (usize, usize, Box<dyn Fn(usize, usize) -> (usize, usize)>) {
    match orientation {
        2 => (h, w, Box::new(move |i, j| (i, w - 1 - j))),
        3 => (h, w, Box::new(move |i, j| (h - 1 - i, w - 1 - j))),
        4 => (h, w, Box::new(move |i, j| (h - 1 - i, j))),
        5 => (w, h, Box::new(move |i, j| (j, i))),
        6 => (w, h, Box::new(move |i, j| (h - 1 - j, i))),
        7 => (w, h, Box::new(move |i, j| (h - 1 - j, w - 1 - i))),
        8 => (w, h, Box::new(move |i, j| (j, w - 1 - i))),
        _ => (h, w, Box::new(move |i, j| (i, j))),
    }
}

pub(crate) fn apply_orientation_vec<T: Copy + Default>(
    data: &[T],
    h: usize,
    w: usize,
    ch: usize,
    orientation: u16,
) -> (usize, usize, Vec<T>) {
    if orientation <= 1 || orientation > 8 {
        return (h, w, data.to_vec());
    }
    let (oh, ow, map) = orientation_map(h, w, orientation);
    let mut out = vec![T::default(); oh * ow * ch];
    for oi in 0..oh {
        for oj in 0..ow {
            let (iy, ix) = map(oi, oj);
            let si = (iy * w + ix) * ch;
            let di = (oi * ow + oj) * ch;
            out[di..di + ch].copy_from_slice(&data[si..si + ch]);
        }
    }
    (oh, ow, out)
}

// --- Composite alpha trên nền trắng -------------------------------------------------------

fn composite_u8(rgb: &[u8], a: u8) -> [u8; 3] {
    let af = a as f32 / 255.0;
    let mut out = [0u8; 3];
    for c in 0..3 {
        let v = (rgb[c] as f32 / 255.0) * af + (1.0 - af);
        out[c] = (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8;
    }
    out
}

fn composite_u16(rgb: &[u16], a: u16) -> [u16; 3] {
    let af = a as f32 / 65535.0;
    let mut out = [0u16; 3];
    for c in 0..3 {
        let v = (rgb[c] as f32 / 65535.0) * af + (1.0 - af);
        out[c] = (v.clamp(0.0, 1.0) * 65535.0).round_ties_even() as u16;
    }
    out
}

fn to_rgb(img: DynamicImage) -> Pixels {
    let (w, h) = (img.width() as usize, img.height() as usize);
    match img {
        DynamicImage::ImageLuma8(g) => {
            let mut data = vec![0u8; w * h * 3];
            for (i, p) in g.pixels().enumerate() {
                data[i * 3..i * 3 + 3].copy_from_slice(&[p.0[0], p.0[0], p.0[0]]);
            }
            Pixels::U8(ImageU8 { h, w, data })
        }
        DynamicImage::ImageLumaA8(ga) => {
            let mut data = vec![0u8; w * h * 3];
            for (i, p) in ga.pixels().enumerate() {
                let c = composite_u8(&[p.0[0], p.0[0], p.0[0]], p.0[1]);
                data[i * 3..i * 3 + 3].copy_from_slice(&c);
            }
            Pixels::U8(ImageU8 { h, w, data })
        }
        DynamicImage::ImageRgb8(rgb) => Pixels::U8(ImageU8 {
            h,
            w,
            data: rgb.into_raw(),
        }),
        DynamicImage::ImageRgba8(rgba) => {
            let mut data = vec![0u8; w * h * 3];
            for (i, p) in rgba.pixels().enumerate() {
                let c = composite_u8(&[p.0[0], p.0[1], p.0[2]], p.0[3]);
                data[i * 3..i * 3 + 3].copy_from_slice(&c);
            }
            Pixels::U8(ImageU8 { h, w, data })
        }
        DynamicImage::ImageLuma16(g) => {
            let mut data = vec![0u16; w * h * 3];
            for (i, p) in g.pixels().enumerate() {
                data[i * 3..i * 3 + 3].copy_from_slice(&[p.0[0], p.0[0], p.0[0]]);
            }
            Pixels::U16(ImageU16 { h, w, data })
        }
        DynamicImage::ImageLumaA16(ga) => {
            let mut data = vec![0u16; w * h * 3];
            for (i, p) in ga.pixels().enumerate() {
                let c = composite_u16(&[p.0[0], p.0[0], p.0[0]], p.0[1]);
                data[i * 3..i * 3 + 3].copy_from_slice(&c);
            }
            Pixels::U16(ImageU16 { h, w, data })
        }
        DynamicImage::ImageRgb16(rgb) => Pixels::U16(ImageU16 {
            h,
            w,
            data: rgb.into_raw(),
        }),
        DynamicImage::ImageRgba16(rgba) => {
            let mut data = vec![0u16; w * h * 3];
            for (i, p) in rgba.pixels().enumerate() {
                let c = composite_u16(&[p.0[0], p.0[1], p.0[2]], p.0[3]);
                data[i * 3..i * 3 + 3].copy_from_slice(&c);
            }
            Pixels::U16(ImageU16 { h, w, data })
        }
        other => {
            // float và các dạng khác -> f32 RGB trong [0,1]
            let rgb = other.to_rgb32f();
            Pixels::F32(ImageF32 {
                h,
                w,
                data: rgb.into_raw(),
            })
        }
    }
}

pub(crate) fn read_orientation(bytes: &[u8]) -> u16 {
    let exif_reader = exif::Reader::new();
    if let Ok(exif) = exif_reader.read_from_container(&mut Cursor::new(bytes)) {
        if let Some(field) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) {
            if let Some(v) = field.value.get_uint(0) {
                return v as u16;
            }
        }
    }
    1
}

/// Trích phần TIFF của segment APP1 "Exif" trong JPEG (nếu có).
fn extract_jpeg_exif(bytes: &[u8]) -> Option<Vec<u8>> {
    for (off, len) in jpeg_app_segments(bytes, 0xE1) {
        let seg = &bytes[off..off + len];
        if seg.len() > 6 && &seg[..6] == b"Exif\0\0" {
            return Some(seg[6..].to_vec());
        }
    }
    None
}

/// Nối các chunk ICC (APP2 "ICC_PROFILE") của JPEG.
fn extract_jpeg_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut chunks: Vec<(u8, Vec<u8>)> = vec![];
    for (off, len) in jpeg_app_segments(bytes, 0xE2) {
        let seg = &bytes[off..off + len];
        if seg.len() > 14 && &seg[..12] == b"ICC_PROFILE\0" {
            let seq = seg[12];
            chunks.push((seq, seg[14..].to_vec()));
        }
    }
    if chunks.is_empty() {
        return None;
    }
    chunks.sort_by_key(|(seq, _)| *seq);
    Some(chunks.into_iter().flat_map(|(_, d)| d).collect())
}

/// `(offset, length)` phần nội dung (sau 2 byte độ dài) của mọi marker `0xFF<marker>`.
fn jpeg_app_segments(bytes: &[u8], marker: u8) -> Vec<(usize, usize)> {
    let mut out = vec![];
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return out;
    }
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            break;
        }
        let m = bytes[i + 1];
        if m == 0xD9 || m == 0xDA {
            break; // EOI / SOS
        }
        let len = ((bytes[i + 2] as usize) << 8) | bytes[i + 3] as usize;
        if len < 2 || i + 2 + len > bytes.len() {
            break;
        }
        if m == marker {
            out.push((i + 4, len - 2));
        }
        i += 2 + len;
    }
    out
}

pub fn read_image(path: &Path) -> Result<LoadedImage, ImageReadError> {
    let bytes = std::fs::read(path).map_err(|e| ImageReadError(format!("Cannot read file: {e}")))?;
    if bytes.is_empty() {
        return Err(ImageReadError("Empty file".into()));
    }
    let decoded = image::load_from_memory(&bytes)
        .map_err(|e| ImageReadError(format!("Corrupt file: {e}")))?;
    let pixels = to_rgb(decoded);
    let orientation = read_orientation(&bytes);
    let exif = extract_jpeg_exif(&bytes);
    let icc = extract_jpeg_icc(&bytes);
    let pixels = apply_orientation_pixels(pixels, orientation);
    Ok(LoadedImage { pixels, exif, icc })
}

fn apply_orientation_pixels(pixels: Pixels, orientation: u16) -> Pixels {
    match pixels {
        Pixels::U8(img) => {
            let (h, w, data) = apply_orientation_vec(&img.data, img.h, img.w, 3, orientation);
            Pixels::U8(ImageU8 { h, w, data })
        }
        Pixels::U16(img) => {
            let (h, w, data) = apply_orientation_vec(&img.data, img.h, img.w, 3, orientation);
            Pixels::U16(ImageU16 { h, w, data })
        }
        Pixels::F32(img) => {
            let (h, w, data) = apply_orientation_vec(&img.data, img.h, img.w, 3, orientation);
            Pixels::F32(ImageF32 { h, w, data })
        }
    }
}

// --- Ghi JPEG -----------------------------------------------------------------------------

/// Patch EXIF: đặt Orientation = 1 và cập nhật PixelX/Y (nếu có). Trả về None nếu không hợp lệ.
fn patch_exif(tiff: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut t = tiff.to_vec();
    if t.len() < 8 {
        return None;
    }
    let le = match &t[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let rd16 = |b: &[u8], o: usize| -> u16 {
        if le {
            u16::from_le_bytes([b[o], b[o + 1]])
        } else {
            u16::from_be_bytes([b[o], b[o + 1]])
        }
    };
    let rd32 = |b: &[u8], o: usize| -> u32 {
        if le {
            u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
        } else {
            u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
        }
    };
    let wr_val = |b: &mut [u8], o: usize, ftype: u16, val: u32| {
        // ghi vào trường value 4 byte theo kiểu SHORT/LONG
        if ftype == 3 {
            let v = val as u16;
            let bb = if le { v.to_le_bytes() } else { v.to_be_bytes() };
            b[o..o + 2].copy_from_slice(&bb);
        } else {
            let bb = if le { val.to_le_bytes() } else { val.to_be_bytes() };
            b[o..o + 4].copy_from_slice(&bb);
        }
    };
    let ifd0 = rd32(&t, 4) as usize;
    if ifd0 + 2 > t.len() {
        return None;
    }
    let mut exif_ifd: Option<usize> = None;
    let process_ifd = |t: &mut Vec<u8>, ifd: usize, exif_ifd: &mut Option<usize>| -> bool {
        if ifd + 2 > t.len() {
            return false;
        }
        let count = rd16(t, ifd) as usize;
        for e in 0..count {
            let eo = ifd + 2 + e * 12;
            if eo + 12 > t.len() {
                return false;
            }
            let tag = rd16(t, eo);
            let ftype = rd16(t, eo + 2);
            match tag {
                TAG_ORIENTATION => wr_val(t, eo + 8, ftype, 1),
                TAG_EXIF_IFD => *exif_ifd = Some(rd32(t, eo + 8) as usize),
                TAG_PIXEL_X => wr_val(t, eo + 8, ftype, width),
                TAG_PIXEL_Y => wr_val(t, eo + 8, ftype, height),
                _ => {}
            }
        }
        true
    };
    if !process_ifd(&mut t, ifd0, &mut exif_ifd) {
        return None;
    }
    if let Some(eifd) = exif_ifd {
        process_ifd(&mut t, eifd, &mut None);
    }
    Some(t)
}

/// Ghi RGB uint8 thành JPEG chất lượng 100, 4:4:4, kèm EXIF (Orientation=1) + ICC. Trả về cảnh báo.
pub fn write_jpeg(
    out_path: &Path,
    pixels: &ImageU8,
    exif: Option<&[u8]>,
    icc: Option<&[u8]>,
) -> Result<Vec<String>, ImageReadError> {
    let mut warnings: Vec<String> = vec![];
    let mut buf: Vec<u8> = vec![];
    {
        use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
        let mut encoder = Encoder::new(&mut buf, 100);
        encoder.set_sampling_factor(SamplingFactor::R_4_4_4);
        if let Some(icc) = icc {
            let _ = encoder.add_icc_profile(icc);
        }
        if let Some(tiff) = exif {
            match patch_exif(tiff, pixels.w as u32, pixels.h as u32) {
                // EXIF phải lọt trong một segment APP1 (giới hạn ~64 KB).
                Some(patched) if patched.len() + 6 <= 65533 => {
                    let mut seg = b"Exif\0\0".to_vec();
                    seg.extend_from_slice(&patched);
                    encoder.add_app_segment(1, &seg).ok();
                }
                _ => warnings.push(EXIF_NOT_KEPT.to_string()),
            }
        }
        encoder
            .encode(&pixels.data, pixels.w as u16, pixels.h as u16, ColorType::Rgb)
            .map_err(|e| ImageReadError(format!("JPEG encode failed: {e}")))?;
    }
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ImageReadError(e.to_string()))?;
    }
    let tmp = out_path.with_file_name(format!(
        "{}.tmp",
        out_path.file_name().and_then(|n| n.to_str()).unwrap_or("out.jpg")
    ));
    std::fs::write(&tmp, &buf).map_err(|e| ImageReadError(e.to_string()))?;
    std::fs::rename(&tmp, out_path).map_err(|e| ImageReadError(e.to_string()))?;
    Ok(warnings)
}
