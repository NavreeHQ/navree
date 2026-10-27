//! [`PdfEngine`]: the driver trait every PDF engine implements, and
//! [`EngineKind`].

use navree_kernel::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::render::Render;
use crate::{DocumentsError, Pdf};

/// The PDF engines. Both print `.html` templates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    /// Laid out in process by the native engine (the HTML subset and
    /// [`PdfDocument`](crate::PdfDocument)s).
    #[default]
    Native,
    /// Printed by headless Chrome (full CSS).
    Chrome,
}

impl EngineKind {
    /// `native` or `chrome`.
    pub fn name(self) -> &'static str {
        match self {
            EngineKind::Native => "native",
            EngineKind::Chrome => "chrome",
        }
    }

    /// The file extension of the engine's templates: `html` for both.
    pub fn extension(self) -> &'static str {
        crate::templates::HTML_EXTENSION
    }
}

impl std::fmt::Display for EngineKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for EngineKind {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "native" => Ok(EngineKind::Native),
            "chrome" => Ok(EngineKind::Chrome),
            "typst" => Err(
                "the typst engine was replaced by `native`: write the template as HTML"
                    .to_owned(),
            ),
            other => Err(format!("unknown PDF engine `{other}` (native or chrome)")),
        }
    }
}

/// A PDF engine: turns a [`Render`] into a [`Pdf`].
///
/// [`NativeEngine`](crate::NativeEngine) and
/// [`ChromeEngine`](crate::ChromeEngine) implement it, and so does the
/// engine of [`DocumentsFake`](crate::DocumentsFake). Every
/// implementation passes the same conformance suite: a named template
/// renders with its data (with or without its extension), inline markup
/// renders, an unknown name is [`DocumentsError::TemplateNotFound`]
/// naming the file tried, data that does not serialize is
/// [`DocumentsError::Data`], and a render's title lands in the PDF's
/// metadata. The real engines also report a broken template as
/// [`DocumentsError::Template`] naming the file.
pub trait PdfEngine: Send + Sync + 'static {
    /// Which engine this is.
    fn kind(&self) -> EngineKind;

    /// Renders one document.
    fn render<'a>(&'a self, render: &'a Render) -> BoxFuture<'a, Result<Pdf, DocumentsError>>;

    /// Releases what the engine holds (Chrome closes its browser). The
    /// engine stays usable: the next render starts again. The default does
    /// nothing.
    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_extensions() {
        assert_eq!(EngineKind::Native.name(), "native");
        assert_eq!(EngineKind::Native.extension(), "html");
        assert_eq!(EngineKind::Chrome.extension(), "html");
        assert_eq!("chrome".parse::<EngineKind>(), Ok(EngineKind::Chrome));
        assert_eq!("native".parse::<EngineKind>(), Ok(EngineKind::Native));
        assert!("wkhtml".parse::<EngineKind>().is_err());
        assert!("typst".parse::<EngineKind>().unwrap_err().contains("native"));
        assert_eq!(EngineKind::default(), EngineKind::Native);
        assert_eq!(serde_json::to_value(EngineKind::Chrome).unwrap(), "chrome");
    }
}
