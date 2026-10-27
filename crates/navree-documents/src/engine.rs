//! [`PdfEngine`]: the driver trait every PDF engine implements, and
//! [`EngineKind`].

use navree_kernel::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::render::Render;
use crate::{DocumentsError, Pdf};

/// The PDF engines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    /// Typst templates (`.typ`), compiled in process.
    #[default]
    Typst,
    /// HTML templates (`.html`), printed by headless Chrome.
    Chrome,
}

impl EngineKind {
    /// `typst` or `chrome`.
    pub fn name(self) -> &'static str {
        match self {
            EngineKind::Typst => "typst",
            EngineKind::Chrome => "chrome",
        }
    }

    /// The file extension of the engine's templates: `typ` or `html`.
    pub fn extension(self) -> &'static str {
        match self {
            EngineKind::Typst => crate::templates::TYPST_EXTENSION,
            EngineKind::Chrome => crate::templates::HTML_EXTENSION,
        }
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
            "typst" => Ok(EngineKind::Typst),
            "chrome" => Ok(EngineKind::Chrome),
            other => Err(format!("unknown PDF engine `{other}` (typst or chrome)")),
        }
    }
}

/// A PDF engine: turns a [`Render`] into a [`Pdf`].
///
/// [`TypstEngine`](crate::TypstEngine) and
/// [`ChromeEngine`](crate::ChromeEngine) implement it, and so does the
/// engine of [`DocumentsFake`](crate::DocumentsFake). Every
/// implementation passes the same conformance suite: a named template
/// renders with its data, an unknown name is
/// [`DocumentsError::TemplateNotFound`], a broken template is
/// [`DocumentsError::Template`] naming the file.
pub trait PdfEngine: Send + Sync + 'static {
    /// Which engine this is.
    fn kind(&self) -> EngineKind;

    /// Renders one document.
    fn render<'a>(&'a self, render: &'a Render) -> BoxFuture<'a, Result<Pdf, DocumentsError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_extensions() {
        assert_eq!(EngineKind::Typst.name(), "typst");
        assert_eq!(EngineKind::Chrome.extension(), "html");
        assert_eq!("chrome".parse::<EngineKind>(), Ok(EngineKind::Chrome));
        assert!("wkhtml".parse::<EngineKind>().is_err());
        assert_eq!(EngineKind::default(), EngineKind::Typst);
        assert_eq!(serde_json::to_value(EngineKind::Chrome).unwrap(), "chrome");
    }
}
