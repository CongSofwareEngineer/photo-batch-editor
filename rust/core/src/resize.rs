//! Image › Image Size: resample pixel (phần còn lại của `core/image_size.py`).
//! Chạy trên CPU, trên ảnh float32 `(H, W, 3)` trong `[0, 1]`.
//!
//! Lưu ý khớp OpenCV: test Python của resample chỉ kiểm tra shape/finite/khoảng giá trị
//! (`test_every_resample_runs`), không so pixel từng bit; bản Rust cài các bộ lọc tương ứng
//! (bilinear/area khớp OpenCV; cubic a=-0.75, lanczos a=4 theo đúng công thức OpenCV).

use crate::adjustments::ImageF32;
use crate::backend::{self, Plane};
use crate::image_size::target_size;
use crate::settings::ImageSizeSettings;

fn to_planes(img: &ImageF32) -> [Plane; 3] {
    let n = img.h * img.w;
    let mut planes = [Plane::new(img.h, img.w), Plane::new(img.h, img.w), Plane::new(img.h, img.w)];
    for i in 0..n {
        for c in 0..3 {
            planes[c].data[i] = img.data[i * 3 + c];
        }
    }
    planes
}

fn from_planes(planes: &[Plane; 3]) -> ImageF32 {
    let (h, w) = (planes[0].h, planes[0].w);
    let mut data = vec![0.0f32; h * w * 3];
    for i in 0..h * w {
        for c in 0..3 {
            data[i * 3 + c] = planes[c].data[i];
        }
    }
    ImageF32 { h, w, data }
}

#[inline]
fn reflect(mut i: isize, n: isize) -> usize {
    if n == 1 {
        return 0;
    }
    loop {
        if i < 0 {
            i = -i - 1;
        } else if i >= n {
            i = 2 * n - i - 1;
        } else {
            return i as usize;
        }
    }
}

fn cubic_kernel(x: f32) -> f32 {
    // OpenCV dùng a = -0.75.
    let a = -0.75f32;
    let x = x.abs();
    if x < 1.0 {
        ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
    } else if x < 2.0 {
        (((x - 5.0) * x + 8.0) * x - 4.0) * a
    } else {
        0.0
    }
}

fn lanczos4_kernel(x: f32) -> f32 {
    let x = x.abs();
    if x < 1e-6 {
        return 1.0;
    }
    if x >= 4.0 {
        return 0.0;
    }
    let px = std::f32::consts::PI * x;
    4.0 * (px.sin() * (px / 4.0).sin()) / (px * px)
}

/// Resample tách rời với kernel + support (số tap mỗi bên). Viền BORDER_REFLECT.
fn separable_resize(
    plane: &Plane,
    out_h: usize,
    out_w: usize,
    kernel: fn(f32) -> f32,
    support: f32,
) -> Plane {
    let resize_1d = |src: &[f32], n_in: usize, n_out: usize, stride: usize, out: &mut [f32], out_stride: usize, lines: usize| {
        let scale = n_in as f32 / n_out as f32;
        // scale kernel khi thu nhỏ (anti-alias) như OpenCV khi downsample bằng area;
        // với cubic/lanczos OpenCV KHÔNG giãn kernel, nên giữ support cố định.
        for o in 0..n_out {
            let center = (o as f32 + 0.5) * scale - 0.5;
            let base = center.floor() as isize;
            let s = support.ceil() as isize;
            let mut taps: Vec<(usize, f32)> = vec![];
            let mut wsum = 0.0f32;
            for t in (base - s + 1)..=(base + s) {
                let w = kernel(center - t as f32);
                if w != 0.0 {
                    let idx = reflect(t, n_in as isize);
                    taps.push((idx, w));
                    wsum += w;
                }
            }
            for line in 0..lines {
                let mut acc = 0.0f32;
                for &(idx, w) in &taps {
                    acc += src[line * stride + idx] * w;
                }
                out[line * out_stride + o] = if wsum != 0.0 { acc / wsum } else { acc };
            }
        }
    };

    let (h, w) = (plane.h, plane.w);
    // Ngang: (h, w) -> (h, out_w)
    let mut tmp = vec![0.0f32; h * out_w];
    resize_1d(&plane.data, w, out_w, w, &mut tmp, out_w, h);
    // Dọc: xử lý theo cột — chuyển thành thao tác trên hàng bằng cách coi mỗi cột là 1 "line".
    let mut out = Plane::new(out_h, out_w);
    // transpose tmp -> cột liền nhau
    let mut tmp_t = vec![0.0f32; out_w * h];
    for y in 0..h {
        for x in 0..out_w {
            tmp_t[x * h + y] = tmp[y * out_w + x];
        }
    }
    let mut out_t = vec![0.0f32; out_w * out_h];
    resize_1d(&tmp_t, h, out_h, h, &mut out_t, out_h, out_w);
    for x in 0..out_w {
        for y in 0..out_h {
            out.data[y * out_w + x] = out_t[x * out_h + y];
        }
    }
    out
}

fn resize_nearest(plane: &Plane, out_h: usize, out_w: usize) -> Plane {
    let (h, w) = (plane.h, plane.w);
    let mut out = Plane::new(out_h, out_w);
    for oy in 0..out_h {
        let sy = ((oy as f32 * h as f32 / out_h as f32).floor() as usize).min(h - 1);
        for ox in 0..out_w {
            let sx = ((ox as f32 * w as f32 / out_w as f32).floor() as usize).min(w - 1);
            out.data[oy * out_w + ox] = plane.data[sy * w + sx];
        }
    }
    out
}

/// Area: thu nhỏ = trung bình vùng; phóng to = bilinear (như OpenCV INTER_AREA).
fn resize_area(plane: &Plane, out_h: usize, out_w: usize) -> Plane {
    let (h, w) = (plane.h, plane.w);
    if out_h > h || out_w > w {
        return backend::resize_linear(plane, out_h, out_w);
    }
    let sy = h as f32 / out_h as f32;
    let sx = w as f32 / out_w as f32;
    let mut out = Plane::new(out_h, out_w);
    for oy in 0..out_h {
        let y0 = oy as f32 * sy;
        let y1 = (oy as f32 + 1.0) * sy;
        for ox in 0..out_w {
            let x0 = ox as f32 * sx;
            let x1 = (ox as f32 + 1.0) * sx;
            let mut acc = 0.0f32;
            let mut wsum = 0.0f32;
            let yi0 = y0.floor() as usize;
            let yi1 = (y1.ceil() as usize).min(h);
            let xi0 = x0.floor() as usize;
            let xi1 = (x1.ceil() as usize).min(w);
            for yy in yi0..yi1 {
                let wy = ((yy as f32 + 1.0).min(y1) - (yy as f32).max(y0)).max(0.0);
                for xx in xi0..xi1 {
                    let wx = ((xx as f32 + 1.0).min(x1) - (xx as f32).max(x0)).max(0.0);
                    let wgt = wy * wx;
                    acc += plane.data[yy * w + xx] * wgt;
                    wsum += wgt;
                }
            }
            out.data[oy * out_w + ox] = if wsum > 0.0 { acc / wsum } else { 0.0 };
        }
    }
    out
}

fn resize_plane(plane: &Plane, out_h: usize, out_w: usize, method: &str, enlarge: bool) -> Plane {
    match method {
        "automatic" => {
            if enlarge {
                separable_resize(plane, out_h, out_w, lanczos4_kernel, 4.0)
            } else {
                resize_area(plane, out_h, out_w)
            }
        }
        "preserve_details" => separable_resize(plane, out_h, out_w, lanczos4_kernel, 4.0),
        "bicubic_sharper" => resize_area(plane, out_h, out_w),
        "bicubic_smoother" | "bicubic" => separable_resize(plane, out_h, out_w, cubic_kernel, 2.0),
        "bilinear" => backend::resize_linear(plane, out_h, out_w),
        "nearest_neighbor" => resize_nearest(plane, out_h, out_w),
        _ => resize_area(plane, out_h, out_w),
    }
}

fn unsharp(planes: &mut [Plane; 3], amount: f32, sigma: f64) {
    for plane in planes.iter_mut() {
        let blur = backend::gaussian_blur(plane, sigma);
        for i in 0..plane.data.len() {
            plane.data[i] += amount * (plane.data[i] - blur.data[i]);
        }
    }
}

/// Resize ảnh float32 `(H, W, 3)` trong `[0,1]`; trả về `(image, warnings)`.
pub fn resize_image(img: &ImageF32, s: &ImageSizeSettings) -> (ImageF32, Vec<String>) {
    let (tw, th, warnings) = target_size(img.w as i64, img.h as i64, s);
    let (tw, th) = (tw as usize, th as usize);
    if (tw, th) == (img.w, img.h) {
        return (img.clone(), warnings);
    }
    let enlarge = tw * th > img.w * img.h;
    let mut planes = to_planes(img);
    for plane in planes.iter_mut() {
        *plane = resize_plane(plane, th, tw, &s.resample, enlarge);
    }
    match s.resample.as_str() {
        "preserve_details" => unsharp(&mut planes, 0.3, 1.0),
        "bicubic_sharper" => unsharp(&mut planes, 0.25, 0.6),
        _ => {}
    }
    for plane in planes.iter_mut() {
        for v in &mut plane.data {
            *v = v.clamp(0.0, 1.0);
        }
    }
    (from_planes(&planes), warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::RESAMPLE_OPTIONS;

    fn make_img(h: usize, w: usize) -> ImageF32 {
        let mut data = vec![0.0f32; h * w * 3];
        for i in 0..h * w {
            data[i * 3] = (i % 7) as f32 / 6.0;
            data[i * 3 + 1] = (i % 13) as f32 / 12.0;
            data[i * 3 + 2] = (i % 5) as f32 / 4.0;
        }
        ImageF32 { h, w, data }
    }

    #[test]
    fn every_resample_runs() {
        // Giống test_every_resample_runs: mọi phương pháp cho đúng shape, hữu hạn, trong [0,1].
        let img = make_img(60, 80);
        for (method, _) in RESAMPLE_OPTIONS {
            for value in [50.0, 180.0] {
                let s = ImageSizeSettings::new("percent", value, method, false);
                let (out, _) = resize_image(&img, &s);
                let expected = ((80.0 * value / 100.0).round() as usize, (60.0 * value / 100.0).round() as usize);
                assert_eq!((out.w, out.h), expected, "{method} {value}");
                assert!(out.data.iter().all(|v| v.is_finite()));
                assert!(out.data.iter().all(|&v| (0.0..=1.0).contains(&v)), "{method} {value} ngoài [0,1]");
            }
        }
    }

    #[test]
    fn off_keeps_size() {
        let img = make_img(50, 70);
        let (out, warn) = resize_image(&img, &ImageSizeSettings::default());
        assert_eq!((out.h, out.w), (50, 70));
        assert!(warn.is_empty());
        assert_eq!(out.data, img.data);
    }
}
