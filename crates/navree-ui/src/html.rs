//! The one escaping path, and the two types that may reach HTML without it:
//! [`Href`] (a URL whose scheme was checked) and [`SafeHtml`] (markup you
//! vouch for).
//!
//! Everything else that reaches the output goes through [`escape_into`]:
//! `& < > " '` become entities. `/` is not escaped. The renderer always
//! double-quotes attributes and writes no script or style text, so `/` can't
//! end a context, and escaping it would add 5 bytes per slash to every URL.

use std::error::Error;
use std::fmt;

/// Which bytes [`escape_into`] replaces.
const SPECIAL: [bool; 256] = {
    let mut table = [false; 256];
    table[b'&' as usize] = true;
    table[b'<' as usize] = true;
    table[b'>' as usize] = true;
    table[b'"' as usize] = true;
    table[b'\'' as usize] = true;
    table
};

/// Appends `text` to `out`, escaped for HTML text and double-quoted attributes.
///
/// ```
/// let mut out = String::new();
/// navree_ui::html::escape_into(&mut out, "a<b & \"c\"");
/// assert_eq!(out, "a&lt;b &amp; &quot;c&quot;");
/// ```
#[inline]
pub fn escape_into(out: &mut String, text: &str) {
    let bytes = text.as_bytes();
    let mut start = 0;
    for (at, byte) in bytes.iter().enumerate() {
        if !SPECIAL[*byte as usize] {
            continue;
        }
        out.push_str(&text[start..at]);
        out.push_str(match byte {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            _ => "&#39;",
        });
        start = at + 1;
    }
    out.push_str(&text[start..]);
}

/// Markup that is written as it is. Only code that produced or sanitized the
/// markup itself should build one: nothing from a user belongs here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeHtml(pub(crate) Box<str>);

impl SafeHtml {
    /// Markup you vouch for.
    pub fn trusted(markup: impl Into<Box<str>>) -> Self {
        Self(markup.into())
    }

    /// The markup.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a URL can't be a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HrefError {
    /// A control character, or space at the start: browsers strip those
    /// before reading the scheme, so `java\tscript:` would still run.
    Control,
    /// A scheme other than `http`, `https`, `mailto` or `tel`.
    Scheme,
}

impl fmt::Display for HrefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Control => "a URL can't hold control characters or start with a space",
            Self::Scheme => "a URL's scheme must be http, https, mailto or tel",
        })
    }
}

impl Error for HrefError {}

/// A URL that is safe in `href`, `src`, `action` and `formaction`: a relative
/// reference, or `http`, `https`, `mailto` or `tel`.
///
/// [`Href::parse`] says why a URL is refused. `From<&str>` and `From<String>`
/// are for builders: a refused URL becomes `#`, which goes nowhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Href(Box<str>);

impl Href {
    /// Checks `url`.
    ///
    /// ```
    /// use navree_ui::html::Href;
    ///
    /// assert!(Href::parse("/deals?stage=won").is_ok());
    /// assert!(Href::parse("https://navree.com").is_ok());
    /// assert!(Href::parse("JavaScript:alert(1)").is_err());
    /// assert!(Href::parse("java\tscript:alert(1)").is_err());
    /// ```
    pub fn parse(url: &str) -> Result<Self, HrefError> {
        if url.starts_with(' ') || url.bytes().any(|byte| byte < 0x20 || byte == 0x7f) {
            return Err(HrefError::Control);
        }
        let scheme_end = url.find([':', '/', '?', '#']);
        if let Some(end) = scheme_end
            && url.as_bytes()[end] == b':'
        {
            let scheme = &url[..end];
            let allowed = ["http", "https", "mailto", "tel"];
            if !allowed.iter().any(|name| name.eq_ignore_ascii_case(scheme)) {
                return Err(HrefError::Scheme);
            }
        }
        Ok(Self(url.into()))
    }

    /// The URL.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn into_inner(self) -> Box<str> {
        self.0
    }
}

impl From<&str> for Href {
    fn from(url: &str) -> Self {
        Self::parse(url).unwrap_or_else(|_| Self("#".into()))
    }
}

impl From<String> for Href {
    fn from(url: String) -> Self {
        Self::parse(&url).unwrap_or_else(|_| Self("#".into()))
    }
}

impl From<&String> for Href {
    fn from(url: &String) -> Self {
        Self::from(url.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escaped(text: &str) -> String {
        let mut out = String::new();
        escape_into(&mut out, text);
        out
    }

    #[test]
    fn every_special_byte_is_escaped() {
        assert_eq!(escaped("<&>\"'"), "&lt;&amp;&gt;&quot;&#39;");
        assert_eq!(escaped("plain"), "plain");
        assert_eq!(escaped(""), "");
        assert_eq!(escaped("/a/b"), "/a/b");
        assert_eq!(escaped("héllo <wörld>"), "héllo &lt;wörld&gt;");
    }

    #[test]
    fn scripts_cannot_be_links() {
        for url in [
            "javascript:alert(1)",
            "JAVASCRIPT:alert(1)",
            "vbscript:x",
            "data:text/html,<script>alert(1)</script>",
            "file:///etc/passwd",
            "blob:x",
            "unknown:x",
        ] {
            assert_eq!(Href::parse(url), Err(HrefError::Scheme), "{url}");
        }
        for url in ["java\tscript:alert(1)", "java\nscript:x", " javascript:x", "\u{1}javascript:x"] {
            assert_eq!(Href::parse(url), Err(HrefError::Control), "{url:?}");
        }
    }

    #[test]
    fn ordinary_urls_are_links() {
        for url in [
            "",
            "/",
            "/deals/1?x=a:b#c:d",
            "./a",
            "../a",
            "?page=2",
            "#top",
            "deals/1",
            "http://a.b",
            "HTTPS://a.b",
            "mailto:a@b.c",
            "tel:+1",
            "/a:b",
        ] {
            assert!(Href::parse(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn builders_turn_a_refused_url_into_nowhere() {
        assert_eq!(Href::from("javascript:x").as_str(), "#");
        assert_eq!(Href::from("/ok".to_owned()).as_str(), "/ok");
    }
}
