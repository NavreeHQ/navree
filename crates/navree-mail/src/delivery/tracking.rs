//! Open and click tracking: links rewritten through a signed redirect and
//! an open image, both naming the delivery.

use std::sync::Arc;

use chrono::Duration;
use navree_contracts::Clock;
use navree_http::UrlSigner;
use uuid::Uuid;

/// The absolute path of the open image: `/mail/t/o/{delivery}`.
pub(crate) const OPEN_PATH: &str = "/mail/t/o";
/// The absolute path of the click redirect: `/mail/t/c/{delivery}?u=..`.
pub(crate) const CLICK_PATH: &str = "/mail/t/c";

/// Rewrites HTML bodies for tracking.
pub(crate) struct Tracking {
    signer: Arc<UrlSigner>,
    clock: Arc<dyn Clock>,
    base: String,
    ttl: Duration,
    opens: bool,
    clicks: bool,
}

impl Tracking {
    pub(crate) fn new(
        signer: Arc<UrlSigner>,
        clock: Arc<dyn Clock>,
        base: &str,
        ttl_days: u32,
        opens: bool,
        clicks: bool,
    ) -> Self {
        Self {
            signer,
            clock,
            base: base.trim_end_matches('/').to_owned(),
            ttl: Duration::days(i64::from(ttl_days.max(1))),
            opens,
            clicks,
        }
    }

    /// `html` with its `http(s)` links sent through the click redirect and
    /// the open image added before `</body>` (or at the end).
    pub(crate) fn rewrite(&self, html: &str, delivery_id: Uuid) -> String {
        let mut out = if self.clicks {
            rewrite_links(html, |url| self.click_url(delivery_id, url))
        } else {
            html.to_owned()
        };
        if self.opens {
            let image = format!(
                "<img src=\"{}\" width=\"1\" height=\"1\" alt=\"\" style=\"display:none;border:0\">",
                escape_attribute(&self.open_url(delivery_id))
            );
            match out.to_ascii_lowercase().rfind("</body>") {
                Some(at) => out.insert_str(at, &image),
                None => out.push_str(&image),
            }
        }
        out
    }

    /// The signed open-image URL.
    pub(crate) fn open_url(&self, delivery_id: Uuid) -> String {
        let path = format!("{OPEN_PATH}/{}", delivery_id.simple());
        format!("{}{}", self.base, self.signer.sign(&path, self.expires()))
    }

    /// The signed click URL for `target`.
    pub(crate) fn click_url(&self, delivery_id: Uuid, target: &str) -> String {
        let encoded: String = url::form_urlencoded::byte_serialize(target.as_bytes()).collect();
        let path = format!("{CLICK_PATH}/{}?u={encoded}", delivery_id.simple());
        format!("{}{}", self.base, self.signer.sign(&path, self.expires()))
    }

    fn expires(&self) -> chrono::DateTime<chrono::Utc> {
        self.clock.now() + self.ttl
    }
}

/// `html` with the `href` of every `<a>` tag whose link is `http(s)` passed
/// through `track`; tags with a `data-no-track` attribute are left alone.
pub(crate) fn rewrite_links(html: &str, track: impl Fn(&str) -> String) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() + 256);
    let mut cursor = 0;
    while let Some(found) = lower[cursor..].find("<a") {
        let start = cursor + found;
        let after = start + 2;
        let is_anchor = lower[after..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace);
        let Some(end) = lower[after..].find('>').map(|i| after + i) else {
            break;
        };
        out.push_str(&html[cursor..start]);
        let tag = &html[start..end];
        let tag_lower = &lower[start..end];
        if is_anchor && !tag_lower.contains("data-no-track") {
            out.push_str(&rewrite_href(tag, tag_lower, &track));
        } else {
            out.push_str(tag);
        }
        cursor = end;
    }
    out.push_str(&html[cursor..]);
    out
}

/// One `<a ...` tag (without its `>`) with its `href` rewritten.
fn rewrite_href(tag: &str, tag_lower: &str, track: &impl Fn(&str) -> String) -> String {
    let mut search = 0;
    while let Some(found) = tag_lower[search..].find("href") {
        let name_start = search + found;
        let preceded = tag_lower[..name_start]
            .chars()
            .last()
            .is_some_and(char::is_whitespace);
        let mut i = name_start + 4;
        let bytes = tag.as_bytes();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if !preceded || i >= bytes.len() || bytes[i] != b'=' {
            search = name_start + 4;
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let Some(&quote) = bytes.get(i) else {
            return tag.to_owned();
        };
        if quote != b'"' && quote != b'\'' {
            return tag.to_owned();
        }
        let value_start = i + 1;
        let Some(value_len) = tag[value_start..].find(quote as char) else {
            return tag.to_owned();
        };
        let value = &tag[value_start..value_start + value_len];
        let url = unescape(value);
        let lower_url = url.to_ascii_lowercase();
        if !(lower_url.starts_with("http://") || lower_url.starts_with("https://")) {
            return tag.to_owned();
        }
        return format!(
            "{}{}{}",
            &tag[..value_start],
            escape_attribute(&track(&url)),
            &tag[value_start + value_len..]
        );
    }
    tag.to_owned()
}

/// The entities a rendered `href` may hold.
fn unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_rewritten_and_others_kept() {
        let html = r##"<p><a href="https://acme.test/a?x=1&amp;y=2">A</a> <A HREF='http://acme.test/b'>B</A>
            <a href="mailto:ada@example.com">mail</a> <a href="#top">top</a>
            <a data-no-track href="https://acme.test/c">C</a> <abbr>no</abbr> <a class="x">none</a></p>"##;
        let out = rewrite_links(html, |url| format!("T({url})"));
        assert!(out.contains(r#"<a href="T(https://acme.test/a?x=1&amp;y=2)">A</a>"#), "{out}");
        assert!(out.contains("<A HREF='T(http://acme.test/b)'>B</A>"), "{out}");
        assert!(out.contains(r#"<a href="mailto:ada@example.com">"#), "{out}");
        assert!(out.contains(r##"<a href="#top">"##), "{out}");
        assert!(out.contains(r#"<a data-no-track href="https://acme.test/c">"#), "{out}");
        assert!(out.contains("<abbr>no</abbr>"), "{out}");
        assert!(out.contains(r#"<a class="x">none</a>"#), "{out}");
    }

    #[test]
    fn an_attribute_named_like_href_is_not_rewritten() {
        let out = rewrite_links(r#"<a data-href="https://x.test" href="https://y.test">"#, |url| {
            format!("T({url})")
        });
        assert_eq!(out, r#"<a data-href="https://x.test" href="T(https://y.test)">"#);
    }
}
