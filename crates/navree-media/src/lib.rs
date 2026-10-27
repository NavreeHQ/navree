//! # navree-media
//!
//! `MediaModule`: files attached to models, and what an app does with
//! them.
//!
//! - **Attachments.** A model implements [`HasMedia`] (its type name, its
//!   key and its [`MediaCollections`]); [`Media::add`] stores a file from
//!   an upload, a URL, a disk or the local filesystem ([`MediaSource`]) in
//!   one of its collections, with the type sniffed from the content and
//!   checked against the collection. Rows live in the `media` table
//!   ([`MediaRecord`], a navree-orm model), files on navree-storage disks
//!   under `{prefix}/{id}/`.
//! - **Conversions.** A collection declares [`Conversion`]s (resize, crop,
//!   re-encode to JPEG, PNG or WebP, greyscale, blur, responsive variants
//!   for `srcset`), generated on the queue (job `media.conversions`) or
//!   while the file is added, and again with `media:regenerate`. Originals
//!   lose their EXIF, XMP and text metadata; SVGs are rendered with resvg.
//! - **Remote files.** [`MediaSource::url`] downloads through
//!   navree-http-client and its SSRF guard, capped in size and time.
//! - **Archives.** [`archive::Archive`] reads zip files with zip-slip,
//!   symlink, encryption and size-bomb guards, and unpacks them to a disk;
//!   [`archive::ZipBuilder`] writes them.
//! - **Text.** [`text::extract`] reads the text of PDF, DOCX, HTML and
//!   plain-text files (for search and retrieval).
//! - **Video and audio** (feature `ffmpeg`): an FFmpeg sidecar probes
//!   files, grabs poster frames and transcodes ([`video`]).
//! - **Tests.** [`MediaFake`] runs the real pipeline on memory disks and
//!   rows and records what was added and deleted.
//!
//! ```
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! use navree_media::{Conversion, Fit, HasMedia, MediaCollections, MediaFake, MediaSource, OutputFormat};
//!
//! struct Course {
//!     id: i64,
//! }
//!
//! impl HasMedia for Course {
//!     const MEDIA_TYPE: &'static str = "Course";
//!
//!     fn media_key(&self) -> String {
//!         self.id.to_string()
//!     }
//!
//!     fn media_collections(collections: &mut MediaCollections) {
//!         collections
//!             .collection("thumbnail")
//!             .single_file()
//!             .accepts(["image/*"])
//!             .conversion(Conversion::new("card").fit(Fit::Cover, 64, 36).format(OutputFormat::WebP));
//!     }
//! }
//!
//! # let png = {
//! #     let img = image::RgbImage::from_pixel(160, 120, image::Rgb([200, 40, 40]));
//! #     let mut out = std::io::Cursor::new(Vec::new());
//! #     image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
//! #     out.into_inner()
//! # };
//! let media = MediaFake::new().media(); // in an app: resolve `Arc<Media>`
//! let course = Course { id: 7 };
//! let item = media
//!     .add(&course, "thumbnail", MediaSource::bytes("cover.png", png))
//!     .property("alt", "The course cover")
//!     .save(None)
//!     .await?;
//! assert_eq!((item.width, item.height), (Some(160), Some(120)));
//! let card = &item.conversions["card"];
//! assert_eq!((card.width, card.height, card.mime_type.as_str()), (Some(64), Some(36), "image/webp"));
//! # Ok::<(), navree_media::MediaError>(())
//! # }).unwrap();
//! ```
#![forbid(unsafe_code)]

pub mod archive;
mod collections;
mod commands;
mod conversion;
mod error;
mod fake;
mod generate;
pub mod images;
mod item;
mod jobs;
pub mod mime;
mod migration;
mod model;
mod module;
mod options;
mod service;
mod source;
mod store;
pub mod text;
#[cfg(feature = "ffmpeg")]
pub mod video;

pub use collections::{HasMedia, MediaCollection, MediaCollections};
pub use conversion::{Conversion, DEFAULT_QUALITY, Fit, OutputFormat};
pub use error::{ArchiveError, MediaError};
pub use fake::MediaFake;
pub use item::{GeneratedConversion, MediaItem, MediaResource, ResponsiveVariant};
pub use jobs::GenerateConversions;
pub use migration::{MEDIA_MIGRATIONS, MEDIA_TABLE};
pub use model::MediaRecord;
pub use module::{MediaModule, ModuleBuilderMediaExt};
pub use options::{MediaOptions, StoreKind};
pub use service::{Media, MediaBuilder, PendingMedia, Regenerate, RegenerateReport};
pub use source::MediaSource;
pub use store::{DatabaseStore, MediaQuery, MediaStore, MemoryStore};
