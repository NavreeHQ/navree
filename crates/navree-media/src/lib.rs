//! # navree-media
#![forbid(unsafe_code)]

pub mod archive;
mod conversion;
mod error;
pub mod images;
pub mod mime;
pub mod text;
#[cfg(feature = "ffmpeg")]
pub mod video;

pub use conversion::{Conversion, DEFAULT_QUALITY, Fit, OutputFormat};
pub use error::{ArchiveError, MediaError};
