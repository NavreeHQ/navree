//! # navree-documents
#![forbid(unsafe_code)]

#[cfg(feature = "chrome")]
mod chrome;
mod engine;
mod error;
#[cfg(any(feature = "typst", feature = "chrome"))]
mod fonts;
mod options;
mod pdf;
mod print;
mod render;
mod stamp;
mod templates;
mod text;
#[cfg(feature = "typst")]
mod typst_engine;

#[cfg(feature = "chrome")]
pub use chrome::{CHROME_ENV, ChromeEngine, DOCUMENT_ORIGIN};
pub use engine::{EngineKind, PdfEngine};
pub use error::DocumentsError;
#[cfg(any(feature = "typst", feature = "chrome"))]
pub use fonts::FONTS_STYLESHEET;
pub use options::{ChromeOptions, DocumentsOptions, TypstOptions};
pub use pdf::Pdf;
pub use print::{Margins, Paper, PrintOptions};
pub use render::{Render, TemplateSource, is_rtl};
pub use stamp::{Pages, Position, Stamp};
pub use templates::{HTML_EXTENSION, TYPST_EXTENSION, Templates};
#[cfg(feature = "typst")]
pub use typst_engine::TypstEngine;
