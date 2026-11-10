//! Upload: a drop area around a native file input, and the server that receives what it
//! sends.
//!
//! - [`Upload`] is the field. With an action it is a multipart form of its own, and a page
//!   that never loads a script still sends its files with a plain post. The `upload` module
//!   adds the rest: a drop area, a list of the files with progress, a way to stop one and to
//!   send it again, and large files in chunks.
//! - [`UploadLimits`] says what is accepted. The page shows it as a hint; the server holds
//!   every file to it.
//! - [`UploadedFile`] is a file the server already has, as a row of the list.
//! - [`file_key`] signs a stored file's path, so a form's file field
//!   (`#[form(widget = file)]`) can post the key of a file the module already sent, and
//!   [`file_path`] reads it back.
//! - `Uploads` (with the `storage` feature) receives the files into a navree-storage disk. It
//!   counts a file's size while the bytes arrive, reads its kind from its first bytes, treats
//!   its name as text to show, and puts the parts of a large file together only when every
//!   part is here and the whole passes the same checks.
//!
//! The browser does not draw a preview of a chosen file: the page's content security policy
//! allows images only from this site and as `data:` URLs, and a preview needs a `blob:` URL.
//! Once the server has stored an image, show it from its own address.

mod component;
mod key;
mod labels;
mod limits;
#[cfg(feature = "storage")]
mod server;

pub use component::{Upload, UploadedFile};
pub use key::{file_key, file_path};
pub use labels::UploadLabels;
pub use limits::UploadLimits;
#[cfg(feature = "storage")]
pub use server::{ChunkStart, ChunkTicket, Received, Refused, UploadCode, UploadError, Uploads};
