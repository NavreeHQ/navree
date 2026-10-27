//! # navree-documents
#![forbid(unsafe_code)]

#[cfg(feature = "chrome")]
mod chrome;
mod commands;
mod documents;
#[cfg(feature = "docx")]
pub mod docx;
mod download;
mod engine;
mod error;
mod fake;
mod module;
#[cfg(any(feature = "typst", feature = "chrome"))]
mod fonts;
mod options;
mod pdf;
mod print;
mod queue;
mod render;
mod stamp;
mod templates;
mod text;
#[cfg(feature = "typst")]
mod typst_engine;

#[cfg(feature = "chrome")]
pub use chrome::{CHROME_ENV, ChromeEngine, DOCUMENT_ORIGIN};
pub use documents::Documents;
#[cfg(feature = "docx")]
pub use docx::{Docx, DocxFile};
pub use download::{DOCX_CONTENT_TYPE, Download, PDF_CONTENT_TYPE};
pub use engine::{EngineKind, PdfEngine};
pub use error::DocumentsError;
pub use fake::{DocumentsFake, FakeEngine, QueuedRender};
pub use module::{DocumentsModule, ModuleBuilderDocumentsExt};
#[cfg(any(feature = "typst", feature = "chrome"))]
pub use fonts::FONTS_STYLESHEET;
pub use options::{ChromeOptions, DocumentsOptions, TypstOptions};
pub use pdf::Pdf;
pub use print::{Margins, Paper, PrintOptions};
pub use queue::{DocumentStatus, FAILED_SUFFIX, QueuedDocument, RenderDocument, StoredDocument};
pub use render::{Render, TemplateSource, is_rtl};
pub use stamp::{Pages, Position, Stamp};
pub use templates::{HTML_EXTENSION, TYPST_EXTENSION, Templates};
#[cfg(feature = "typst")]
pub use typst_engine::TypstEngine;
