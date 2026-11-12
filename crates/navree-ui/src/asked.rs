//! Who asked: the one answer every place that chooses the form of a reply
//! goes by.
//!
//! A request comes from a browser following a link or posting a form itself,
//! from the toolkit's browser runtime, which says so with `X-Navree-UI`, or,
//! with the `native-preview` feature, from a native client (`X-Navree-Native`)
//! or the web view inside one. A web view sends a custom header on its first
//! request only, so it is known by the last word of its user agent,
//! `NavreeNative/<version>`, which it sends on every request.

use http::HeaderMap;

use crate::op::UI_HEADER;

/// The word a native client's web view ends its user agent with, before a `/`
/// and the client library's version: `... Mobile Safari/537.36 NavreeNative/0.1.0`.
#[cfg(feature = "native-preview")]
pub const WEB_VIEW_TOKEN: &str = "NavreeNative/";

/// Who a request came from, as the toolkit answers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(not(feature = "native-preview"), allow(dead_code))]
pub enum WhoAsked {
    /// A browser that follows links and posts forms itself: answered with
    /// whole pages, and a redirect after a post.
    Browser,
    /// The toolkit's browser runtime, in a browser or in a native client's web
    /// view: answered with ops where a reply has them.
    Runtime,
    /// A native client: answered with the envelope of the native protocol.
    Native,
    /// The web view inside a native client, navigating by itself: answered as
    /// a browser is, without the app's chrome, which the client draws.
    WebView,
}

#[cfg_attr(not(feature = "native-preview"), allow(dead_code))]
impl WhoAsked {
    /// Whether the answer is a native envelope.
    pub fn is_native(self) -> bool {
        self == Self::Native
    }

    /// Whether the answer is ops for the browser runtime.
    pub fn is_runtime(self) -> bool {
        self == Self::Runtime
    }

    /// Who asked for the request the current task answers, as far as the
    /// reply layer made it known: a native client, the web view inside one,
    /// or `None` for the web, which is a browser or its runtime (read the
    /// request's headers with [`who_asked`] to tell those apart). One read of
    /// a task-local value, and none without the `native-preview` feature.
    #[inline]
    pub fn now() -> Option<Self> {
        #[cfg(feature = "native-preview")]
        {
            crate::native::scope::asked().map(|asked| match asked {
                crate::native::scope::Asked::Native(_) => Self::Native,
                crate::native::scope::Asked::Embed => Self::WebView,
            })
        }
        #[cfg(not(feature = "native-preview"))]
        {
            None
        }
    }
}

/// Who sent a request with these headers. A browser's request costs a few
/// header lookups and no allocation.
///
/// ```
/// use http::{HeaderMap, HeaderValue};
/// use navree_ui::native::{WhoAsked, who_asked};
///
/// let mut headers = HeaderMap::new();
/// assert_eq!(who_asked(&headers), WhoAsked::Browser);
/// headers.insert("x-navree-ui", HeaderValue::from_static("1"));
/// assert_eq!(who_asked(&headers), WhoAsked::Runtime);
/// headers.insert("x-navree-native", HeaderValue::from_static("1"));
/// assert_eq!(who_asked(&headers), WhoAsked::Native);
/// ```
#[inline]
pub fn who_asked(headers: &HeaderMap) -> WhoAsked {
    #[cfg(feature = "native-preview")]
    if headers.contains_key(navree_native::NATIVE_HEADER) {
        return WhoAsked::Native;
    }
    if headers.contains_key(UI_HEADER) {
        return WhoAsked::Runtime;
    }
    #[cfg(feature = "native-preview")]
    if in_web_view(headers) {
        return WhoAsked::WebView;
    }
    WhoAsked::Browser
}

/// Whether the request came from a native client's web view: the last word of
/// its user agent is [`WEB_VIEW_TOKEN`] and a version, or, on the first load,
/// it says `X-Navree-Embed`. Only the end of the user agent is read.
#[cfg(feature = "native-preview")]
#[inline]
pub(crate) fn in_web_view(headers: &HeaderMap) -> bool {
    if headers.contains_key(navree_native::EMBED_HEADER) {
        return true;
    }
    headers
        .get(http::header::USER_AGENT)
        .is_some_and(|agent| last_word(agent.as_bytes()).starts_with(WEB_VIEW_TOKEN.as_bytes()))
}

/// The bytes after the last space of `agent`, trailing spaces left out.
#[cfg(feature = "native-preview")]
fn last_word(agent: &[u8]) -> &[u8] {
    let mut end = agent.len();
    while end > 0 && agent[end - 1] == b' ' {
        end -= 1;
    }
    let trimmed = &agent[..end];
    match trimmed.iter().rposition(|byte| *byte == b' ') {
        Some(space) => &trimmed[space + 1..],
        None => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_static(value));
        }
        headers
    }

    #[test]
    fn a_browser_is_a_browser() {
        assert_eq!(who_asked(&HeaderMap::new()), WhoAsked::Browser);
        assert_eq!(
            who_asked(&headers(&[(
                "user-agent",
                "Mozilla/5.0 (Linux; Android 16) Chrome/140 Mobile Safari/537.36"
            )])),
            WhoAsked::Browser
        );
        assert_eq!(WhoAsked::now(), None);
    }

    #[test]
    fn the_runtime_says_so_itself() {
        assert_eq!(
            who_asked(&headers(&[("x-navree-ui", "1")])),
            WhoAsked::Runtime
        );
    }

    #[cfg(feature = "native-preview")]
    #[test]
    fn a_native_client_says_so_before_anything_else() {
        assert_eq!(
            who_asked(&headers(&[("x-navree-native", "1"), ("x-navree-ui", "1")])),
            WhoAsked::Native
        );
        assert!(who_asked(&headers(&[("x-navree-native", "1")])).is_native());
    }

    #[cfg(feature = "native-preview")]
    #[test]
    fn a_web_view_is_known_by_the_last_word_of_its_user_agent() {
        let agent = "Mozilla/5.0 (iPhone; CPU iPhone OS 26_1 like Mac OS X) AppleWebKit/605.1.15 NavreeNative/0.1.0";
        assert_eq!(
            who_asked(&headers(&[("user-agent", agent)])),
            WhoAsked::WebView
        );
        // Its runtime's own requests are the runtime's.
        assert_eq!(
            who_asked(&headers(&[("user-agent", agent), ("x-navree-ui", "1")])),
            WhoAsked::Runtime
        );
        // The first load may say so with a header instead.
        assert_eq!(
            who_asked(&headers(&[("x-navree-embed", "1")])),
            WhoAsked::WebView
        );
        // The word anywhere but at the end is a browser's own words.
        assert_eq!(
            who_asked(&headers(&[(
                "user-agent",
                "NavreeNative/0.1.0 (a crawler) Mozilla/5.0"
            )])),
            WhoAsked::Browser
        );
        assert_eq!(last_word(b"a b  "), b"b");
        assert_eq!(last_word(b"single"), b"single");
        assert_eq!(last_word(b""), b"");
    }
}
