//! Enhance › Super Resolution với Real-ESRGAN + ONNX Runtime. Port của `core/enhance.py`.
//!
//! Gắn sau feature `sr` (chỉ khi đó mới kéo `ort`/ONNX runtime). Cài đặt trait
//! [`crate::pipeline::Upscaler`] để pipeline dùng được.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ndarray::Array4;
use ort::session::Session;
use ort::value::Tensor;

use crate::adjustments::ImageF32;
use crate::paths::resource_path;
use crate::pipeline::Upscaler;

pub const MODEL_FILENAME: &str = "realesr-general-x4v3.onnx";
pub const NATIVE_SCALE: usize = 4;
pub const OVERLAP: usize = 16;
pub const TILE_CPU: usize = 256;

pub fn default_model_path() -> PathBuf {
    resource_path(["models", MODEL_FILENAME])
}

/// Session ONNX tái sử dụng, phóng to ×2/×4 theo ô (tiled). Khớp `core.enhance.SuperResolver`.
pub struct SuperResolver {
    session: Mutex<Session>,
    input_name: String,
    tile_size: usize,
}

impl SuperResolver {
    pub fn new(model_path: Option<&Path>, tile_size: Option<usize>) -> Result<Self, String> {
        let path = model_path.map(|p| p.to_path_buf()).unwrap_or_else(default_model_path);
        if !path.is_file() {
            return Err(format!("Super Resolution model not found: {}", path.display()));
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("Could not load Super Resolution model: {e}"))?;
        let session = Session::builder()
            .and_then(|b| b.commit_from_memory(&bytes))
            .map_err(|e| format!("Could not load Super Resolution model: {e}"))?;
        let input_name = session.inputs[0].name.clone();
        Ok(SuperResolver {
            session: Mutex::new(session),
            input_name,
            tile_size: tile_size.unwrap_or(TILE_CPU),
        })
    }

    fn run_tile(&self, tile: &[f32], th: usize, tw: usize) -> Result<(usize, usize, Vec<f32>), String> {
        // HWC -> NCHW
        let input = Array4::<f32>::from_shape_fn((1, 3, th, tw), |(_, c, y, x)| tile[(y * tw + x) * 3 + c]);
        let tensor = Tensor::from_array(input).map_err(|e| e.to_string())?;
        let mut session = self.session.lock().unwrap();
        let outputs = session
            .run(ort::inputs![self.input_name.as_str() => tensor])
            .map_err(|e| e.to_string())?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let oh = shape[2] as usize;
        let ow = shape[3] as usize;
        // NCHW -> HWC
        let mut hwc = vec![0.0f32; oh * ow * 3];
        for c in 0..3 {
            let base = c * oh * ow;
            for y in 0..oh {
                for x in 0..ow {
                    hwc[(y * ow + x) * 3 + c] = data[base + y * ow + x];
                }
            }
        }
        Ok((oh, ow, hwc))
    }
}

/// np.pad cho HWC: mode "reflect" (reflect_101, không lặp biên) hoặc "edge" (lặp biên).
fn pad_hwc(img: &ImageF32, pad: usize, reflect: bool) -> ImageF32 {
    let (h, w) = (img.h, img.w);
    let (nh, nw) = (h + 2 * pad, w + 2 * pad);
    let map = |i: isize, n: usize| -> usize {
        let n = n as isize;
        if reflect {
            // reflect_101: 0 1 2 .. ; mép không lặp
            let mut k = i;
            loop {
                if k < 0 {
                    k = -k;
                } else if k >= n {
                    k = 2 * (n - 1) - k;
                } else {
                    return k as usize;
                }
            }
        } else {
            i.clamp(0, n - 1) as usize
        }
    };
    let mut data = vec![0.0f32; nh * nw * 3];
    for oy in 0..nh {
        let iy = map(oy as isize - pad as isize, h);
        for ox in 0..nw {
            let ix = map(ox as isize - pad as isize, w);
            let si = (iy * w + ix) * 3;
            let di = (oy * nw + ox) * 3;
            data[di..di + 3].copy_from_slice(&img.data[si..si + 3]);
        }
    }
    ImageF32 { h: nh, w: nw, data }
}

/// Trung bình khối nguyên `ratio×ratio` (= cv2.INTER_AREA hệ số nguyên), cho HWC.
fn downscale_area_int(src: &[f32], h: usize, w: usize, ratio: usize) -> (usize, usize, Vec<f32>) {
    let (oh, ow) = (h / ratio, w / ratio);
    let mut out = vec![0.0f32; oh * ow * 3];
    let area = (ratio * ratio) as f32;
    for by in 0..oh {
        for bx in 0..ow {
            let mut acc = [0.0f32; 3];
            for dy in 0..ratio {
                for dx in 0..ratio {
                    let si = ((by * ratio + dy) * w + (bx * ratio + dx)) * 3;
                    for c in 0..3 {
                        acc[c] += src[si + c];
                    }
                }
            }
            let di = (by * ow + bx) * 3;
            for c in 0..3 {
                out[di + c] = acc[c] / area;
            }
        }
    }
    (oh, ow, out)
}

impl Upscaler for SuperResolver {
    fn upscale(&self, img: &ImageF32, factor: i64) -> Result<ImageF32, String> {
        let factor = factor as usize;
        if factor != 2 && factor != 4 {
            return Ok(img.clone());
        }
        let (h, w) = (img.h, img.w);
        let pad = OVERLAP;
        let reflect = h.min(w) > pad;
        let padded = pad_hwc(img, pad, reflect);
        let s = NATIVE_SCALE;
        let (ow, oh) = (w * factor, h * factor);
        let mut out = vec![0.0f32; oh * ow * 3];
        let t = self.tile_size;
        let mut y0 = 0;
        while y0 < h {
            let y1 = (y0 + t).min(h);
            let mut x0 = 0;
            while x0 < w {
                let x1 = (x0 + t).min(w);
                let (th, tw) = (y1 - y0 + 2 * pad, x1 - x0 + 2 * pad);
                // cắt ô từ padded
                let mut tile = vec![0.0f32; th * tw * 3];
                for ty in 0..th {
                    let src = ((y0 + ty) * padded.w + x0) * 3;
                    tile[ty * tw * 3..ty * tw * 3 + tw * 3]
                        .copy_from_slice(&padded.data[src..src + tw * 3]);
                }
                let (rh, rw, res) = self.run_tile(&tile, th, tw)?;
                // lấy phần lõi (bỏ overlap)
                let (ch, cw) = ((y1 - y0) * s, (x1 - x0) * s);
                let mut core = vec![0.0f32; ch * cw * 3];
                for cy in 0..ch {
                    let src = ((pad * s + cy) * rw + pad * s) * 3;
                    core[cy * cw * 3..cy * cw * 3 + cw * 3].copy_from_slice(&res[src..src + cw * 3]);
                }
                let _ = rh;
                // về đúng hệ số (factor 2 -> thu nhỏ /2)
                let (fh, fw, fcore) = if factor != s {
                    downscale_area_int(&core, ch, cw, s / factor)
                } else {
                    (ch, cw, core)
                };
                // đặt vào ảnh ra
                for fy in 0..fh {
                    let di = ((y0 * factor + fy) * ow + x0 * factor) * 3;
                    out[di..di + fw * 3].copy_from_slice(&fcore[fy * fw * 3..fy * fw * 3 + fw * 3]);
                }
                x0 += t;
            }
            y0 += t;
        }
        for v in &mut out {
            *v = v.clamp(0.0, 1.0);
        }
        Ok(ImageF32 { h: oh, w: ow, data: out })
    }
}
