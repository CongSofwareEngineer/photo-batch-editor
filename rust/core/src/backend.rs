//! Nền tảng CPU: Gaussian blur, box filter, block-mean (down), bilinear (up) — khớp OpenCV.
//! Port phần CPU của `core/backend.py` (NumPy + OpenCV). GPU/CuPy không port (xem rust-port.md).
//!
//! Mọi phép khớp với OpenCV theo đúng ngữ nghĩa: kernel Gaussian do `getGaussianKernel` dựng,
//! viền `BORDER_REFLECT` (`fedcba|abcdef|fedcba`), `INTER_AREA` hệ số nguyên = trung bình khối,
//! `INTER_LINEAR` = bilinear tâm nửa-pixel (giống `_resize_linear_xp`).

/// Mặt phẳng 2D float32 (H×W), hàng liền nhau — tương ứng mảng NumPy `(H, W)`.
#[derive(Debug, Clone)]
pub struct Plane {
    pub h: usize,
    pub w: usize,
    pub data: Vec<f32>,
}

impl Plane {
    pub fn new(h: usize, w: usize) -> Self {
        Plane {
            h,
            w,
            data: vec![0.0; h * w],
        }
    }

    pub fn from_vec(h: usize, w: usize, data: Vec<f32>) -> Self {
        assert_eq!(data.len(), h * w);
        Plane { h, w, data }
    }

    #[inline]
    pub fn at(&self, y: usize, x: usize) -> f32 {
        self.data[y * self.w + x]
    }
}

pub const LARGE_SIGMA: f64 = 8.0;
pub const SMALL_SIGMA_TARGET: f64 = 4.0;

/// Bán kính kernel dùng chung với OpenCV/SciPy (`truncate=4`).
pub fn gaussian_radius(sigma: f64) -> usize {
    (4.0 * sigma + 0.5) as usize
}

fn downscale_factor(sigma: f64) -> usize {
    ((sigma / SMALL_SIGMA_TARGET).ceil() as usize).max(2)
}

/// Chỉ số phản chiếu kiểu `BORDER_REFLECT` (có lặp pixel biên).
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

/// Kernel Gaussian như `cv2.getGaussianKernel(ksize, sigma)` với sigma > 0.
fn gaussian_kernel(sigma: f64) -> Vec<f64> {
    let r = gaussian_radius(sigma);
    let ksize = 2 * r + 1;
    let scale2x = -0.5 / (sigma * sigma);
    let center = (ksize as f64 - 1.0) * 0.5;
    let mut k = vec![0.0f64; ksize];
    let mut sum = 0.0;
    for (i, slot) in k.iter_mut().enumerate() {
        let x = i as f64 - center;
        let t = (scale2x * x * x).exp();
        *slot = t;
        sum += t;
    }
    for v in &mut k {
        *v /= sum;
    }
    k
}

/// Lọc tách (separable) một mặt phẳng với kernel 1D đối xứng, viền BORDER_REFLECT.
fn sep_filter(src: &Plane, kernel: &[f64]) -> Plane {
    let (h, w) = (src.h, src.w);
    let r = (kernel.len() / 2) as isize;
    // Ngang
    let mut tmp = vec![0.0f64; h * w];
    for y in 0..h {
        let row = &src.data[y * w..y * w + w];
        for x in 0..w {
            let mut acc = 0.0;
            for (j, &kv) in kernel.iter().enumerate() {
                let xx = reflect(x as isize + j as isize - r, w as isize);
                acc += kv * row[xx] as f64;
            }
            tmp[y * w + x] = acc;
        }
    }
    // Dọc
    let mut out = Plane::new(h, w);
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (j, &kv) in kernel.iter().enumerate() {
                let yy = reflect(y as isize + j as isize - r, h as isize);
                acc += kv * tmp[yy * w + x];
            }
            out.data[y * w + x] = acc as f32;
        }
    }
    out
}

fn gaussian_native(src: &Plane, sigma: f64) -> Plane {
    sep_filter(src, &gaussian_kernel(sigma))
}

/// Blur Gaussian viền phản chiếu; sigma lớn chạy trên bản thu nhỏ (giống backend.py).
pub fn gaussian_blur(src: &Plane, sigma: f64) -> Plane {
    if sigma <= 0.0 {
        return src.clone();
    }
    if sigma <= LARGE_SIGMA {
        return gaussian_native(src, sigma);
    }
    let f = downscale_factor(sigma);
    let small = gaussian_native(&downscale(src, f), sigma / f as f64);
    upscale(&small, src.h, src.w, f)
}

/// `fn(gaussian_blur(src, sigma))` cho `fn` theo từng điểm. Sigma lớn thì fn chạy trên bản thu nhỏ.
pub fn blur_map<F>(src: &Plane, sigma: f64, f: F) -> Plane
where
    F: Fn(&Plane) -> Plane,
{
    if sigma <= LARGE_SIGMA {
        return f(&gaussian_blur(src, sigma));
    }
    let factor = downscale_factor(sigma);
    let small = gaussian_native(&downscale(src, factor), sigma / factor as f64);
    upscale(&f(&small), src.h, src.w, factor)
}

/// Box filter chuẩn hoá (mean), viền BORDER_REFLECT, cửa sổ `2r+1`.
pub fn box_filter(src: &Plane, radius: usize) -> Plane {
    let (h, w) = (src.h, src.w);
    let k = (2 * radius + 1) as f64;
    let r = radius as isize;
    // Ngang (tổng / k)
    let mut tmp = vec![0.0f64; h * w];
    for y in 0..h {
        let row = &src.data[y * w..y * w + w];
        for x in 0..w {
            let mut acc = 0.0;
            for j in -r..=r {
                let xx = reflect(x as isize + j, w as isize);
                acc += row[xx] as f64;
            }
            tmp[y * w + x] = acc / k;
        }
    }
    // Dọc
    let mut out = Plane::new(h, w);
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for j in -r..=r {
                let yy = reflect(y as isize + j, h as isize);
                acc += tmp[yy * w + x];
            }
            out.data[y * w + x] = (acc / k) as f32;
        }
    }
    out
}

/// Trung bình khối `f×f` (viền phải/dưới đệm phản chiếu tới bội của `f`). Giống `_block_mean`.
pub fn block_mean(src: &Plane, f: usize, ph: usize, pw: usize) -> Plane {
    let (h, w) = (src.h, src.w);
    let (nh, nw) = (h + ph, w + pw);
    let (oh, ow) = (nh / f, nw / f);
    let mut out = Plane::new(oh, ow);
    let area = (f * f) as f64;
    for by in 0..oh {
        for bx in 0..ow {
            let mut acc = 0.0;
            for dy in 0..f {
                let sy = reflect((by * f + dy) as isize, h as isize);
                for dx in 0..f {
                    let sx = reflect((bx * f + dx) as isize, w as isize);
                    acc += src.data[sy * w + sx] as f64;
                }
            }
            out.data[by * ow + bx] = (acc / area) as f32;
        }
    }
    out
}

/// Thu nhỏ: trung bình khối `f×f` với đệm phản chiếu (giống `downscale`).
pub fn downscale(src: &Plane, f: usize) -> Plane {
    let ph = (f - src.h % f) % f;
    let pw = (f - src.w % f) % f;
    block_mean(src, f, ph, pw)
}

/// Bilinear tâm nửa-pixel kiểu OpenCV `INTER_LINEAR` (giống `_resize_linear_xp`).
pub fn resize_linear(src: &Plane, out_h: usize, out_w: usize) -> Plane {
    let (h, w) = (src.h, src.w);
    let coords = |n_out: usize, n_in: usize| -> Vec<(usize, usize, f32)> {
        (0..n_out)
            .map(|i| {
                let s = ((i as f32 + 0.5) * (n_in as f32 / n_out as f32) - 0.5)
                    .clamp(0.0, (n_in - 1) as f32);
                let i0 = s.floor() as usize;
                let i1 = (i0 + 1).min(n_in - 1);
                (i0, i1, s - i0 as f32)
            })
            .collect()
    };
    let ys = coords(out_h, h);
    let xs = coords(out_w, w);
    let mut out = Plane::new(out_h, out_w);
    for (oy, &(y0, y1, wy)) in ys.iter().enumerate() {
        for (ox, &(x0, x1, wx)) in xs.iter().enumerate() {
            let top = src.at(y0, x0) * (1.0 - wx) + src.at(y0, x1) * wx;
            let bot = src.at(y1, x0) * (1.0 - wx) + src.at(y1, x1) * wx;
            out.data[oy * out_w + ox] = top * (1.0 - wy) + bot * wy;
        }
    }
    out
}

/// Nghịch đảo `downscale`: bilinear ×`f`, cắt về `h×w`.
pub fn upscale(small: &Plane, h: usize, w: usize, f: usize) -> Plane {
    let full = resize_linear(small, small.h * f, small.w * f);
    // cắt [:h, :w]
    let mut out = Plane::new(h, w);
    for y in 0..h {
        out.data[y * w..y * w + w].copy_from_slice(&full.data[y * full.w..y * full.w + w]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflect_border() {
        // BORDER_REFLECT: fedcba|abcdef|fedcba
        assert_eq!(reflect(-1, 6), 0);
        assert_eq!(reflect(-2, 6), 1);
        assert_eq!(reflect(6, 6), 5);
        assert_eq!(reflect(7, 6), 4);
        assert_eq!(reflect(3, 6), 3);
    }

    #[test]
    fn gaussian_kernel_normalized() {
        let k = gaussian_kernel(2.0);
        let sum: f64 = k.iter().sum();
        assert!((sum - 1.0).abs() < 1e-12);
        // đối xứng
        let n = k.len();
        for i in 0..n {
            assert!((k[i] - k[n - 1 - i]).abs() < 1e-12);
        }
    }
}
