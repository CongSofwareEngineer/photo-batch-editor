//! Photo Batch Editor — core logic, ported from the Python `core/` package.
//!
//! Giai đoạn 1 của việc port sang Rust (xem `docs/instruction/rust-port.md`):
//! các module logic thuần, test được ngay, không phụ thuộc OpenCV / ONNX / Qt.
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
pub mod pipeline;
pub mod presets;
pub mod resize;
pub mod scanner;
pub mod settings;
pub mod system;
pub mod version;
