//! # navree-documents
//!
//! `DocumentsModule`: PDF and Word documents from your app's data.
//!
//! - **Two PDF engines** behind one service, [`Documents`]:
//!   [`TypstEngine`] compiles `.typ` templates in process (no external
//!   program), with bundled Noto fonts for Latin, Greek, Cyrillic, Arabic
//!   and Devanagari; [`ChromeEngine`] prints `.html` templates (rendered
//!   with navree-view) in headless Chrome over the DevTools protocol, with
//!   a small page pool, assets served from the template tree and every
//!   other origin blocked. `documents.pdf("certificates/classic", &data)`
//!   picks the engine by the template's file.
//! - **Right to left**: a render's locale sets the language and direction
//!   in both engines; text extraction ([`Pdf::text`]) returns right-to-left
//!   text in reading order, so golden-file tests read naturally.
//! - **Editing** with [`Pdf`]: [`merge`](Pdf::merge), text
//!   [`stamp`](Pdf::stamp)s (watermarks, "PAID") and
//!   [`overlay`](Pdf::overlay)s (a seal or signature in any script).
//! - **DOCX** with [`Docx`]: headings, formatted paragraphs, lists,
//!   tables, images, page numbers, right-to-left documents.
//! - **Responses**: [`Pdf`] and [`DocxFile`] are responses, and
//!   [`Pdf::download`] names the file ([`Download`]).
//! - **Stored and queued**: [`Documents::save`] writes to a storage disk;
//!   [`Documents::queue`] dispatches the `documents.render` job and returns
//!   a [`QueuedDocument`] to poll with [`Documents::status`]; with the
//!   `operations` feature, `Documents::queue_operation` queues it as the
//!   work of a navree-queue operation and returns the operation.
//! - **Tests**: [`DocumentsFake`] renders without engines and records
//!   every render; every engine passes the same conformance suite.
//!
//! ```
//! use navree_documents::{Documents, Pdf, Stamp, Templates};
//! use serde_json::json;
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), navree_documents::DocumentsError> {
//! let documents = Documents::new(Templates::from_files([(
//!     "certificates/classic.typ",
//!     "#let data = json(\"/data.json\")\n#set text(lang: \"ar\", dir: rtl)\nشهادة إتمام\n\n#data.name",
//! )]));
//! let pdf = documents
//!     .pdf("certificates/classic", json!({ "name": "مريم العلي" }))
//!     .await?;
//! assert_eq!(pdf.text()?, "شهادة إتمام\nمريم العلي");
//!
//! let draft = pdf.stamp(&Stamp::text("DRAFT"))?;
//! assert_eq!(Pdf::merge([&pdf, &draft])?.page_count()?, 2);
//! # Ok(())
//! # }
//! ```
//!
//! Crate features: `typst`, `chrome` and `docx` (on by default), and
//! `operations` (off by default; it adds navree-queue and navree-db).
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
#[cfg(any(feature = "typst", feature = "chrome"))]
mod fonts;
mod module;
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
#[cfg(any(feature = "typst", feature = "chrome"))]
pub use fonts::FONTS_STYLESHEET;
pub use module::{DocumentsModule, ModuleBuilderDocumentsExt};
pub use options::{ChromeOptions, DocumentsOptions, TypstOptions};
pub use pdf::Pdf;
pub use print::{Margins, Paper, PrintOptions};
pub use queue::{
    DocumentStatus, FAILED_SUFFIX, QueuedDocument, RENDER_FAILED_MESSAGE, RenderDocument,
    StoredDocument,
};
pub use render::{Render, TemplateSource, is_rtl};
pub use stamp::{Pages, Position, Stamp};
pub use templates::{HTML_EXTENSION, TYPST_EXTENSION, Templates};
#[cfg(feature = "typst")]
pub use typst_engine::TypstEngine;
