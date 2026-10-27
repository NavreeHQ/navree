//! [`Render`]: what to render, with which data, locale and engine.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::EngineKind;
use crate::print::PrintOptions;

/// Where the template of a render comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TemplateSource {
    /// A template by name (`certificates/classic`): `<name>.typ` for
    /// Typst, `<name>.html` for Chrome.
    Template(String),
    /// Typst markup given inline.
    Typst(String),
    /// HTML given inline (printed as it is, not run through a template
    /// engine).
    Html(String),
}

/// One document to render: the template, its data, the locale and,
/// optionally, the engine and the print settings.
///
/// ```
/// use navree_documents::{EngineKind, Render};
/// use serde_json::json;
///
/// let render = Render::template("certificates/classic")
///     .data(json!({ "name": "Ada Lovelace", "course": "Analytical Engines" }))
///     .locale("ar");
/// assert_eq!(render.engine, None); // chosen by the template's file
///
/// let render = Render::html("<h1>Invoice 42</h1>").engine(EngineKind::Chrome);
/// ```
///
/// A `Render` serializes, so a queued render ([`Documents::queue`](crate::Documents::queue))
/// carries it as the job's payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Render {
    /// The template.
    pub source: TemplateSource,
    /// The data the template reads: `json("/data.json")` in Typst, the
    /// template variables in HTML templates. `null` when not given.
    #[serde(default)]
    pub data: Value,
    /// The language of the document (`en`, `ar`, `hi`, `pt-BR`): sets the
    /// text language and direction (right to left for Arabic, Hebrew,
    /// Persian and Urdu). `None`: the template's own, or English.
    #[serde(default)]
    pub locale: Option<String>,
    /// The engine. `None`: picked by the template (`.typ` is Typst,
    /// `.html` is Chrome; with both, `documents.engine` decides).
    #[serde(default)]
    pub engine: Option<EngineKind>,
    /// Page settings for Chrome. `None`: `documents.chrome.print`. Typst
    /// templates set their own pages with `#set page(..)`.
    #[serde(default)]
    pub print: Option<PrintOptions>,
    /// The PDF's title in its metadata. `None`: the template's own.
    #[serde(default)]
    pub title: Option<String>,
    /// Why the data could not be serialized; the render fails with
    /// [`DocumentsError::Data`](crate::DocumentsError::Data).
    #[serde(skip)]
    pub(crate) data_error: Option<String>,
}

impl Render {
    fn new(source: TemplateSource) -> Self {
        Self {
            source,
            data: Value::Null,
            locale: None,
            engine: None,
            print: None,
            title: None,
            data_error: None,
        }
    }

    /// The template `name` (`certificates/classic`), without or with its
    /// extension.
    pub fn template(name: impl Into<String>) -> Self {
        Self::new(TemplateSource::Template(name.into()))
    }

    /// Typst markup given inline. Imports and images resolve against the
    /// template directories.
    pub fn typst(source: impl Into<String>) -> Self {
        Self::new(TemplateSource::Typst(source.into()))
    }

    /// HTML given inline, printed by Chrome.
    pub fn html(html: impl Into<String>) -> Self {
        Self::new(TemplateSource::Html(html.into())).engine(EngineKind::Chrome)
    }

    /// The data the template reads (anything `Serialize`).
    pub fn data(mut self, data: impl Serialize) -> Self {
        match serde_json::to_value(data) {
            Ok(value) => {
                self.data = value;
                self.data_error = None;
            }
            Err(error) => self.data_error = Some(error.to_string()),
        }
        self
    }

    /// The document's language (`ar`, `hi`, `en-GB`).
    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = Some(locale.into());
        self
    }

    /// Renders with `engine` instead of the one the template picks.
    pub fn engine(mut self, engine: EngineKind) -> Self {
        self.engine = Some(engine);
        self
    }

    /// Page settings for Chrome.
    pub fn print(mut self, print: PrintOptions) -> Self {
        self.print = Some(print);
        self
    }

    /// The PDF's title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The template's name, or `inline` for inline markup: what errors and
    /// logs name.
    pub fn name(&self) -> &str {
        match &self.source {
            TemplateSource::Template(name) => name,
            TemplateSource::Typst(_) | TemplateSource::Html(_) => "inline",
        }
    }

    /// The text direction of the locale: `rtl` for Arabic, Hebrew,
    /// Persian, Urdu and the other right-to-left scripts, `ltr` otherwise.
    pub fn direction(&self) -> &'static str {
        match &self.locale {
            Some(locale) if is_rtl(locale) => "rtl",
            _ => "ltr",
        }
    }
}

/// Whether `locale` is written right to left (by its language subtag).
pub fn is_rtl(locale: &str) -> bool {
    let language = locale
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        language.as_str(),
        "ar" | "arc"
            | "ckb"
            | "dv"
            | "fa"
            | "he"
            | "iw"
            | "ks"
            | "ku"
            | "ps"
            | "sd"
            | "ug"
            | "ur"
            | "yi"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn right_to_left_locales() {
        for locale in ["ar", "ar-EG", "he", "fa_IR", "ur", "UR-pk", "yi", "ckb"] {
            assert!(is_rtl(locale), "{locale}");
        }
        for locale in ["en", "hi", "pt-BR", "", "arn"] {
            assert!(!is_rtl(locale), "{locale}");
        }
        assert_eq!(Render::template("x").locale("ar").direction(), "rtl");
        assert_eq!(Render::template("x").direction(), "ltr");
    }

    #[test]
    fn a_render_round_trips_through_json() {
        let render = Render::template("certificates/classic")
            .data(json!({"name": "Ada"}))
            .locale("hi")
            .engine(EngineKind::Typst)
            .title("Certificate");
        let json = serde_json::to_value(&render).unwrap();
        assert_eq!(
            json["source"],
            json!({"kind": "template", "value": "certificates/classic"})
        );
        assert_eq!(json["engine"], "typst");
        let back: Render = serde_json::from_value(json).unwrap();
        assert_eq!(back, render);
        assert_eq!(back.name(), "certificates/classic");
        assert_eq!(Render::html("<p>").name(), "inline");
        assert_eq!(Render::html("<p>").engine, Some(EngineKind::Chrome));
    }

    #[test]
    fn data_that_cannot_be_serialized_is_kept_as_an_error() {
        use std::collections::BTreeMap;
        let mut map = BTreeMap::new();
        map.insert(vec![1u8], 1);
        let render = Render::template("x").data(map);
        assert!(render.data_error.is_some());
        assert_eq!(render.data, Value::Null);
    }
}
