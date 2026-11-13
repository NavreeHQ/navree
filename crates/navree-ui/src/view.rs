//! [`View`]: a whole page, with its content security policy.

use std::borrow::Cow;

use http::StatusCode;
use http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderName, HeaderValue, SET_COOKIE, VARY,
};
use navree_http::response::{IntoResponse, Response};

use crate::app_files::{AppFont, AppSheet};
use crate::assets;
use crate::form::csrf_field;
use crate::html::escape_into;
use crate::node::{IntoNodes, Node, Uses, el};
use crate::render::{Measure, POST_FORM, measure, render_into};
use crate::seo::Seo;
use crate::theme::{Appearance, Density, Font, Theme};

/// The content security policy of every page: scripts, styles, fonts and
/// requests from the page's own origin only, images also as `data:` URLs, no
/// inline scripts or styles, no framing.
pub const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// The `Speculation-Rules` header's name.
const SPECULATION_RULES: HeaderName = HeaderName::from_static("speculation-rules");

/// What the document head costs before its title and scripts.
const HEAD_BYTES: usize = 512;

/// What a page's search and preview tags usually cost, structured data included.
const SEO_BYTES: usize = 2048;

/// A whole page: `<head>` from the toolkit, `<body>` from you.
///
/// A page whose tree needs no script ships none: the links it marks with
/// `prefetch` are fetched ahead through the `Speculation-Rules` header, with no
/// JavaScript. A page that
/// needs the runtime loads it, and the runtime boosts its links.
///
/// ```
/// use navree_ui::{View, el};
///
/// let html = View::new("Deals").body(el("main").child("Hello")).to_html();
/// assert!(html.contains("<title>Deals</title>"));
/// assert!(!html.contains("<script"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct View {
    title: Cow<'static, str>,
    lang: &'static str,
    body: Node,
    status: StatusCode,
    cache: &'static str,
    vary: Vec<&'static str>,
    cookies: Vec<HeaderValue>,
    csrf: Option<String>,
    appearance: Option<Appearance>,
    density: Option<Density>,
    sheets: Vec<Theme>,
    app_sheets: Vec<AppSheet>,
    app_fonts: Vec<AppFont>,
    mark_build: bool,
    /// The page in the app's shell this view was made from while a native
    /// client asked: its body is then built only if the view is written as HTML.
    #[cfg(feature = "native-preview")]
    note: Option<Box<crate::shell::PageNote>>,
    seo: Option<Box<Seo>>,
}

impl View {
    /// A page titled `title`.
    pub fn new(title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            title: title.into(),
            lang: "en",
            body: Node::Empty,
            status: StatusCode::OK,
            cache: "no-store",
            vary: Vec::new(),
            cookies: Vec::new(),
            csrf: None,
            appearance: None,
            density: None,
            sheets: Vec::new(),
            app_sheets: Vec::new(),
            app_fonts: Vec::new(),
            mark_build: false,
            #[cfg(feature = "native-preview")]
            note: None,
            seo: None,
        }
    }

    /// Sets how the page looks: `data-theme` and `data-mode` on `<html>`, the
    /// `color-scheme` meta, the theme's density, and the theme's stylesheet in the
    /// head when it is one of your own. All of it is in the first bytes of the
    /// page, so it paints right with no script.
    pub fn appearance(mut self, appearance: Appearance) -> Self {
        self.appearance = Some(appearance);
        self
    }

    /// Sets the page's density, in place of its theme's.
    pub fn density(mut self, density: Density) -> Self {
        self.density = Some(density);
        self
    }

    /// Links the stylesheet of a theme the page draws in a part of itself (a
    /// card in a theme menu, a preview): a theme of your own needs its sheet
    /// wherever it is drawn. A built-in needs none and is passed over.
    pub fn theme_sheet(mut self, theme: &Theme) -> Self {
        if !theme.is_builtin() && !self.sheets.iter().any(|sheet| sheet.key() == theme.key()) {
            self.sheets.push(theme.clone());
        }
        self
    }

    /// Links a stylesheet of the app's own ([`AppSheet`]), after the toolkit's sheets and the
    /// page's theme, so its rules win over rules of equal weight. Sheets load in the order
    /// they are added, each once.
    pub fn stylesheet(mut self, sheet: &AppSheet) -> Self {
        if !self.app_sheets.contains(sheet) {
            self.app_sheets.push(*sheet);
        }
        self
    }

    /// Links a typeface of the app's own ([`AppFont`]): its `@font-face` rule, and its file
    /// as a preload, so text drawn in it paints right on first load. Each font once.
    pub fn font(mut self, font: &AppFont) -> Self {
        if !self.app_fonts.contains(font) {
            self.app_fonts.push(*font);
        }
        self
    }

    /// The app's stylesheets the page links, in order. A static copy of the page writes each
    /// one's [`AppSheet::bytes`] where its [`AppSheet::url`] points.
    pub fn app_sheets(&self) -> &[AppSheet] {
        &self.app_sheets
    }

    /// The app's fonts the page links. A static copy of the page writes each one's
    /// [`AppFont::face`] and [`AppFont::bytes`] where their URLs point.
    pub fn app_fonts(&self) -> &[AppFont] {
        &self.app_fonts
    }

    /// The stylesheets of the themes the page draws, its own first: each URL once.
    fn theme_sheets(&self) -> Vec<String> {
        self.own_themes()
            .iter()
            .filter_map(|theme| theme.stylesheet())
            .collect()
    }

    /// The themes of your own that the page draws, its own first, each once: what
    /// its `<link data-ui-theme>` stylesheets are for. A built-in is in the
    /// toolkit's stylesheet and is not listed. A static copy of the page, such as
    /// a docs preview, writes each one's [`Theme::css`] where its
    /// [`Theme::stylesheet`] points.
    pub fn own_themes(&self) -> Vec<&Theme> {
        let mut themes: Vec<&Theme> = Vec::new();
        let mut urls: Vec<String> = Vec::new();
        let page = self.appearance.as_ref().map(Appearance::theme);
        for theme in page.into_iter().chain(&self.sheets) {
            if let Some(url) = theme.stylesheet()
                && !urls.contains(&url)
            {
                urls.push(url);
                themes.push(theme);
            }
        }
        themes
    }

    /// What `<html>` carries for the page's look, in the order the page writes it:
    /// `data-theme` and `data-mode` for a look other than the default, then
    /// `data-density` for a density other than the default. Each value is a
    /// theme's key, a mode's id or a density's id, never text from outside. A
    /// static copy of the page writes them as they are.
    pub fn look(&self) -> Vec<(&'static str, String)> {
        let mut look = Vec::new();
        if let Some(appearance) = self.appearance.as_ref().filter(|a| !a.is_default()) {
            look.push(("data-theme", appearance.theme().key().to_owned()));
            look.push(("data-mode", appearance.mode().id().to_owned()));
        }
        let density = self
            .density
            .or_else(|| self.appearance.as_ref().map(|a| a.theme().spec().density))
            .unwrap_or_default();
        if density != Density::Default {
            look.push(("data-density", density.id().to_owned()));
        }
        look
    }

    /// The page's `color-scheme`: its mode's, or `dark`.
    pub fn color_scheme(&self) -> &'static str {
        self.appearance
            .as_ref()
            .map_or("dark", |appearance| appearance.mode().color_scheme())
    }

    /// Says what the page is to search engines, link previews and answer engines: its
    /// description, canonical address, robots rules, Open Graph and Twitter cards, its other
    /// languages and its structured data, all written in the head after the title. See
    /// [`seo`](crate::seo).
    ///
    /// ```
    /// use navree_ui::View;
    /// use navree_ui::seo::Seo;
    ///
    /// let html = View::new("Pricing: Kesar plans in rupees, monthly or yearly")
    ///     .seo(
    ///         Seo::new("Three plans for sleep, heart and steps tracking, from free to family.")
    ///             .canonical("https://kesar.example/pricing/"),
    ///     )
    ///     .to_html();
    /// assert!(html.contains(r#"<meta name="description" content="Three plans"#));
    /// assert!(html.contains(r#"<meta property="og:title" content="Pricing: Kesar plans"#));
    /// ```
    pub fn seo(mut self, seo: Seo) -> Self {
        self.seo = Some(Box::new(seo));
        self
    }

    /// What [`seo`](Self::seo) says about the page, when it was set.
    pub fn seo_info(&self) -> Option<&Seo> {
        self.seo.as_deref()
    }

    /// What a search engine or a link preview would hold against the page: see
    /// [`Seo::problems`]. A page with no [`seo`](Self::seo) has one problem, that it says
    /// nothing; a test of a public site's pages asserts this is empty.
    pub fn seo_problems(&self) -> Vec<String> {
        match &self.seo {
            Some(seo) => seo.problems(&self.title),
            None => vec!["the page says nothing about itself: no `View::seo`".to_owned()],
        }
    }

    /// Sets the page's language.
    pub fn lang(mut self, lang: &'static str) -> Self {
        self.lang = lang;
        self
    }

    /// Sets the page's content.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        self.body = Node::list(body);
        #[cfg(feature = "native-preview")]
        {
            self.note = None;
        }
        self
    }

    /// Sets the status.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Sets `Cache-Control` (`no-store` by default: pages show private data).
    /// A page in the app shell is drawn in the reader's theme, read from a
    /// cookie, so the same address differs from reader to reader: cache it as
    /// `private`, never in a shared cache.
    pub fn cache(mut self, cache: &'static str) -> Self {
        self.cache = cache;
        self
    }

    /// Adds a request header to `Vary`: the page differs by it, so a cache keeps one copy
    /// per value. A page that is public for a guest and private for a reader who is signed
    /// in varies by `Cookie` (and `Authorization`, for a client that signs in with a token),
    /// for the caches that honour it. Several CDNs ignore `Vary: Cookie`: in front of one,
    /// also skip the cache for requests that carry the session cookie, and send a signed-in
    /// reader's page as `private, no-store`:
    ///
    /// ```
    /// use navree_http::response::IntoResponse;
    /// use navree_ui::View;
    ///
    /// let response = View::new("Kesar")
    ///     .cache("public, max-age=300")
    ///     .vary("Cookie")
    ///     .vary("Authorization")
    ///     .into_response();
    /// assert_eq!(response.headers()["vary"], "Cookie, Authorization");
    /// ```
    ///
    /// A name that is not a header's is passed over, and each name is written once.
    pub fn vary(mut self, header: &'static str) -> Self {
        if HeaderName::from_bytes(header.as_bytes()).is_ok()
            && !self
                .vary
                .iter()
                .any(|have| have.eq_ignore_ascii_case(header))
        {
            self.vary.push(header);
        }
        self
    }

    /// Gives the page's post form the session's CSRF token. A page with a menu's post
    /// items has one hidden form that they all post through, so none is nested in a form of
    /// yours; with the token in it they also work with JavaScript off.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Writes the build on `<html>` though the page loads no runtime, so the runtime of the
    /// page a boosted link comes from can show this one in place. A page that loads the runtime
    /// carries it already.
    pub fn mark_build(mut self) -> Self {
        self.mark_build = true;
        self
    }

    /// Adds a `Set-Cookie` header.
    pub(crate) fn cookie(mut self, cookie: HeaderValue) -> Self {
        self.cookies.push(cookie);
        self
    }

    /// The page's content.
    pub fn body_node(&self) -> &Node {
        #[cfg(feature = "native-preview")]
        if let Some(note) = &self.note {
            return note.chrome();
        }
        &self.body
    }

    /// What this page loads.
    pub fn uses(&self) -> Uses {
        measure(self.body_node()).uses
    }

    /// The page's HTML.
    pub fn to_html(&self) -> String {
        let found = measure(self.body_node());
        let seo = if self.seo.is_some() { SEO_BYTES } else { 0 };
        let mut out = String::with_capacity(found.bytes + self.title.len() + HEAD_BYTES + seo);
        self.write(&mut out, &found);
        out
    }

    fn write(&self, out: &mut String, found: &Measure) {
        let uses = found.uses;
        out.push_str("<!doctype html><html lang=\"");
        out.push_str(self.lang);
        out.push('"');
        // A built-in's id or `c` and hex digits, a mode's id, a density's id: never text from outside.
        for (name, value) in self.look() {
            out.push(' ');
            out.push_str(name);
            out.push_str("=\"");
            out.push_str(&value);
            out.push('"');
        }
        if uses.has(Uses::CORE) || self.mark_build {
            out.push_str(" data-build=\"");
            out.push_str(assets::BUILD);
            out.push('"');
        }
        out.push_str("><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"color-scheme\" content=\"");
        out.push_str(self.color_scheme());
        out.push_str("\"><title>");
        escape_into(out, &self.title);
        out.push_str("</title>");
        if let Some(seo) = &self.seo {
            seo.write_head(out, &self.title);
        }
        // Geist is preloaded only for a theme that sets its text in Geist.
        if self
            .appearance
            .as_ref()
            .is_none_or(|appearance| appearance.theme().spec().font == Font::Geist)
        {
            out.push_str(
                "<link rel=\"preload\" as=\"font\" type=\"font/woff2\" crossorigin href=\"",
            );
            out.push_str(assets::GEIST_URL);
            out.push_str("\">");
        }
        // The app's fonts are asked for first, so the file is on its way while the sheets load.
        for font in self.app_fonts.iter().filter(|font| font.preloads()) {
            out.push_str(
                "<link rel=\"preload\" as=\"font\" type=\"font/woff2\" crossorigin href=\"",
            );
            out.push_str(&font.url());
            out.push_str("\">");
        }
        out.push_str("<link rel=\"stylesheet\" href=\"");
        out.push_str(assets::NAVREE_UI_URL);
        out.push_str("\">");
        for url in assets::stylesheets(uses) {
            out.push_str("<link rel=\"stylesheet\" href=\"");
            out.push_str(url);
            out.push_str("\">");
        }
        // A theme of your own: its sheet after the toolkit's, so its rules come later.
        for url in self.theme_sheets() {
            out.push_str("<link rel=\"stylesheet\" href=\"");
            out.push_str(&url);
            out.push_str("\" data-ui-theme>");
        }
        // The app's own, last of the sheets: its fonts' rules, then its stylesheets.
        for font in &self.app_fonts {
            out.push_str("<link rel=\"stylesheet\" href=\"");
            out.push_str(&font.face_url());
            out.push_str("\" data-ui-app>");
        }
        for sheet in &self.app_sheets {
            out.push_str("<link rel=\"stylesheet\" href=\"");
            out.push_str(&sheet.url());
            out.push_str("\" data-ui-app>");
        }
        out.push_str("<link rel=\"icon\" href=\"");
        out.push_str(assets::FAVICON_URL);
        out.push_str("\">");
        for url in assets::scripts(uses) {
            out.push_str("<script type=\"module\" src=\"");
            out.push_str(url);
            out.push_str("\"></script>");
        }
        out.push_str(if uses.has(Uses::CORE) {
            "</head><body data-ui-boost>"
        } else {
            "</head><body>"
        });
        render_into(out, self.body_node());
        if found.posts {
            // The one form every post item names: outside any form of the page, so none nests.
            // It is never validated: a field elsewhere that names it, and isn't valid, must not hold
            // back a post that carries none of it.
            let form: Node = el("form")
                .attr("id", POST_FORM)
                .attr("method", "post")
                .flag("novalidate", true)
                .flag("hidden", true)
                .flag("data-ui-form", true)
                .children(
                    self.csrf
                        .as_deref()
                        .map(|token| csrf_field(token.to_owned())),
                )
                .into();
            render_into(out, &form);
        }
        out.push_str("</body></html>");
    }
}

/// How many links a page asks to have fetched as soon as it is quiet. The runtime takes
/// no more, and a page with no script, whose browser follows Speculation Rules, is never
/// asked for more.
const EAGER_MOST: usize = 4;

/// Keeps the first [`EAGER_MOST`] links marked `data-ui-prefetch="eager"`, in the page's
/// order, markup the app wrote itself included; the rest are fetched on hover or focus
/// instead.
fn cap_eager(body: &mut Node) {
    let mut left = EAGER_MOST;
    let mut stack: Vec<&mut Node> = vec![body];
    while let Some(node) = stack.pop() {
        match node {
            Node::Element(element) => {
                for attr in &mut element.attrs {
                    if attr.name == "data-ui-prefetch"
                        && attr.value.as_text().is_some_and(|value| value == "eager")
                    {
                        if left == 0 {
                            attr.value = crate::node::AttrValue::Flag;
                        } else {
                            left -= 1;
                        }
                    }
                }
                stack.extend(element.children.iter_mut().rev());
            }
            Node::Fragment(nodes) => stack.extend(nodes.iter_mut().rev()),
            Node::Raw(html) => {
                if let Some(capped) = cap_eager_markup(html.as_str(), &mut left) {
                    *node = Node::Raw(crate::SafeHtml::trusted(capped));
                }
            }
            Node::Static(html) => {
                if let Some(capped) = cap_eager_markup(html, &mut left) {
                    *node = Node::Raw(crate::SafeHtml::trusted(capped));
                }
            }
            _ => {}
        }
    }
}

/// [`cap_eager`] in markup: `None` when it stays as it is. Every `data-ui-prefetch` with
/// a value counts, whatever the value spells (a browser reads `&#101;ager` as `eager`, a
/// name in any case); past the cap the value goes, and the link is fetched on hover. Only
/// tags are read: text, comments and the text of a script, a style, a title or a
/// textarea are not.
fn cap_eager_markup(html: &str, left: &mut usize) -> Option<String> {
    const MARK: &str = "data-ui-prefetch";
    // The ranges to take out: from the end of the name to the end of the value.
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    let bytes = html.as_bytes();
    let n = bytes.len();
    let space = |at: usize| at < n && bytes[at].is_ascii_whitespace();
    let mut i = 0;
    while i < n {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i + 4..].find("-->").map_or(n, |at| i + 4 + at + 3);
            continue;
        }
        if !bytes.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < n && !space(j) && bytes[j] != b'>' && bytes[j] != b'/' {
            j += 1;
        }
        let tag = html[i + 1..j].to_ascii_lowercase();
        // The attributes, to the end of the tag.
        loop {
            while space(j) || (j < n && bytes[j] == b'/') {
                j += 1;
            }
            if j >= n || bytes[j] == b'>' {
                j += 1;
                break;
            }
            let name = j;
            while j < n && !space(j) && !matches!(bytes[j], b'=' | b'>' | b'/') {
                j += 1;
            }
            let name_end = j.max(name + 1).min(n);
            j = name_end;
            let mut k = j;
            while space(k) {
                k += 1;
            }
            let mut value = "";
            if k < n && bytes[k] == b'=' {
                k += 1;
                while space(k) {
                    k += 1;
                }
                if k < n && (bytes[k] == b'"' || bytes[k] == b'\'') {
                    let quote = bytes[k] as char;
                    match html[k + 1..].find(quote) {
                        Some(at) => {
                            value = &html[k + 1..k + 1 + at];
                            k += at + 2;
                        }
                        None => {
                            value = &html[k + 1..];
                            k = n;
                        }
                    }
                } else {
                    let start = k;
                    while k < n && !space(k) && bytes[k] != b'>' {
                        k += 1;
                    }
                    value = &html[start..k];
                }
                j = k;
            }
            if html[name..name_end].eq_ignore_ascii_case(MARK) && !value.is_empty() {
                if *left == 0 {
                    cuts.push((name_end, j));
                } else {
                    *left -= 1;
                }
            }
        }
        // Text that is never markup runs to its own end tag.
        if matches!(
            tag.as_str(),
            "script" | "style" | "title" | "textarea" | "xmp" | "iframe" | "noembed" | "noframes"
        ) {
            let close = format!("</{tag}");
            j = html[j.min(n)..]
                .to_ascii_lowercase()
                .find(&close)
                .map_or(n, |at| j + at);
        }
        i = j;
    }
    if cuts.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    for (from, to) in cuts {
        out.push_str(&html[at..from]);
        at = to;
    }
    out.push_str(&html[at..]);
    Some(out)
}

/// What a page is made of, for the answer to a client that is not a browser.
#[cfg(feature = "native-preview")]
pub(crate) struct NativeParts<'a> {
    pub(crate) title: &'a str,
    pub(crate) body: &'a mut Node,
    pub(crate) status: StatusCode,
    pub(crate) cache: &'static str,
    pub(crate) cookies: &'a [HeaderValue],
}

#[cfg(feature = "native-preview")]
impl View {
    /// This view, keeping the page it was made from.
    pub(crate) fn with_note(mut self, note: crate::shell::PageNote) -> Self {
        self.note = Some(Box::new(note));
        self
    }

    /// The page in the app's shell this view was made from, when a native
    /// client asked.
    pub(crate) fn note(&self) -> Option<&crate::shell::PageNote> {
        self.note.as_deref()
    }

    /// Takes the view's body out, built when it was kept as a note.
    pub(crate) fn take_body(mut self) -> Node {
        match self.note.take() {
            Some(note) => note.into_chrome(),
            None => std::mem::take(&mut self.body),
        }
    }

    /// The token the page's posts carry.
    pub(crate) fn csrf_token(&self) -> Option<&str> {
        self.csrf.as_deref()
    }

    pub(crate) fn native_parts(&mut self) -> NativeParts<'_> {
        NativeParts {
            title: &self.title,
            body: &mut self.body,
            status: self.status,
            cache: self.cache,
            cookies: &self.cookies,
        }
    }
}

impl IntoResponse for View {
    fn into_response(mut self) -> Response {
        // A native client, or its web view, gets its own answer; a browser's
        // request goes on as it always has.
        #[cfg(feature = "native-preview")]
        {
            self = match crate::native::swap::view(self) {
                Ok(response) => return response,
                Err(view) => view,
            };
            // A view made while a native client asked, given to a browser: the
            // chrome is built now.
            if let Some(note) = self.note.take() {
                self.body = note.into_chrome();
            }
        }
        cap_eager(&mut self.body);
        let uses = self.uses();
        let html = self.to_html();
        let mut response = (
            self.status,
            [
                (
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                ),
                (CONTENT_SECURITY_POLICY, HeaderValue::from_static(PAGE_CSP)),
                (CACHE_CONTROL, HeaderValue::from_static(self.cache)),
            ],
            html,
        )
            .into_response();
        let headers = response.headers_mut();
        if !uses.has(Uses::CORE) {
            headers.insert(
                SPECULATION_RULES,
                HeaderValue::from_static(assets::SPECULATION_HEADER),
            );
        }
        if !self.vary.is_empty()
            && let Ok(vary) = HeaderValue::from_str(&self.vary.join(", "))
        {
            headers.append(VARY, vary);
        }
        for cookie in self.cookies {
            headers.append(SET_COOKIE, cookie);
        }
        // Whether a region's fetch was answered with a page the toolkit drew: see `ViewAnswer`.
        response.extensions_mut().insert(ViewAnswer);
        response
    }
}

/// On a response a [`View`] made. A region's fetch answered with one, successfully, is the
/// region's fragment, whatever its body uses: the reply layer says so in `X-Navree-Fragment`,
/// so the runtime draws it in place rather than taking a page that loads the runtime for a
/// page of its own (somewhere else, such as a sign-in page).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewAnswer;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::Cmd;
    use crate::el;

    #[test]
    fn a_static_page_ships_no_script() {
        let html = View::new("x").body(el("main").child("hi")).to_html();
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("data-build"), "{html}");
        assert!(html.contains(assets::NAVREE_UI_URL));
    }

    #[test]
    fn a_command_loads_the_core_runtime() {
        let html = View::new("x")
            .body(el("button").on_click_one(Cmd::toggle("a")))
            .to_html();
        assert!(html.contains(assets::CORE_URL), "{html}");
        assert!(html.contains("data-ui-boost"), "{html}");
        assert!(
            html.contains(&format!("data-build=\"{}\"", assets::BUILD)),
            "{html}"
        );
    }

    #[test]
    fn menus_post_items_get_one_post_form_after_the_page() {
        use crate::components::{DropdownMenu, MenuItem};
        let menu = |id: &'static str| DropdownMenu::new(id, "A").item(MenuItem::post("Del", "/d"));
        let html = View::new("x")
            .body((
                el("form").attr("action", "/save").child(menu("one")),
                menu("two"),
            ))
            .csrf("tok\"><x>")
            .to_html();
        assert_eq!(html.matches("id=\"ui-post\"").count(), 1, "{html}");
        assert!(
            html.contains(r#"<form id="ui-post" method="post" novalidate hidden data-ui-form><input type="hidden" name="_csrf" value="tok&quot;&gt;&lt;x&gt;"></form></body>"#),
            "{html}"
        );
        // Without a token the form is there and carries none.
        let bare = View::new("x").body(menu("one")).to_html();
        assert!(
            bare.contains(
                r#"<form id="ui-post" method="post" novalidate hidden data-ui-form></form></body>"#
            ),
            "{bare}"
        );
        // A page with no post item draws no form.
        let none = View::new("x")
            .body(el("main").child("hi"))
            .csrf("tok")
            .to_html();
        assert!(!none.contains("ui-post"), "{none}");
    }

    #[test]
    fn a_page_says_its_look_as_it_writes_it() {
        use crate::theme::{Mode, ThemeId};
        let plain = View::new("x");
        assert!(plain.look().is_empty());
        assert_eq!(plain.color_scheme(), "dark");
        assert!(plain.own_themes().is_empty());

        let ocean = View::new("x").appearance(Appearance::new(Theme::OCEAN, Mode::Light));
        assert_eq!(
            ocean.look(),
            vec![
                ("data-theme", "ocean".to_owned()),
                ("data-mode", "light".to_owned())
            ]
        );
        assert_eq!(ocean.color_scheme(), "light");
        assert!(ocean.to_html().starts_with(
            r#"<!doctype html><html lang="en" data-theme="ocean" data-mode="light">"#
        ));
        // A built-in has no sheet of its own.
        assert!(ocean.own_themes().is_empty());

        let mine = Theme::custom(
            ThemeId::parse("sutlej").unwrap(),
            "Sutlej",
            *Theme::VIOLET.spec(),
        )
        .unwrap();
        let page = View::new("x")
            .appearance(Appearance::new(mine.clone(), Mode::Dark))
            .theme_sheet(&mine)
            .density(Density::Compact);
        assert_eq!(page.own_themes().len(), 1, "each theme once");
        assert_eq!(page.own_themes()[0].key(), mine.key());
        assert_eq!(
            page.look().last(),
            Some(&("data-density", "compact".to_owned()))
        );
        let html = page.to_html();
        let url = mine.stylesheet().unwrap();
        assert_eq!(html.matches(url.as_str()).count(), 1, "{html}");
        for (name, value) in page.look() {
            assert!(html.contains(&format!(" {name}=\"{value}\"")), "{html}");
        }
    }

    #[test]
    fn the_apps_own_sheets_and_fonts_come_after_the_toolkits_and_the_themes() {
        use crate::app_files::{AppFont, AppSheet};
        use crate::theme::{Mode, Rgb, ThemeId};
        static BRAND: AppSheet = AppSheet::new("brand", b".hero{color:var(--accent)}", 1024);
        static ART: AppSheet = AppSheet::new("art", b".blob{opacity:.5}", 1024);
        static DISPLAY: AppFont =
            AppFont::new("display", "Fraunces", b"wOF2\0\0\0\0", 1024).weights(400, 700);
        static BODY: AppFont = AppFont::new("body", "Inter Tight", b"wOF2\0\0\0\0", 1024).lazy();

        let mine = Theme::custom(
            ThemeId::parse("brand").unwrap(),
            "Brand",
            Theme::NAVREE
                .spec()
                .with_accent(Rgb::hex(0x2dd4bf), Rgb::hex(0x0f766e)),
        )
        .unwrap();
        let html = View::new("x")
            .appearance(Appearance::new(mine.clone(), Mode::Light))
            .stylesheet(&BRAND)
            .font(&DISPLAY)
            .stylesheet(&ART)
            .stylesheet(&BRAND)
            .font(&DISPLAY)
            .font(&BODY)
            .to_html();
        let at = |needle: &str| {
            html.find(needle)
                .unwrap_or_else(|| panic!("{needle} in {html}"))
        };
        // Once each, in the order they were added, after the theme's sheet and before the icon.
        for url in [BRAND.url(), ART.url(), DISPLAY.face_url(), BODY.face_url()] {
            assert_eq!(html.matches(url.as_str()).count(), 1, "{url}");
        }
        let theme = at(&mine.stylesheet().unwrap());
        let (display, body) = (at(&DISPLAY.face_url()), at(&BODY.face_url()));
        let (brand, art) = (at(&BRAND.url()), at(&ART.url()));
        let icon = at("rel=\"icon\"");
        assert!(theme < display && display < body, "fonts after the theme");
        assert!(
            body < brand && brand < art && art < icon,
            "sheets after fonts"
        );
        // The file of a font that is preloaded is asked for before any sheet; a lazy one is not.
        let preload = at(&format!(
            "rel=\"preload\" as=\"font\" type=\"font/woff2\" crossorigin href=\"{}\"",
            DISPLAY.url()
        ));
        assert!(preload < at(assets::NAVREE_UI_URL), "{html}");
        assert!(
            !html.contains(&format!("href=\"{}\"", BODY.url())),
            "{html}"
        );
        // The page lists what it links, for a static copy of it.
        let page = View::new("x").stylesheet(&BRAND).font(&DISPLAY);
        assert_eq!(page.app_sheets(), &[BRAND]);
        assert_eq!(page.app_fonts(), &[DISPLAY]);
        // Nothing inline: the policy allows files of the page's own origin only.
        assert!(!html.contains("<style"), "{html}");
        assert!(!html.contains(" style="), "{html}");
    }

    #[test]
    fn the_title_is_escaped() {
        let html = View::new("<x>").to_html();
        assert!(html.contains("<title>&lt;x&gt;</title>"), "{html}");
    }

    #[test]
    fn responses_carry_the_policy() {
        let response = View::new("x").into_response();
        assert_eq!(response.headers()[CONTENT_SECURITY_POLICY], PAGE_CSP);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        assert_eq!(
            response.headers()["speculation-rules"],
            assets::SPECULATION_HEADER
        );
    }

    #[test]
    fn no_inline_code_is_allowed_by_the_policy() {
        assert!(!PAGE_CSP.contains("unsafe"));
    }
}
