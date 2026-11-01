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
//! A document is read within its [`ReadLimits`]. What a file can make the
//! reader do, in terms that can be checked:
//!
//! - **Work.** Every byte the reader looks at is charged to
//!   [`ReadLimits::work`] (1 GiB), again each time it is looked at: what a
//!   scan passes, what a parse reached before it failed and moved back, what
//!   each filter reads and writes, whether the next filter works or not. So
//!   is every object looked up: [`Document::object`], the one way to an object,
//!   charges 32 bytes, read before or not, so following a reference cannot be
//!   free, and a chain of references stops at 8. No loop looks through a table
//!   once for each entry: scans move forward, and tables are looked up by key.
//!   The time a document takes is in proportion to what is charged
//!   ([`Document::work_used`]), and is linear in the file until the limit stops
//!   it.
//! - **Memory.** Each object, string and dictionary entry, each object
//!   stream and cross-reference stream inflated, and each entry of the
//!   cross-reference is counted against [`ReadLimits::memory`] (256 MiB) as
//!   it is built, and [`Document::memory_used`] says how much. An object is 32
//!   bytes where it sits: a dictionary is a list of entries (a map past
//!   sixteen of them), a short name is kept inside the entry and a stream is
//!   a pointer to its dictionary and its bytes. What the process holds for
//!   them at its peak is at most twice what is counted (1.15 times for a
//!   tagged spreadsheet of 10,000 rows, 1.95 for a dictionary of 700,000
//!   keys), so 256 MiB counted is at most 512 MiB held. The bytes of a stream
//!   are the file's own, and are not copied.
//! - **Bytes out.** A stream ends before the next object starts, whatever its
//!   `Length` says, so the streams a document holds add up to the file's
//!   size (twice that when the cross-reference is wrong about some objects).
//!   [`Objects::save`] writes at most the file's size plus
//!   [`ReadLimits::memory`]. A stream has 8 filters at most.
//! - **Edits.** [`Objects`] from a document have budgets of their own, as large
//!   as the document's: each lookup costs work as above, and what is added is
//!   counted as it is added ([`Objects::add`], [`Objects::hold`]), before it is
//!   held. Compressing a stream takes 4 KiB of the work and 2 bytes for each of
//!   its own. [`Objects::check`] and saving return the first that ran out. Edits
//!   that read more than one document ([`Objects::limits_left`],
//!   [`Objects::share_limits`]) read the next within what the ones before left.
//!
//! A small file that would take more is refused with
//! [`PdfError::TooLarge`], however it comes to take it. A request for an
//! object never fails: one that is missing or damaged is `None`, and so is
//! every request after a limit ran out; [`Document::check`] returns that
//! error.
//!
//! ## Text
//!
//! [`text::page_texts`] reads what each page shows, in reading order, held
//! to its own [`TextLimits`]: work and text per page and per document, the
//! character codes of a page, the nesting of saved states and marked content,
//! caps on `ToUnicode` maps, enforced by the one reader of a map as it reads,
//! and the memory of the fonts read. A font's `ToUnicode` map comes before its
//! `Encoding`.
//!
//! The work of a page is charged by construction: every operation pays before
//! its operator is looked at, every character code before it is decoded, and
//! every charge also takes the objects the document looked up since the last.
//! What is left out is therefore not free, only cheaper than it should be, and
//! `tests/guarantee.rs` runs every shape known to come close against the time
//! a byte of the densest content takes in the same run ([`text::page_texts_counted`]
//! gives what was charged). No byte charged takes more than 3 of those. In a
//! release build, where that byte took about 7.5 ns on one core, the slowest
//! shape is pages that each show a million random two-byte codes through a map
//! of 16,000 lines and 65,000 widths, 1.8 of them (about 14 ns a byte): the
//! whole of [`TextLimits::document_work`] (1 GiB) spent on it took 22 s. The
//! densest plain shape, pages showing a line of a million glyphs, is 1.25 of
//! them, and took 15 s. Looking up an object far from the last takes 1.2 of
//! them, and compressing, at the fastest level of Flate, 0.6 at the most. A
//! byte of [`ReadLimits::work`] takes about 10 ns at the most. Peak memory at
//! the default limits is at most about 1.1 GiB for text, and about 1.4 GiB plus
//! twice the file for an edit, as the documents guide adds up.
//!
//! ## Writing
//!
//! [`Objects`] holds a document's objects to be changed (renumbered,
//! pruned, compressed) and saves them, with a cross-reference table or with
//! object streams and a cross-reference stream. [`Objects::read`] refuses a
//! document when an object it needs cannot be read, since what is saved
//! would lose it.

#![forbid(unsafe_code)]

mod budget;
pub mod content;
mod crypt;
mod document;
mod error;
mod filters;
mod name;
mod object;
mod parse;
pub mod text;
mod write;
mod xref;

pub use budget::ReadLimits;
pub use document::{ContentError, Document};
pub use error::PdfError;
pub use name::Name;
pub use object::{Dictionary, Iter, IterMut, Object, ObjectId, Stream, StringFormat};
pub use text::TextLimits;
pub use write::{Objects, memory_of};
