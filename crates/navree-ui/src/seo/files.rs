//! The two files a public site serves beside its pages: `sitemap.xml` and `robots.txt`.

use std::fmt::Display;

use http::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderValue};
use navree_http::response::{IntoResponse, Response};

use crate::html::escape_into;

/// How long a sitemap or robots file may be kept, by a browser or a shared cache.
const FILE_CACHE: &str = "public, max-age=3600";

/// The crawlers of AI search and assistants that [`RobotsTxt::welcome`] names, so that
/// anyone reading the file sees they are welcome:
///
/// - search and answers: OAI-SearchBot, Claude-SearchBot, PerplexityBot;
/// - fetches a person asked for: ChatGPT-User, Claude-User, Perplexity-User;
/// - training and indexing: GPTBot, ClaudeBot, CCBot, and the Google-Extended and
///   Applebot-Extended tokens, which only say whether Google and Apple may use pages for
///   their AI models.
pub const AI_CRAWLERS: &[&str] = &[
    "GPTBot",
    "OAI-SearchBot",
    "ChatGPT-User",
    "ClaudeBot",
    "Claude-SearchBot",
    "Claude-User",
    "PerplexityBot",
    "Perplexity-User",
    "Google-Extended",
    "Applebot-Extended",
    "CCBot",
];

/// One page of a [`Sitemap`].
#[derive(Clone, Debug)]
#[must_use]
pub struct SitemapUrl {
    loc: String,
    lastmod: Option<String>,
    languages: Vec<(String, String)>,
}

impl SitemapUrl {
    /// The page at `loc`: its absolute, canonical address.
    pub fn new(loc: impl Into<String>) -> Self {
        Self {
            loc: loc.into(),
            lastmod: None,
            languages: Vec::new(),
        }
    }

    /// When its content last changed: a date such as `2026-11-12`, or a
    /// `chrono::NaiveDate`. Never a date in the future, and never the build's date for a
    /// page that didn't change.
    pub fn lastmod(mut self, date: impl Display) -> Self {
        self.lastmod = Some(date.to_string());
        self
    }

    /// The same page in another language, as [`Seo::language`](super::Seo::language)
    /// lists it in the page's head.
    pub fn language(mut self, hreflang: impl Into<String>, url: impl Into<String>) -> Self {
        self.languages.push((hreflang.into(), url.into()));
        self
    }
}

/// `sitemap.xml`: every page a search engine should list, each with when it last changed.
/// Serve it from a route; it answers with its XML.
///
/// ```
/// use navree_ui::seo::{Sitemap, SitemapUrl};
///
/// let xml = Sitemap::new()
///     .url(SitemapUrl::new("https://kesar.example/").lastmod("2026-11-12"))
///     .url(SitemapUrl::new("https://kesar.example/pricing/").lastmod("2026-11-02"))
///     .to_xml();
/// assert!(xml.contains("<loc>https://kesar.example/pricing/</loc><lastmod>2026-11-02</lastmod>"));
/// ```
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct Sitemap {
    urls: Vec<SitemapUrl>,
}

impl Sitemap {
    /// An empty sitemap.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a page. A page listed twice is listed once.
    pub fn url(mut self, url: SitemapUrl) -> Self {
        if !self.urls.iter().any(|known| known.loc == url.loc) {
            self.urls.push(url);
        }
        self
    }

    /// Adds pages.
    pub fn urls(self, urls: impl IntoIterator<Item = SitemapUrl>) -> Self {
        urls.into_iter().fold(self, Self::url)
    }

    /// The addresses it lists, in order.
    pub fn locations(&self) -> impl Iterator<Item = &str> {
        self.urls.iter().map(|url| url.loc.as_str())
    }

    /// The file.
    pub fn to_xml(&self) -> String {
        let languages = self.urls.iter().any(|url| !url.languages.is_empty());
        let mut out = String::with_capacity(96 + self.urls.len() * 96);
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\"");
        if languages {
            out.push_str(" xmlns:xhtml=\"http://www.w3.org/1999/xhtml\"");
        }
        out.push_str(">\n");
        for url in &self.urls {
            out.push_str("<url><loc>");
            escape_into(&mut out, &url.loc);
            out.push_str("</loc>");
            if let Some(date) = &url.lastmod {
                out.push_str("<lastmod>");
                escape_into(&mut out, date);
                out.push_str("</lastmod>");
            }
            for (hreflang, href) in &url.languages {
                out.push_str("<xhtml:link rel=\"alternate\" hreflang=\"");
                escape_into(&mut out, hreflang);
                out.push_str("\" href=\"");
                escape_into(&mut out, href);
                out.push_str("\"/>");
            }
            out.push_str("</url>\n");
        }
        out.push_str("</urlset>\n");
        out
    }
}

impl IntoResponse for Sitemap {
    fn into_response(self) -> Response {
        file(self.to_xml(), "application/xml; charset=utf-8")
    }
}

/// One group of rules in a [`RobotsTxt`].
#[derive(Clone, Debug)]
struct Group {
    agents: Vec<String>,
    allow: Vec<String>,
    disallow: Vec<String>,
}

/// `robots.txt`: which crawlers may read which paths, and where the sitemap is. Serve it
/// from a route; it answers with its text.
///
/// ```
/// use navree_ui::seo::{AI_CRAWLERS, RobotsTxt};
///
/// let text = RobotsTxt::new()
///     .disallow("/account/")
///     .welcome(AI_CRAWLERS)
///     .sitemap("https://kesar.example/sitemap.xml")
///     .to_text();
/// assert!(text.starts_with("User-agent: *\nAllow: /\nDisallow: /account/\n"));
/// assert!(text.contains("User-agent: ClaudeBot\nAllow: /\n"));
/// assert!(text.ends_with("Sitemap: https://kesar.example/sitemap.xml\n"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct RobotsTxt {
    groups: Vec<Group>,
    sitemaps: Vec<String>,
}

impl RobotsTxt {
    /// Every crawler may read every path.
    pub fn new() -> Self {
        Self {
            groups: vec![Group {
                agents: vec!["*".to_owned()],
                allow: vec!["/".to_owned()],
                disallow: Vec::new(),
            }],
            sitemaps: Vec::new(),
        }
    }

    /// Keeps every crawler out of paths that start with `path`: an account area, a search
    /// page, an admin. A page that must not be listed also says so in its own head
    /// ([`Robots::noindex`](super::Robots::noindex)); this only stops the reading.
    pub fn disallow(mut self, path: impl Into<String>) -> Self {
        let path = clean(&path.into());
        if !path.is_empty() {
            self.groups[0].disallow.push(path);
        }
        self
    }

    /// Names each crawler in `agents` in a group of its own that allows every path: the
    /// rule for everyone already does, and naming them keeps the welcome explicit if that
    /// rule ever changes. [`AI_CRAWLERS`] is the usual list.
    pub fn welcome(mut self, agents: &[&str]) -> Self {
        for agent in agents {
            let agent = clean(agent);
            if agent.is_empty() {
                continue;
            }
            self.groups.push(Group {
                agents: vec![agent],
                allow: vec!["/".to_owned()],
                disallow: Vec::new(),
            });
        }
        self
    }

    /// Keeps the crawler `agent` out of the whole site.
    pub fn block(mut self, agent: &str) -> Self {
        let agent = clean(agent);
        if !agent.is_empty() {
            self.groups.push(Group {
                agents: vec![agent],
                allow: Vec::new(),
                disallow: vec!["/".to_owned()],
            });
        }
        self
    }

    /// Where the sitemap is: an absolute URL.
    pub fn sitemap(mut self, url: impl Into<String>) -> Self {
        let url = clean(&url.into());
        if !url.is_empty() {
            self.sitemaps.push(url);
        }
        self
    }

    /// The file.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for group in &self.groups {
            for agent in &group.agents {
                out.push_str("User-agent: ");
                out.push_str(agent);
                out.push('\n');
            }
            for path in &group.allow {
                out.push_str("Allow: ");
                out.push_str(path);
                out.push('\n');
            }
            for path in &group.disallow {
                out.push_str("Disallow: ");
                out.push_str(path);
                out.push('\n');
            }
            out.push('\n');
        }
        for url in &self.sitemaps {
            out.push_str("Sitemap: ");
            out.push_str(url);
            out.push('\n');
        }
        out
    }
}

impl Default for RobotsTxt {
    fn default() -> Self {
        Self::new()
    }
}

impl IntoResponse for RobotsTxt {
    fn into_response(self) -> Response {
        file(self.to_text(), "text/plain; charset=utf-8")
    }
}

/// A value on one line of a robots file: what comes before any line break or other
/// control character, without a `#`, which would start a comment.
fn clean(text: &str) -> String {
    text.split(char::is_control)
        .next()
        .unwrap_or_default()
        .replace('#', "")
        .trim()
        .to_owned()
}

/// A text file that a shared cache may keep for an hour.
fn file(body: String, content_type: &'static str) -> Response {
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (CACHE_CONTROL, HeaderValue::from_static(FILE_CACHE)),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sitemap_escapes_and_lists_each_page_once() {
        let xml = Sitemap::new()
            .url(SitemapUrl::new("https://kesar.example/?a=1&b=<2>").lastmod("2026-11-12"))
            .url(SitemapUrl::new("https://kesar.example/?a=1&b=<2>"))
            .url(
                SitemapUrl::new("https://kesar.example/en/")
                    .language("hi-IN", "https://kesar.example/hi/")
                    .language("x-default", "https://kesar.example/en/"),
            )
            .to_xml();
        assert_eq!(xml.matches("<url>").count(), 2, "{xml}");
        assert!(
            xml.contains("<loc>https://kesar.example/?a=1&amp;b=&lt;2&gt;</loc>"),
            "{xml}"
        );
        assert!(xml.contains("xmlns:xhtml"), "{xml}");
        assert!(
            xml.contains(
                r#"<xhtml:link rel="alternate" hreflang="hi-IN" href="https://kesar.example/hi/"/>"#
            ),
            "{xml}"
        );
    }

    #[test]
    fn a_robots_file_has_no_injected_lines() {
        let text = RobotsTxt::new()
            .disallow("/x\nUser-agent: evil\nDisallow: /")
            .welcome(&["Good\nBot", "#comment", ""])
            .block("SlurpBot")
            .sitemap("https://kesar.example/sitemap.xml")
            .to_text();
        assert!(text.contains("Disallow: /x\n\n"), "{text}");
        assert!(!text.contains("evil"), "{text}");
        assert!(text.contains("User-agent: Good\nAllow: /\n"), "{text}");
        assert!(text.contains("User-agent: comment\n"), "{text}");
        assert!(
            text.contains("User-agent: SlurpBot\nDisallow: /\n"),
            "{text}"
        );
        assert_eq!(text.matches("User-agent:").count(), 4, "{text}");
    }

    #[test]
    fn the_files_answer_with_their_types() {
        let response = Sitemap::new().into_response();
        assert_eq!(
            response.headers()[http::header::CONTENT_TYPE],
            "application/xml; charset=utf-8"
        );
        let response = RobotsTxt::new().into_response();
        assert_eq!(
            response.headers()[http::header::CONTENT_TYPE],
            "text/plain; charset=utf-8"
        );
        assert_eq!(response.headers()[http::header::CACHE_CONTROL], FILE_CACHE);
    }
}
