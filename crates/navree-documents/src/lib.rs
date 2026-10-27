//! # navree-documents
#![forbid(unsafe_code)]

mod engine;
mod error;
mod pdf;
mod print;
mod render;
mod stamp;
mod templates;
mod text;
#[cfg(feature = "typst")]
mod typst_engine;

pub use engine::{EngineKind, PdfEngine};
pub use error::DocumentsError;
pub use pdf::Pdf;
pub use print::{Margins, Paper, PrintOptions};
pub use render::{Render, TemplateSource, is_rtl};
pub use stamp::{Pages, Position, Stamp};
pub use templates::{HTML_EXTENSION, TYPST_EXTENSION, Templates};
#[cfg(feature = "typst")]
pub use typst_engine::TypstEngine;
