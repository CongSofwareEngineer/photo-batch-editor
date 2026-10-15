//! Tách nền bằng AI: model MODNet (portrait matting) chạy qua ONNX Runtime.
//!
//! Gắn sau feature `bgremove` (chỉ khi đó mới kéo `ort`). Tiền / hậu xử lý nằm trong
//! [`crate::photo::matting`] để test được không cần ONNX. Xem `docs/instruction/background-removal.md`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ndarray::Array4;
use ort::session::Session;
use ort::value::Tensor;

use crate::paths::resource_path;
use crate::photo::matting::{matte_from_model, model_input_size, preprocess, Matte};
use crate::photo::RgbaImage;

pub const MODEL_FILENAME: &str = "modnet.onnx";

/// Nơi tìm model: `models/` của gốc dự án (chạy từ mã nguồn / `PBE_APP_ROOT`), cạnh file exe
/// (Windows) hoặc `Contents/Resources/models` của bundle `.app` (macOS).
pub fn default_model_path() -> PathBuf {
    let mut candidates = vec![resource_path(["models", MODEL_FILENAME])];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        candidates.push(dir.join("models").join(MODEL_FILENAME));
        candidates.push(dir.join("../Resources/models").join(MODEL_FILENAME));
    }
    candidates
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .unwrap_or_else(|| candidates.remove(0))
}

/// Session ONNX tái sử dụng (nạp model một lần, ~25 MB).
pub struct BackgroundRemover {
    session: Mutex<Session>,
    input_name: String,
}

impl BackgroundRemover {
    pub fn new(model_path: Option<&Path>) -> Result<Self, String> {
        let path = model_path
            .map(Path::to_path_buf)
            .unwrap_or_else(default_model_path);
        if !path.is_file() {
            return Err(format!(
                "Background removal model not found: {}",
                path.display()
            ));
        }
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("Could not load background removal model: {e}"))?;
        let session = Session::builder()
            .and_then(|b| b.commit_from_memory(&bytes))
            .map_err(|e| format!("Could not load background removal model: {e}"))?;
        let input_name = session.inputs[0].name.clone();
        Ok(BackgroundRemover {
            session: Mutex::new(session),
            input_name,
        })
    }

    /// Matte (0 = nền, 255 = người) cùng cỡ `img`.
    pub fn matte(&self, img: &RgbaImage) -> Result<Matte, String> {
        if img.is_empty() {
            return Err("Empty image".to_string());
        }
        let (mw, mh) = model_input_size(img.w, img.h);
        let input = Array4::from_shape_vec((1, 3, mh, mw), preprocess(img, mw, mh))
            .map_err(|e| e.to_string())?;
        let tensor = Tensor::from_array(input).map_err(|e| e.to_string())?;
        let low = {
            let mut session = self.session.lock().unwrap();
            let outputs = session
                .run(ort::inputs![self.input_name.as_str() => tensor])
                .map_err(|e| e.to_string())?;
            let (shape, data) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| e.to_string())?;
            let (oh, ow) = (
                shape[shape.len() - 2] as usize,
                shape[shape.len() - 1] as usize,
            );
            if (oh, ow) != (mh, mw) || data.len() != mh * mw {
                return Err(format!("Unexpected model output shape {shape:?}"));
            }
            data.to_vec()
        };
        Ok(matte_from_model(img, &low, mh, mw))
    }
}
