//! # navree-media
#![forbid(unsafe_code)]

mod conversion;
mod error;
pub mod images;
#[cfg(feature = "ffmpeg")]
pub mod video;

pub use conversion::{Conversion, DEFAULT_QUALITY, Fit, OutputFormat};
pub use error::{ArchiveError, MediaError};
