//! The one escaping path, and the two types that may reach HTML without it:
//! [`Href`] (a URL whose scheme was checked) and [`SafeHtml`] (markup you
//! vouch for). [`LocalHref`] is an [`Href`] that stays on this site, for redirects.
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
#[non_exhaustive]
pub enum HrefError {
    /// A control character anywhere, or whitespace or an invisible format character (a
    /// zero-width space, a direction mark, a byte order mark) at the start: browsers
    /// strip some of those before reading the scheme, so `java\tscript:` would still
    /// run, and other clients trim the rest, so what follows would be read as a scheme
    /// or a host.
    Control,
    /// A scheme other than `http`, `https`, `mailto` or `tel`.
    Scheme,
    /// A URL that leaves this site, where only a [`LocalHref`] will do.
    OffSite,
}

impl fmt::Display for HrefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Control => {
                "a URL can't hold control characters or start with whitespace or an invisible character"
            }
            Self::Scheme => "a URL's scheme must be http, https, mailto or tel",
            Self::OffSite => "this URL must stay on this site: a path, a query or a fragment",
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
    /// // Whitespace or a line break at the start, in any script: a client that trims it
    /// // would read what follows as another site.
    /// assert!(Href::parse("\u{3000}//evil.example/x").is_err());
    /// assert!(Href::parse(" /deals").is_err());
    /// // So is an invisible format character there, which some clients trim too.
    /// assert!(Href::parse("\u{200b}//evil.example/x").is_err());
    /// // A scheme is one of the four as written, so nothing hides in one.
    /// assert!(Href::parse("https\u{a0}://evil.example").is_err());
    /// assert!(Href::parse("java\u{ad}script:alert(1)").is_err());
    /// // After the start, text is kept as written: a joiner of a word, a space at the end.
    /// assert!(Href::parse("tel:+91 98140 12345").is_ok());
    /// assert!(Href::parse("/ਗਾਹਕ?ਸ਼ਹਿਰ=ਲੁਧਿਆਣਾ").is_ok());
    /// assert!(Href::parse("/खोज?q=क्\u{200d}").is_ok());
    /// ```
    pub fn parse(url: &str) -> Result<Self, HrefError> {
        // Browsers strip a space or a control from the start of an address; a native client
        // trims every kind of whitespace there, and some trimmers drop the invisible format
        // characters too. What follows one would then be read as a scheme or a host that was
        // not checked here. Nothing trimmed from the end changes where an address leads.
        let trimmed = |c: char| c.is_whitespace() || c.is_control() || invisible(c);
        if url.starts_with(trimmed) || url.bytes().any(|byte| byte < 0x20 || byte == 0x7f) {
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

/// The invisible format characters some clients trim from the start of an address: the soft
/// hyphen, the Arabic letter mark, the Mongolian vowel separator, the zero-width space,
/// joiners and direction marks, the direction embeddings, overrides and isolates, the word
/// joiner and the invisible operators, and the byte order mark. No address needs one as its
/// first character; inside a word (a joiner in Devanagari or Persian text) they are text.
fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{ad}'
            | '\u{61c}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
    )
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

/// A URL on this site: a path, a query or a fragment. It has no scheme and no
/// host, and browsers can't read one into it: `//host`, `/\\host` and `\\\\host` all
/// name another site, so they are refused.
///
/// It is the type for addresses your own code writes: a link in a builder, a redirect
/// to a page of the app. It also accepts relative paths and dot segments, which a URL
/// taken from a request must not have, so for a redirect to a URL that comes from a
/// request (`?next=`) use [`ReturnTo`](crate::pages::ReturnTo), which takes only one
/// absolute path of this site. `From<&str>` and `From<String>` are for builders: a
/// refused URL becomes `#`, which stays on the page.
///
/// ```
/// use navree_ui::html::LocalHref;
///
/// assert!(LocalHref::parse("/deals/1?tab=notes").is_ok());
/// assert!(LocalHref::parse("//evil.example").is_err());
/// assert!(LocalHref::parse("/\\evil.example").is_err());
/// assert!(LocalHref::parse("https://evil.example").is_err());
/// // A client that trims whitespace would read another site here.
/// assert!(LocalHref::parse("\u{3000}//evil.example/x").is_err());
/// assert!(LocalHref::parse("\u{2029}/\\evil.example").is_err());
/// assert!(LocalHref::parse("\u{feff}//evil.example/x").is_err());
/// assert!(LocalHref::parse("\u{2066}/deals").is_err());
/// assert!(LocalHref::parse("ਰਿਪੋਰਟ?ਮਹੀਨਾ=ਕੱਤਕ").is_ok());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalHref(Href);

impl LocalHref {
    /// Checks `url`.
    pub fn parse(url: &str) -> Result<Self, HrefError> {
        let href = Href::parse(url)?;
        let first = url.as_bytes().first().copied();
        let second = url.as_bytes().get(1).copied();
        let slash = |byte: Option<u8>| matches!(byte, Some(b'/' | b'\\'));
        // A browser reads a backslash as a slash in a URL of this kind.
        let has_host = first == Some(b'\\') || (slash(first) && slash(second));
        let has_scheme = url
            .find([':', '/', '?', '#'])
            .is_some_and(|end| url.as_bytes()[end] == b':');
        if has_host || has_scheme {
            return Err(HrefError::OffSite);
        }
        Ok(Self(href))
    }

    /// The URL.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub(crate) fn into_href(self) -> Href {
        self.0
    }
}

impl From<&str> for LocalHref {
    fn from(url: &str) -> Self {
        Self::parse(url).unwrap_or_else(|_| Self(Href("#".into())))
    }
}

impl From<String> for LocalHref {
    fn from(url: String) -> Self {
        Self::from(url.as_str())
    }
}

impl From<&String> for LocalHref {
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
        for url in [
            "java\tscript:alert(1)",
            "java\nscript:x",
            " javascript:x",
            "\u{1}javascript:x",
        ] {
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
    fn a_local_url_has_no_host_and_no_scheme() {
        for url in [
            "//a.b",
            "/\\a.b",
            "\\\\a.b",
            "\\/a.b",
            "\\a.b",
            "///a.b",
            "http://a.b",
            "HTTPS://a.b",
            "https:\\\\a.b",
            "http:a.b",
            "mailto:a@b.c",
            "tel:+1",
        ] {
            assert_eq!(LocalHref::parse(url), Err(HrefError::OffSite), "{url}");
        }
        for url in ["javascript:x", "data:x", "blob:x"] {
            assert_eq!(LocalHref::parse(url), Err(HrefError::Scheme), "{url}");
        }
        assert_eq!(LocalHref::parse("/a\nb"), Err(HrefError::Control));
        for url in [
            "",
            "/",
            "/deals/1?x=a:b#c:d",
            "?page=2",
            "#top",
            "deals/1",
            "../up",
            "/a//b",
            "/a\\b",
        ] {
            assert!(LocalHref::parse(url).is_ok(), "{url}");
        }
        assert_eq!(LocalHref::from("//a.b").as_str(), "#");
    }

    /// A native client trims every kind of whitespace before it resolves an address; a browser
    /// doesn't. An address that starts with a character one of them trims is no link, so what
    /// follows is never a host to one and a path to the other. After the start an address is
    /// kept as it is written: a joiner inside or at the end of a word, a space at the end.
    #[test]
    fn whitespace_a_client_would_trim_from_the_start_is_no_link() {
        for url in [
            "\u{3000}//evil.example/x",
            "\u{a0}//evil.example",
            "\u{85}/\\evil.example",
            "\u{2028}https://evil.example",
            "\u{2029}/deals",
            "\u{9f}/deals",
            " /deals",
        ] {
            assert_eq!(Href::parse(url), Err(HrefError::Control), "{url:?}");
            assert_eq!(LocalHref::parse(url), Err(HrefError::Control), "{url:?}");
            assert_eq!(LocalHref::from(url).as_str(), "#", "{url:?}");
        }
        // A scheme is one of the four as written, so nothing a client drops hides in one.
        for url in ["https\u{a0}://evil.example", "http\u{2028}s://evil.example"] {
            assert_eq!(Href::parse(url), Err(HrefError::Scheme), "{url:?}");
            assert_eq!(LocalHref::from(url).as_str(), "#", "{url:?}");
        }
        let invisible = [
            '\u{ad}', '\u{61c}', '\u{180e}', '\u{200b}', '\u{200c}', '\u{200d}', '\u{200e}',
            '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2060}',
            '\u{2061}', '\u{2062}', '\u{2063}', '\u{2064}', '\u{2066}', '\u{2067}', '\u{2068}',
            '\u{2069}', '\u{feff}',
        ];
        for c in invisible {
            // As the first character.
            for url in [
                format!("{c}//evil.example/x"),
                format!("{c}/\\evil.example"),
                format!("{c}/deals"),
                format!("{c}https://evil.example"),
            ] {
                assert_eq!(Href::parse(&url), Err(HrefError::Control), "{url:?}");
                assert_eq!(LocalHref::parse(&url), Err(HrefError::Control), "{url:?}");
                assert_eq!(LocalHref::from(url.as_str()).as_str(), "#", "{url:?}");
            }
            // In a scheme.
            for url in [
                format!("https{c}://evil.example"),
                format!("java{c}script:alert(1)"),
            ] {
                assert_eq!(Href::parse(&url), Err(HrefError::Scheme), "{url:?}");
                assert_eq!(LocalHref::from(url.as_str()).as_str(), "#", "{url:?}");
            }
            // Anywhere after the start it is text, and stays.
            for url in [
                format!("/deals/{c}x"),
                format!("/search?q=a{c}b"),
                format!("#a{c}b"),
                format!("/deals{c}"),
                format!("deals{c}list?x=1"),
                format!("#top{c}"),
                format!("/\u{916}\u{94b}\u{91c}?q=\u{915}\u{94d}{c}"),
            ] {
                assert!(Href::parse(&url).is_ok(), "{url:?}");
                assert!(LocalHref::parse(&url).is_ok(), "{url:?}");
            }
        }
        // So do whitespace after the start, a space at the end, and text in any script.
        for url in [
            "/deals/\u{3000}x",
            "/search?q=a\u{a0}b",
            "#\u{2028}x",
            "/deals\u{3000}",
            "/deals\u{2029}",
            "/deals ",
            "deals\u{3000}list?x=1",
            "tel:+91 98140 12345",
            "/ਪੰਜਾਬ/ਲੁਧਿਆਣਾ",
            "ਰਿਪੋਰਟ?x=1",
            "/ਗਾਹਕ?ਸ਼ਹਿਰ=ਲੁਧਿਆਣਾ#ਸੂਚੀ",
            "?ਨਾਮ=ਗੁਰਪ੍ਰੀਤ",
        ] {
            assert!(Href::parse(url).is_ok(), "{url:?}");
        }
    }

    #[test]
    fn builders_turn_a_refused_url_into_nowhere() {
        assert_eq!(Href::from("javascript:x").as_str(), "#");
        assert_eq!(Href::from("/ok".to_owned()).as_str(), "/ok");
    }
}
