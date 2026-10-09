//! Photo Batch Editor — core logic, ported from the Python `core/` package.
//!
//! Các giai đoạn 1–4 của việc port sang Rust (xem `docs/instruction/rust-port.md`):
//! logic thuần, số học ảnh, I/O, batch, rồi `photo/` + `video/`. Không phụ thuộc OpenCV / Qt
//! (ONNX chỉ khi bật feature `sr`).
//!
//! `core/` của Python không được import Qt; crate này cũng vậy.

pub mod adjustments;
pub mod auth;
pub mod backend;
pub mod batch;
#[cfg(feature = "sr")]
pub mod enhance;
pub mod i18n;
pub mod i18n_vi;
pub mod io_utils;
pub mod image_size;
pub mod paths;
pub mod photo;
pub mod pipeline;
pub mod presets;
pub mod resize;
pub mod scanner;
pub mod settings;
pub mod system;
pub mod version;
pub mod video;
