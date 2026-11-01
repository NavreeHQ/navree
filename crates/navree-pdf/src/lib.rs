//! # navree-pdf
//!
//! Navree's PDF object layer, under navree-documents (`Pdf`: page counts,
//! text, merge, stamps, overlays, metadata) and navree-media (text from
//! uploaded PDFs): one reader, one writer, one set of limits.
//!
//! ```
//! use navree_pdf::content::{Operation, encode};
//! use navree_pdf::{Dictionary, Document, Object, Objects, ReadLimits, Stream, dictionary};
//!
//! # fn main() -> Result<(), navree_pdf::PdfError> {
//! // Write a one-page document...
//! let mut objects = Objects::new();
//! let pages = objects.reserve();
//! let font = objects.add(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
//! let content = objects.add(Stream::new(
//!     Dictionary::new(),
//!     encode(&[
//!         Operation::new("BT", vec![]),
//!         Operation::new("Tf", vec![Object::name("F1"), 12.into()]),
//!         Operation::new("Td", vec![72.into(), 720.into()]),
//!         Operation::new("Tj", vec![Object::string_literal("Hello")]),
//!         Operation::new("ET", vec![]),
//!     ]),
//! ));
//! let page = objects.add(dictionary! {
//!     "Type" => "Page", "Parent" => pages, "Contents" => content,
//!     "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
//! });
//! objects.insert(pages, dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 });
//! let catalog = objects.add(dictionary! { "Type" => "Catalog", "Pages" => pages });
//! objects.trailer.set("Root", catalog);
//!
//! // ...and read it back.
//! let document = Document::load(objects.save()?, ReadLimits::default())?;
//! assert_eq!(document.pages().len(), 1);
//! assert_eq!(navree_pdf::text::page_texts(&document)?, ["Hello"]);
//! # Ok(())
//! # }
//! ```
//!
//! ## Reading
//!
//! [`Document::load`] reads the header, the cross-reference and the
//! trailer: cross-reference tables and streams, hybrid files, incremental
//! updates back through `/Prev` (a section seen twice ends the chain), and
//! linearized files. A cross-reference that cannot be read, or whose
//! entries mostly do not point at their objects, is rebuilt by scanning
//! the file for objects. Every object is read the first time it is asked
//! for, from the file or from an object stream, and kept.
//!
//! Streams decode through Flate and LZW (with the PNG and TIFF predictors),
//! ASCII85, ASCIIHex and RunLength; image codecs (DCT, JPX, JBIG2, CCITT)
//! stay encoded. A file encrypted with the standard security handler and
//! an empty password to open it (an owner password that only restricts
//! printing or copying) is decrypted as its objects are read: RC4 and AES
//! of 40 to 256 bits, with RustCrypto's ciphers and hashes. One that needs
//! a password is [`PdfError::Encrypted`].
//!
//! ## The budget
//!
//! A document is read within its [`ReadLimits`]: the memory its objects may
//! hold (256 MiB), counted as each object, string, name and dictionary
//! entry is built and as each object stream is inflated, and the bytes the
//! reader may scan (1 GiB). A small file whose objects would take more is
//! refused with [`PdfError::TooLarge`], however they come to take it:
//! objects of an object stream that share their bytes, containers nested
//! deep (100 levels at most) or wide, cross-reference entries that send the
//! reader over the same bytes again. A request for an object never fails:
//! one that is missing or damaged is `None`, and so is every request after
//! a limit ran out; [`Document::check`] returns that error.
//!
//! ## Text
//!
//! [`text::page_texts`] reads what each page shows, in reading order, held
//! to its own [`TextLimits`]: work and text per page and per document, the
//! glyphs of a page, the nesting of saved states and marked content, caps
//! on `ToUnicode` maps, and the memory of the fonts read.
//!
//! ## Writing
//!
//! [`Objects`] holds a document's objects to be changed (renumbered,
//! pruned, compressed) and saves them, with a cross-reference table or with
//! object streams and a cross-reference stream.

#![forbid(unsafe_code)]

mod budget;
pub mod content;
mod crypt;
mod document;
mod error;
mod filters;
mod object;
mod parse;
pub mod text;
mod write;
mod xref;

pub use budget::ReadLimits;
pub use document::{ContentError, Document};
pub use error::PdfError;
pub use object::{Dictionary, Object, ObjectId, Stream, StringFormat};
pub use text::TextLimits;
pub use write::Objects;
