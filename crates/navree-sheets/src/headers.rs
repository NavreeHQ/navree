//! Header translation: [`HeaderTranslator`] and the built-in
//! [`HeaderCatalog`].

use std::collections::HashMap;
use std::sync::Arc;

use crate::Column;

/// Translates export headers. A column's `header` is looked up as a key;
/// when the translator has no text for it, the header is written as is.
///
/// Register one with `m.header_translator::<T>()`
/// ([`ModuleBuilderSheetsExt`](crate::ModuleBuilderSheetsExt)); the app's
/// localization module can provide it, or use a [`HeaderCatalog`].
pub trait HeaderTranslator: Send + Sync + 'static {
    /// The text of `key` in `locale` (`None`: the translator's default
    /// locale), or `None` when it has none.
    fn translate(&self, key: &str, locale: Option<&str>) -> Option<String>;
}

/// Header texts by locale, kept in memory: a [`HeaderTranslator`] for apps
/// that keep their export headers in code.
///
/// ```
/// use navree_sheets::{HeaderCatalog, HeaderTranslator};
///
/// let catalog = HeaderCatalog::new("en")
///     .locale("en", [("contacts.name", "Name")])
///     .locale("ar", [("contacts.name", "الاسم")]);
/// assert_eq!(catalog.translate("contacts.name", Some("ar")).as_deref(), Some("الاسم"));
/// assert_eq!(catalog.translate("contacts.name", None).as_deref(), Some("Name"));
/// // An unknown locale falls back to the default one.
/// assert_eq!(catalog.translate("contacts.name", Some("fr")).as_deref(), Some("Name"));
/// assert_eq!(catalog.translate("contacts.email", Some("ar")), None);
/// ```
#[derive(Clone, Debug, Default)]
pub struct HeaderCatalog {
    default: String,
    locales: HashMap<String, HashMap<String, String>>,
}

impl HeaderCatalog {
    /// An empty catalog whose default locale is `default`.
    pub fn new(default: impl Into<String>) -> Self {
        HeaderCatalog {
            default: default.into(),
            locales: HashMap::new(),
        }
    }

    /// Adds the texts of `locale`.
    pub fn locale<K: Into<String>, V: Into<String>>(
        mut self,
        locale: impl Into<String>,
        texts: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        let entry = self.locales.entry(normalize(&locale.into())).or_default();
        for (key, text) in texts {
            entry.insert(key.into(), text.into());
        }
        self
    }

    fn lookup(&self, locale: &str, key: &str) -> Option<String> {
        let locale = normalize(locale);
        if let Some(text) = self.locales.get(&locale).and_then(|t| t.get(key)) {
            return Some(text.clone());
        }
        // `ar-EG` falls back to `ar`.
        let language = locale.split('-').next().unwrap_or_default();
        self.locales
            .get(language)
            .and_then(|t| t.get(key))
            .cloned()
    }
}

impl HeaderTranslator for HeaderCatalog {
    fn translate(&self, key: &str, locale: Option<&str>) -> Option<String> {
        locale
            .and_then(|locale| self.lookup(locale, key))
            .or_else(|| self.lookup(&self.default, key))
    }
}

fn normalize(locale: &str) -> String {
    locale.trim().replace('_', "-").to_ascii_lowercase()
}

/// The header texts of `columns`, translated where `translator` knows them.
pub(crate) fn resolve_headers(
    columns: &[Column],
    translator: Option<&Arc<dyn HeaderTranslator>>,
    locale: Option<&str>,
) -> Vec<String> {
    columns
        .iter()
        .map(|column| {
            translator
                .and_then(|t| t.translate(&column.header, locale))
                .unwrap_or_else(|| column.header.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_are_translated_or_kept() {
        let catalog: Arc<dyn HeaderTranslator> = Arc::new(
            HeaderCatalog::new("en").locale("ar_EG", [("contacts.name", "الاسم")]),
        );
        let columns = [
            Column::new("name", "contacts.name"),
            Column::new("email", "Email"),
        ];
        assert_eq!(
            resolve_headers(&columns, Some(&catalog), Some("ar-EG")),
            ["الاسم", "Email"]
        );
        assert_eq!(
            resolve_headers(&columns, None, Some("ar")),
            ["contacts.name", "Email"]
        );
    }

    #[test]
    fn a_region_falls_back_to_its_language() {
        let catalog = HeaderCatalog::new("en").locale("ar", [("k", "v")]);
        assert_eq!(catalog.translate("k", Some("ar-SA")).as_deref(), Some("v"));
    }
}
