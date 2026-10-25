//! [`ViewError`]: everything that can go wrong while loading or rendering templates.

use std::path::PathBuf;

/// An error raised while loading or rendering a template.
///
/// Messages name the template (and the file or directory involved) but never
/// contain context values: a rendering context may hold tokens or other
/// secrets.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ViewError {
    /// No template matches the name.
    #[error("template `{name}` not found (looked for {})", .tried.join(", "))]
    NotFound {
        /// The name passed to [`Views::render`](crate::Views::render).
        name: String,
        /// The template names tried, in order.
        tried: Vec<String>,
    },
    /// A template failed to compile or render.
    #[error("template `{name}` failed: {source}")]
    Template {
        /// The template that failed (with its extension).
        name: String,
        /// The template engine's error.
        #[source]
        source: minijinja::Error,
    },
    /// A template directory or file cannot be read.
    #[error("cannot read `{}`: {source}", .path.display())]
    Io {
        /// The directory or file.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
}
