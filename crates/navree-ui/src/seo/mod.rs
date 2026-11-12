//! What a page tells search engines, link previews and answer engines about itself: its
//! description, canonical address, robots rules, Open Graph and Twitter cards, other
//! languages, and structured data, written in the head by [`View::seo`](crate::View::seo).
//! [`Sitemap`] and [`RobotsTxt`] are the two files a public site serves beside its pages.
//!
//! ```
//! use navree_ui::View;
//! use navree_ui::seo::{JsonLd, Organization, Seo, SocialImage, WebSite};
//!
//! let site = "https://kesar.example";
//! let page = View::new("Kesar: sleep, heart and steps in one place")
//!     .seo(
//!         Seo::new("Kesar reads the health data your phone already has and turns it into one calm daily score, in Hindi and English.")
//!             .canonical(format!("{site}/"))
//!             .site_name("Kesar")
//!             .locale("en_IN")
//!             .image(SocialImage::new(format!("{site}/og.png"), 1200, 630, "Kesar's daily score: a ring at 82"))
//!             .json_ld(
//!                 JsonLd::new()
//!                     .add(Organization::new("Kesar", format!("{site}/")).id(format!("{site}/#organization")))
//!                     .add(WebSite::new("Kesar", format!("{site}/")).publisher(format!("{site}/#organization"))),
//!             ),
//!     );
//! let html = page.to_html();
//! assert!(html.contains(r#"<link rel="canonical" href="https://kesar.example/">"#));
//! assert!(html.contains(r#"<meta property="og:image:width" content="1200">"#));
//! assert!(html.contains(r#"<script type="application/ld+json">"#));
//! ```
//!
//! # The content security policy
//!
//! The structured data is a `<script type="application/ld+json">` block. A browser never
//! runs a script of that type, so [`PAGE_CSP`](crate::PAGE_CSP)'s `script-src 'self'` does
//! not apply to it and it needs no nonce or hash: the page keeps its policy as it is. Its
//! text is written so that it can't end the block early: `<`, `>` and `&` are written as
//! `<`, `>` and `&`, and the line and paragraph separators as escapes too,
//! which every JSON reader reads back as the same characters.

mod files;
mod ld;

use std::borrow::Cow;

pub use files::{AI_CRAWLERS, RobotsTxt, Sitemap, SitemapUrl};
pub use ld::{
    BreadcrumbList, FaqPage, JsonLd, Offer, Organization, PageKind, SoftwareApplication, Thing,
    WebPage, WebSite, script_json,
};

use crate::html::{Href, escape_into};

/// The shortest and longest title a search result shows whole, in characters.
pub const TITLE_LENGTH: (usize, usize) = (30, 60);

/// The shortest and longest description a search result shows whole, in characters.
pub const DESCRIPTION_LENGTH: (usize, usize) = (70, 160);

/// What a page asks of search engines: whether to list it, whether to follow its links,
/// and how much of it a result may show.
///
/// The default lists the page, follows its links and allows a large image preview and a
/// snippet of any length, which is what a public page wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct Robots {
    index: bool,
    follow: bool,
}

impl Robots {
    /// List the page and follow its links, with large previews.
    pub const fn index() -> Self {
        Self {
            index: true,
            follow: true,
        }
    }

    /// Keep the page out of results, but follow its links: a not-found page, a thank-you
    /// page, a search page.
    pub const fn noindex() -> Self {
        Self {
            index: false,
            follow: true,
        }
    }

    /// Don't follow the page's links either.
    pub const fn nofollow(mut self) -> Self {
        self.follow = false;
        self
    }

    /// Whether the page asks to be listed.
    pub const fn is_indexed(self) -> bool {
        self.index
    }

    /// The `content` of `<meta name="robots">`.
    pub fn content(self) -> &'static str {
        match (self.index, self.follow) {
            (true, true) => {
                "index, follow, max-image-preview:large, max-snippet:-1, max-video-preview:-1"
            }
            (true, false) => {
                "index, nofollow, max-image-preview:large, max-snippet:-1, max-video-preview:-1"
            }
            (false, true) => "noindex, follow",
            (false, false) => "noindex, nofollow",
        }
    }
}

impl Default for Robots {
    fn default() -> Self {
        Self::index()
    }
}

/// What kind of thing a page is, for Open Graph (`og:type`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum OgType {
    /// A page of a site: the home page, a features or pricing page. The default.
    #[default]
    Website,
    /// A post, a story, a page of documentation.
    Article,
    /// A thing that is sold.
    Product,
    /// A person.
    Profile,
}

impl OgType {
    /// The value of `og:type`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Website => "website",
            Self::Article => "article",
            Self::Product => "product",
            Self::Profile => "profile",
        }
    }
}

/// The card a link to the page unfolds into on X and the apps that read its tags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TwitterCard {
    /// A wide image above the title. The default.
    #[default]
    SummaryLargeImage,
    /// A small square image beside the title.
    Summary,
}

impl TwitterCard {
    /// The value of `twitter:card`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SummaryLargeImage => "summary_large_image",
            Self::Summary => "summary",
        }
    }
}

/// The picture a shared link shows: an absolute URL, its size in pixels and what it shows,
/// in words. 1200 by 630 is the size every network crops well.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct SocialImage {
    url: String,
    width: u32,
    height: u32,
    alt: String,
    mime: Option<&'static str>,
}

impl SocialImage {
    /// The picture at `url` (absolute, on `https`), `width` by `height` pixels, showing
    /// what `alt` says.
    pub fn new(url: impl Into<String>, width: u32, height: u32, alt: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            width,
            height,
            alt: alt.into(),
            mime: None,
        }
    }

    /// Its type, written as `og:image:type`: `image/png`, `image/jpeg` or `image/webp`.
    pub fn mime(mut self, mime: &'static str) -> Self {
        self.mime = Some(mime);
        self
    }

    /// The URL.
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// What a page says about itself in its head, for search engines, link previews and
/// answer engines. [`View::seo`](crate::View::seo) writes it.
///
/// Every value is escaped as it is written, and a URL that isn't `http` or `https` is left
/// out. [`problems`](Self::problems) lists what a search engine would hold against the
/// page: a title or description of the wrong length, an address that isn't absolute, a
/// structured data node without the properties its type needs.
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct Seo {
    description: Option<String>,
    canonical: Option<String>,
    robots: Robots,
    title: Option<String>,
    site_name: Option<String>,
    locale: Option<String>,
    other_locales: Vec<String>,
    og_type: OgType,
    image: Option<SocialImage>,
    card: TwitterCard,
    twitter_site: Option<String>,
    twitter_creator: Option<String>,
    languages: Vec<(String, String)>,
    formats: Vec<(&'static str, String)>,
    data: Vec<JsonLd>,
}

impl Seo {
    /// A page described by `description`: what a search result shows under its title, and
    /// what a shared link shows under its picture. 70 to 160 characters show whole.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: Some(description.into()),
            ..Self::default()
        }
    }

    /// The page's one address, absolute: what a search engine lists, whatever query or
    /// path it was reached by. Also the page's `og:url`.
    pub fn canonical(mut self, url: impl Into<String>) -> Self {
        self.canonical = Some(url.into());
        self
    }

    /// What search engines may do with the page. Listed, with large previews, by default.
    pub fn robots(mut self, robots: Robots) -> Self {
        self.robots = robots;
        self
    }

    /// Keeps the page out of search results: shorthand for `.robots(Robots::noindex())`.
    pub fn noindex(self) -> Self {
        self.robots(Robots::noindex())
    }

    /// The title a shared link shows, when it should differ from the page's `<title>`
    /// (which drops the site's name, say).
    pub fn social_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The site's name, shown above a shared link's title (`og:site_name`).
    pub fn site_name(mut self, name: impl Into<String>) -> Self {
        self.site_name = Some(name.into());
        self
    }

    /// The page's language and region for Open Graph, with an underscore: `en_IN`.
    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = Some(locale.into());
        self
    }

    /// Another language and region the page is published in (`og:locale:alternate`).
    pub fn other_locale(mut self, locale: impl Into<String>) -> Self {
        self.other_locales.push(locale.into());
        self
    }

    /// What kind of thing the page is. A website page by default.
    pub fn og_type(mut self, kind: OgType) -> Self {
        self.og_type = kind;
        self
    }

    /// The picture a shared link shows, for Open Graph and the Twitter card both.
    pub fn image(mut self, image: SocialImage) -> Self {
        self.image = Some(image);
        self
    }

    /// The Twitter card: a large image by default.
    pub fn twitter_card(mut self, card: TwitterCard) -> Self {
        self.card = card;
        self
    }

    /// The site's account on X, with its `@`.
    pub fn twitter_site(mut self, handle: impl Into<String>) -> Self {
        self.twitter_site = Some(handle.into());
        self
    }

    /// The author's account on X, with its `@`.
    pub fn twitter_creator(mut self, handle: impl Into<String>) -> Self {
        self.twitter_creator = Some(handle.into());
        self
    }

    /// The same page in another language: `hreflang` is a language tag (`hi-IN`, `en`)
    /// or `x-default` for the page a reader of any other language gets. List every
    /// language, this page's own included, on every one of them.
    pub fn language(mut self, hreflang: impl Into<String>, url: impl Into<String>) -> Self {
        self.languages.push((hreflang.into(), url.into()));
        self
    }

    /// The page in another format, such as its Markdown (`text/markdown`) for answer
    /// engines, or the site's feed (`application/rss+xml`).
    pub fn format(mut self, mime: &'static str, url: impl Into<String>) -> Self {
        self.formats.push((mime, url.into()));
        self
    }

    /// Adds structured data. Call it again for another block; one graph per page is the
    /// usual way, and lets nodes point at each other by `@id`.
    pub fn json_ld(mut self, data: JsonLd) -> Self {
        self.data.push(data);
        self
    }

    /// The description.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The canonical address.
    pub fn canonical_url(&self) -> Option<&str> {
        self.canonical.as_deref()
    }

    /// What search engines may do with the page.
    pub fn robots_rule(&self) -> Robots {
        self.robots
    }

    /// The structured data blocks.
    pub fn data(&self) -> &[JsonLd] {
        &self.data
    }

    /// What a search engine or a link preview would hold against the page titled `title`,
    /// in words, one line each: empty when the head is in order. The rules are a search
    /// result's: a title of 30 to 60 characters and a description of 70 to 160, an
    /// absolute `https` canonical address and picture, the picture's size and alt text,
    /// the site's name and locale, and structured data whose nodes have the properties
    /// their types need and whose `@id` references point at nodes of the page. A page
    /// that asks not to be listed is only checked for that.
    pub fn problems(&self, title: &str) -> Vec<String> {
        let mut problems = Vec::new();
        if !self.robots.is_indexed() {
            return problems;
        }
        let length = |text: &str| text.chars().count();
        let title_length = length(title);
        if title_length < TITLE_LENGTH.0 || title_length > TITLE_LENGTH.1 {
            problems.push(format!(
                "the title is {title_length} characters ({}-{})",
                TITLE_LENGTH.0, TITLE_LENGTH.1
            ));
        }
        match self.description.as_deref() {
            None | Some("") => problems.push("no description".to_owned()),
            Some(description) => {
                let n = length(description);
                if n < DESCRIPTION_LENGTH.0 || n > DESCRIPTION_LENGTH.1 {
                    problems.push(format!(
                        "the description is {n} characters ({}-{})",
                        DESCRIPTION_LENGTH.0, DESCRIPTION_LENGTH.1
                    ));
                }
            }
        }
        match self.canonical.as_deref() {
            None => problems.push("no canonical address".to_owned()),
            Some(url) if !is_absolute_https(url) => problems.push(format!(
                "the canonical address {url} is not an absolute https URL"
            )),
            _ => {}
        }
        match &self.image {
            None => problems.push("no picture for a shared link".to_owned()),
            Some(image) => {
                if !is_absolute_https(&image.url) {
                    problems.push(format!(
                        "the picture {} is not an absolute https URL",
                        image.url
                    ));
                }
                if image.alt.trim().is_empty() {
                    problems.push("the picture has no alt text".to_owned());
                }
                if image.width == 0 || image.height == 0 {
                    problems.push("the picture has no size".to_owned());
                }
            }
        }
        if self.site_name.is_none() {
            problems.push("no site name".to_owned());
        }
        if self.locale.is_none() {
            problems.push("no locale".to_owned());
        }
        for (hreflang, url) in &self.languages {
            if !is_absolute_https(url) {
                problems.push(format!(
                    "the {hreflang} page {url} is not an absolute https URL"
                ));
            }
        }
        problems.extend(ld::problems(&self.data, self.canonical.as_deref()));
        problems
    }

    /// Writes the tags into a page's head, after its `<title>`.
    pub(crate) fn write_head(&self, out: &mut String, title: &str) {
        let title = self.title.as_deref().unwrap_or(title);
        if let Some(description) = &self.description {
            meta(out, "name", "description", description);
        }
        meta(out, "name", "robots", self.robots.content());
        if let Some(url) = self.canonical.as_deref().and_then(web_url) {
            link(out, "rel=\"canonical\"", &url);
        }
        for (hreflang, url) in &self.languages {
            if let Some(url) = web_url(url) {
                out.push_str("<link rel=\"alternate\" hreflang=\"");
                escape_into(out, hreflang);
                out.push_str("\" href=\"");
                escape_into(out, &url);
                out.push_str("\">");
            }
        }
        for (mime, url) in &self.formats {
            if let Some(url) = web_url(url) {
                out.push_str("<link rel=\"alternate\" type=\"");
                escape_into(out, mime);
                out.push_str("\" href=\"");
                escape_into(out, &url);
                out.push_str("\">");
            }
        }
        meta(out, "property", "og:type", self.og_type.as_str());
        if let Some(name) = &self.site_name {
            meta(out, "property", "og:site_name", name);
        }
        meta(out, "property", "og:title", title);
        if let Some(description) = &self.description {
            meta(out, "property", "og:description", description);
        }
        if let Some(url) = self.canonical.as_deref().and_then(web_url) {
            meta(out, "property", "og:url", &url);
        }
        if let Some(locale) = &self.locale {
            meta(out, "property", "og:locale", locale);
        }
        for locale in &self.other_locales {
            meta(out, "property", "og:locale:alternate", locale);
        }
        let image = self
            .image
            .as_ref()
            .and_then(|image| web_url(&image.url).map(|url| (image, url)));
        if let Some((image, url)) = &image {
            meta(out, "property", "og:image", url);
            meta(out, "property", "og:image:width", &image.width.to_string());
            meta(
                out,
                "property",
                "og:image:height",
                &image.height.to_string(),
            );
            meta(out, "property", "og:image:alt", &image.alt);
            if let Some(mime) = image.mime {
                meta(out, "property", "og:image:type", mime);
            }
        }
        meta(out, "name", "twitter:card", self.card.as_str());
        meta(out, "name", "twitter:title", title);
        if let Some(description) = &self.description {
            meta(out, "name", "twitter:description", description);
        }
        if let Some((image, url)) = &image {
            meta(out, "name", "twitter:image", url);
            meta(out, "name", "twitter:image:alt", &image.alt);
        }
        if let Some(site) = &self.twitter_site {
            meta(out, "name", "twitter:site", site);
        }
        if let Some(creator) = &self.twitter_creator {
            meta(out, "name", "twitter:creator", creator);
        }
        for data in &self.data {
            out.push_str("<script type=\"application/ld+json\">");
            out.push_str(&data.to_script());
            out.push_str("</script>");
        }
    }
}

/// `<meta {attr}="{key}" content="{value}">`.
fn meta(out: &mut String, attr: &str, key: &str, value: &str) {
    out.push_str("<meta ");
    out.push_str(attr);
    out.push_str("=\"");
    out.push_str(key);
    out.push_str("\" content=\"");
    escape_into(out, value);
    out.push_str("\">");
}

/// `<link {rel} href="{url}">`.
fn link(out: &mut String, rel: &str, url: &str) {
    out.push_str("<link ");
    out.push_str(rel);
    out.push_str(" href=\"");
    escape_into(out, url);
    out.push_str("\">");
}

/// `url` when it is safe in a link and points at the web: a path, or an `http` or
/// `https` address. A `mailto:`, `tel:` or a scheme that runs something is left out.
fn web_url(url: &str) -> Option<Cow<'_, str>> {
    let href = Href::parse(url).ok()?;
    let lower = href
        .as_str()
        .get(..8)
        .unwrap_or(href.as_str())
        .to_ascii_lowercase();
    if lower.starts_with("mailto:") || lower.starts_with("tel:") {
        return None;
    }
    Some(Cow::Borrowed(url))
}

/// Whether `url` is an absolute `https` URL on a host with a dot in its name.
pub(crate) fn is_absolute_https(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    let name = host.split(':').next().unwrap_or_default();
    !name.is_empty()
        && name.contains('.')
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.')
        && !url.bytes().any(|byte| byte <= b' ' || byte == 0x7f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robots_say_what_a_page_asks() {
        assert!(
            Robots::default()
                .content()
                .contains("max-image-preview:large")
        );
        assert_eq!(Robots::noindex().content(), "noindex, follow");
        assert_eq!(Robots::noindex().nofollow().content(), "noindex, nofollow");
        assert!(
            Robots::index()
                .nofollow()
                .content()
                .starts_with("index, nofollow")
        );
    }

    #[test]
    fn an_https_address_is_absolute_only_with_a_real_host() {
        for good in [
            "https://kesar.example/",
            "https://kesar.example",
            "https://www.kesar.example/pricing/?a=1#b",
            "https://kesar.example:8443/x",
        ] {
            assert!(is_absolute_https(good), "{good}");
        }
        for bad in [
            "http://kesar.example/",
            "/pricing/",
            "https://localhost/",
            "https:///x",
            "https://.example/",
            "https://kes ar.example/",
            "javascript:alert(1)",
            "https://kesar.example/\n",
        ] {
            assert!(!is_absolute_https(bad), "{bad}");
        }
    }

    #[test]
    fn web_urls_are_paths_or_http() {
        assert!(web_url("https://kesar.example/").is_some());
        assert!(web_url("/og.png").is_some());
        assert!(web_url("javascript:alert(1)").is_none());
        assert!(web_url("mailto:hi@kesar.example").is_none());
        assert!(web_url("data:text/html,x").is_none());
    }
}
