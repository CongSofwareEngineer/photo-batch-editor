//! Pipeline xử lý một ảnh theo thứ tự cố định (section 6). Port của `core/pipeline.py`.
//!
//! GĐ3: chỉnh sửa (1–15) → Super Resolution → Image Size. Super Resolution (ONNX) để GĐ3b:
//! khi bật, hiện bỏ qua kèm cảnh báo (giống trường hợp thiếu model của bản Python) rồi vẫn
//! chạy tiếp Image Size — nhờ vậy mọi nhánh không-SR (phần lớn test) đã đầy đủ và kiểm chứng được.

use crate::adjustments::{self, ImageF32, ImageU16, ImageU8, Planar};
use crate::image_size::{sr_exceeds_limits, SR_SKIP_WARNING};
use crate::resize::resize_image;
use crate::settings::AdjustmentSettings;

/// Nguồn ảnh đã đọc từ đĩa (giữ nguyên độ sâu bit như bản Python để LUT khớp chính xác).
pub enum Input<'a> {
    U8(&'a ImageU8),
    U16(&'a ImageU16),
    F32(&'a ImageF32),
}

/// Bộ phóng to AI (Super Resolution). Cài đặt thật ở [`crate::enhance`] (feature `sr`);
/// tách trait để `core` không phụ thuộc ONNX runtime khi không cần.
pub trait Upscaler {
    /// Phóng to ảnh float32 `(H,W,3)` trong `[0,1]` theo hệ số 2 hoặc 4.
    fn upscale(&self, img: &ImageF32, factor: i64) -> Result<ImageF32, String>;
}

/// Float (…,3) trong [0,1] → uint8, làm tròn (round-half-to-even như numpy.rint).
pub fn to_uint8(img: &ImageF32) -> ImageU8 {
    let data = img
        .data
        .iter()
        .map(|&v| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8)
        .collect();
    ImageU8 {
        h: img.h,
        w: img.w,
        data,
    }
}

/// Chỉnh sửa 1–15; trả về ảnh HWC float32. Giống `pipeline.apply_adjustments`.
pub fn apply_adjustments_u8(img: &ImageU8, s: &AdjustmentSettings) -> ImageF32 {
    adjustments::apply_adjustments_u8(img, s)
}

pub fn apply_adjustments_f32(img: &ImageF32, s: &AdjustmentSettings) -> ImageF32 {
    adjustments::apply_adjustments_f32(img, s)
}

fn planar_for(input: &Input, s: &AdjustmentSettings) -> Planar {
    match input {
        Input::U8(img) => adjustments::apply_adjustments_planar_u8(img, s),
        Input::U16(img) => adjustments::apply_adjustments_planar_u16(img, s),
        Input::F32(img) => adjustments::apply_adjustments_planar_f32(img, s),
    }
}

/// Các bước 1–7 cho ảnh đọc từ đĩa; trả về `(uint8 RGB, warnings)`. Không bao giờ panic.
///
/// `sr` là bộ Super Resolution tuỳ chọn (xem [`Upscaler`]). `None` → bỏ qua SR kèm cảnh báo
/// (như khi thiếu model ở bản Python).
pub fn process(input: Input, s: &AdjustmentSettings, sr: Option<&dyn Upscaler>) -> (ImageU8, Vec<String>) {
    let p = planar_for(&input, s);
    let factor = s.sr_factor();
    if factor == 1 && s.image_size.is_default() {
        return (adjustments::planar_to_uint8(&p), vec![]);
    }
    let mut host = adjustments::planar_to_hwc(&p);
    let mut warnings = vec![];
    if factor > 1 {
        if sr_exceeds_limits(host.w as i64, host.h as i64, factor) {
            warnings.push(SR_SKIP_WARNING.to_string());
        } else {
            match sr {
                Some(upscaler) => match upscaler.upscale(&host, factor) {
                    Ok(up) => host = up,
                    Err(e) => warnings.push(format!("Super Resolution skipped: {e}")),
                },
                None => warnings.push(
                    "Super Resolution skipped: không có bộ Super Resolution (feature `sr` tắt)".to_string(),
                ),
            }
        }
    }
    let (resized, more) = resize_image(&host, &s.image_size);
    warnings.extend(more);
    (to_uint8(&resized), warnings)
}

/// Tiện ích cho nhánh u8 không SR (tương thích phần test số học hiện có).
pub fn process_u8(img: &ImageU8, s: &AdjustmentSettings) -> Result<(ImageU8, Vec<String>), String> {
    Ok(process(Input::U8(img), s, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ImageSizeSettings;

    #[test]
    fn to_uint8_rounds() {
        let img = ImageF32 {
            h: 1,
            w: 2,
            data: vec![0.0, 0.5 / 255.0, 1.0, 1.0, 1.0, 1.0],
        };
        let out = to_uint8(&img);
        assert_eq!(out.data[0], 0);
        assert_eq!(out.data[2], 255);
    }

    #[test]
    fn resize_path_changes_dims() {
        let img = ImageU8 { h: 40, w: 60, data: vec![128; 40 * 60 * 3] };
        let s = AdjustmentSettings {
            image_size: ImageSizeSettings::new("long_edge", 30.0, "automatic", false),
            ..Default::default()
        };
        let (out, warnings) = process(Input::U8(&img), &s, None);
        assert_eq!((out.w, out.h), (30, 20));
        assert!(warnings.is_empty());
    }
}
