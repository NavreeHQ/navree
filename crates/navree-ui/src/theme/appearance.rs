//! How a page looks for this request: a theme and a mode, from the app's
//! default, the workspace's, the user's and the URL, in that order.

use http::StatusCode;
use http::header::{COOKIE, HOST, HeaderMap, HeaderValue, LOCATION, ORIGIN, SET_COOKIE};
use navree_http::extract::urlencoded;
use navree_http::response::{IntoResponse, Response};

use super::Theme;
use crate::html::LocalHref;

/// The cookie holding a reader's own theme and mode, when the app keeps them in
/// the browser: `dark.ocean`, or `light.v1-...` for a theme of the app's own.
pub const THEME_COOKIE: &str = "navree_ui_theme";

/// Where [`ThemeMenu`](super::ThemeMenu) posts by default: the toolkit sets the
/// cookie and sends the reader back.
pub const SETTINGS_PATH: &str = "/_navree/ui/appearance";

/// Light, dark, or whatever the device prefers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Always light.
    Light,
    /// Always dark: the toolkit's default.
    #[default]
    Dark,
    /// The device's choice, followed live as it changes.
    System,
}

impl Mode {
    /// Every mode, as a menu lists them.
    pub const ALL: [Self; 3] = [Self::Light, Self::Dark, Self::System];

    /// The `data-mode` value: `light`, `dark` or `system`.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
        }
    }

    /// The name shown.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::System => "System",
        }
    }

    /// The `<meta name="color-scheme">` value.
    pub const fn color_scheme(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "light dark",
        }
    }

    /// The mode with the id `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.id() == id)
    }
}

/// A theme and a mode: how a page looks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Appearance {
    theme: Theme,
    mode: Mode,
}

impl Appearance {
    /// `theme` in `mode`.
    pub fn new(theme: Theme, mode: Mode) -> Self {
        Self { theme, mode }
    }

    /// The theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// The mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The same with another theme.
    #[must_use]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// The same with another mode.
    #[must_use]
    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// The text to store for it, in a column or a cookie: `dark.ocean`.
    /// [`Appearance::parse`] reads it back.
    pub fn to_value(&self) -> String {
        format!("{}.{}", self.mode.id(), self.theme.short_form())
    }

    /// Reads [`to_value`](Self::to_value)'s text. A theme of your own comes back
    /// with the id `custom`; match it to yours by [`Theme::key`].
    pub fn parse(value: &str) -> Option<Self> {
        let setting = ThemeSetting::parse(value)?;
        Some(Self {
            theme: setting.theme?,
            mode: setting.mode?,
        })
    }

    /// This look with the reader's own choice laid over it: the theme and mode kept in the
    /// cookie that the theme menu and the theme toggle set, then `?theme=` and `?mode=` in
    /// `query`, which last for this request only. For a page outside the app shell, such
    /// as a public site's, whose [`AppShell`](crate::AppShell) would do this for it.
    ///
    /// A page drawn this way differs from reader to reader at one address: cache it as
    /// `private`, or not at all.
    ///
    /// ```
    /// use navree_ui::theme::{Appearance, Mode, Theme};
    ///
    /// let mut headers = http::HeaderMap::new();
    /// headers.insert(http::header::COOKIE, "navree_ui_theme=light".parse().unwrap());
    /// let site = Appearance::new(Theme::NAVREE, Mode::System);
    /// assert_eq!(site.clone().for_reader(&headers, None).mode(), Mode::Light);
    /// assert_eq!(site.for_reader(&headers, Some("mode=dark")).mode(), Mode::Dark);
    /// ```
    #[must_use]
    pub fn for_reader(mut self, headers: &HeaderMap, query: Option<&str>) -> Self {
        let (cookie, url) = read_setting(headers, query);
        cookie.over(&mut self, false);
        url.over(&mut self, false);
        self
    }

    /// Whether this is what a page gets with nothing set: the default theme, dark.
    pub fn is_default(&self) -> bool {
        self.theme == Theme::NAVREE && self.mode == Mode::Dark
    }
}

/// What one level says about how pages look: a theme, a mode, both or neither.
/// A locked setting keeps its theme from the levels after it; they can still
/// choose the mode.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemeSetting {
    /// The theme, if this level chooses one.
    pub theme: Option<Theme>,
    /// The mode, if this level chooses one.
    pub mode: Option<Mode>,
    /// Whether the levels after this one keep its theme.
    pub locked: bool,
}

impl ThemeSetting {
    /// Nothing chosen.
    pub fn none() -> Self {
        Self::default()
    }

    /// A theme.
    pub fn theme(theme: Theme) -> Self {
        Self {
            theme: Some(theme),
            ..Self::default()
        }
    }

    /// A mode.
    pub fn mode(mode: Mode) -> Self {
        Self {
            mode: Some(mode),
            ..Self::default()
        }
    }

    /// Both.
    pub fn appearance(appearance: Appearance) -> Self {
        Self {
            theme: Some(appearance.theme),
            mode: Some(appearance.mode),
            locked: false,
        }
    }

    /// Keeps this level's theme from the levels after it.
    #[must_use]
    pub fn locked(mut self) -> Self {
        self.locked = true;
        self
    }

    /// Reads `mode.theme`, `mode` alone, or `.theme` alone.
    pub fn parse(value: &str) -> Option<Self> {
        if value.len() > super::codec::FORM_LEN + 16 {
            return None;
        }
        let (mode, theme) = value.split_once('.').unwrap_or((value, ""));
        let mode = match mode {
            "" => None,
            id => Some(Mode::from_id(id)?),
        };
        let theme = match theme {
            "" => None,
            form => Some(Theme::from_short_form(form)?),
        };
        (mode.is_some() || theme.is_some()).then_some(Self {
            theme,
            mode,
            locked: false,
        })
    }

    /// `base` with what this level says laid on top: its mode always, its theme
    /// unless an earlier level is locked.
    pub(crate) fn over(&self, base: &mut Appearance, locked: bool) {
        if let Some(mode) = self.mode {
            base.mode = mode;
        }
        if !locked && let Some(theme) = &self.theme {
            base.theme = theme.clone();
        }
    }
}

/// Why a choice posted to the settings route was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChoiceError {
    /// The post came from another site.
    CrossSite,
    /// The theme or mode isn't one.
    Invalid,
}

/// The reader's own setting from the cookie and the URL: the cookie first, then
/// `?theme=` and `?mode=`, which last only for this request.
pub(crate) fn read_setting(
    headers: &HeaderMap,
    query: Option<&str>,
) -> (ThemeSetting, ThemeSetting) {
    let mut cookie = ThemeSetting::none();
    for value in headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
    {
        for pair in value.split(';') {
            if let Some((THEME_COOKIE, text)) = pair.trim().split_once('=')
                && let Some(setting) = ThemeSetting::parse(text)
            {
                cookie = setting;
            }
        }
    }
    let mut url = ThemeSetting::none();
    if let Some(query) = query {
        for (name, value) in urlencoded::parse(query.as_bytes()).take(64) {
            match &*name {
                "theme" => url.theme = Theme::from_short_form(&value),
                "mode" => url.mode = Mode::from_id(&value),
                _ => {}
            }
        }
    }
    (cookie, url)
}

/// The `Set-Cookie` that keeps `appearance` in the browser for a year. The
/// toolkit's own route adds `Secure` when the request came over HTTPS.
pub fn appearance_cookie(appearance: &Appearance) -> HeaderValue {
    cookie(appearance, false)
}

fn cookie(appearance: &Appearance, secure: bool) -> HeaderValue {
    // The value is a mode id, a dot and a built-in id or a short form: only
    // `[0-9a-z.-]`, which a header always carries.
    HeaderValue::from_str(&format!(
        "{THEME_COOKIE}={}; Path=/; Max-Age=31536000; SameSite=Lax; HttpOnly{}",
        appearance.to_value(),
        if secure { "; Secure" } else { "" }
    ))
    .unwrap_or(HeaderValue::from_static(
        "navree_ui_theme=; Path=/; Max-Age=0",
    ))
}

/// Whether a post comes from this site: the browser's `Sec-Fetch-Site`, else the
/// `Origin` against the `Host`. A client that sends neither (an old browser, a
/// script) is let through: what it changes is its own cookie.
fn same_site(headers: &HeaderMap) -> bool {
    if let Some(site) = headers.get("sec-fetch-site") {
        return matches!(site.to_str(), Ok("same-origin" | "none"));
    }
    match (headers.get(ORIGIN), headers.get(HOST)) {
        (Some(origin), Some(host)) => origin
            .to_str()
            .ok()
            .and_then(|origin| origin.split_once("://"))
            .is_some_and(|(_, rest)| host.to_str().is_ok_and(|host| host == rest)),
        (Some(_), None) => false,
        (None, _) => true,
    }
}

/// Whether the request came over HTTPS: the page's `Origin`, else what a proxy in
/// front says (`X-Forwarded-Proto`, or `proto=` in `Forwarded`). The first value
/// of each is the client's own.
fn over_https(headers: &HeaderMap) -> bool {
    let first = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(|value| value.trim().to_ascii_lowercase())
    };
    if let Some(origin) = first("origin").filter(|origin| origin != "null") {
        return origin.starts_with("https://");
    }
    if let Some(proto) = first("x-forwarded-proto") {
        return proto == "https";
    }
    first("forwarded").is_some_and(|element| {
        element.split(';').any(|pair| {
            pair.trim().split_once('=').is_some_and(|(name, value)| {
                name.trim() == "proto" && value.trim_matches('"') == "https"
            })
        })
    })
}

/// What the settings route does with a posted form: the cookie to set and where
/// to go, or why not.
pub(crate) fn decide(
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(Appearance, String), ChoiceError> {
    if !same_site(headers) {
        return Err(ChoiceError::CrossSite);
    }
    let (mut theme, mut mode, mut back) = (None, None, None);
    for (name, value) in urlencoded::parse(body).take(64) {
        match &*name {
            "theme" => theme = Some(Theme::from_short_form(&value).ok_or(ChoiceError::Invalid)?),
            "mode" => mode = Some(Mode::from_id(&value).ok_or(ChoiceError::Invalid)?),
            "back" => {
                back = LocalHref::parse(value.trim())
                    .ok()
                    .map(|href| href.as_str().to_owned())
                    .filter(|href| !href.is_empty())
            }
            _ => {}
        }
    }
    let (cookie, _) = read_setting(headers, None);
    let mut appearance = Appearance::default();
    cookie.over(&mut appearance, false);
    if let Some(theme) = theme {
        appearance.theme = theme;
    }
    if let Some(mode) = mode {
        appearance.mode = mode;
    }
    Ok((appearance, back.unwrap_or_else(|| "/".to_owned())))
}

/// Answers `POST /_navree/ui/appearance`: sets the cookie and sends the reader
/// back with a 303, so it works as a plain form with JavaScript off.
pub(crate) fn respond_setting(headers: &HeaderMap, body: &[u8]) -> Response {
    match decide(headers, body) {
        Ok((appearance, back)) => {
            let location = HeaderValue::from_str(&back).unwrap_or(HeaderValue::from_static("/"));
            (
                StatusCode::SEE_OTHER,
                [
                    (LOCATION, location),
                    (SET_COOKIE, cookie(&appearance, over_https(headers))),
                ],
            )
                .into_response()
        }
        Err(ChoiceError::CrossSite) => StatusCode::FORBIDDEN.into_response(),
        // Input that isn't a choice is a 400, as every input error is in Navree.
        Err(ChoiceError::Invalid) => StatusCode::BAD_REQUEST.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ThemeId, ThemeSpec};

    fn custom() -> Theme {
        Theme::custom(
            ThemeId::parse("agra").unwrap(),
            "Agra",
            *Theme::OCEAN.spec(),
        )
        .unwrap()
    }

    #[test]
    fn an_appearance_reads_back_from_its_value() {
        for theme in Theme::BUILT_IN {
            for mode in Mode::ALL {
                let appearance = Appearance::new(theme.clone(), mode);
                assert_eq!(Appearance::parse(&appearance.to_value()), Some(appearance));
            }
        }
        let mine = Appearance::new(custom(), Mode::Light);
        let back = Appearance::parse(&mine.to_value()).unwrap();
        assert_eq!(back.theme().key(), custom().key());
        assert_eq!(back.mode(), Mode::Light);
        for bad in [
            "",
            ".",
            "dark",
            "dark.",
            ".ocean",
            "Dark.ocean",
            "dark.Ocean",
            "dark.ocean.x",
            "dim.ocean",
            "dark.v1-x",
        ] {
            assert_eq!(Appearance::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_setting_reads_either_half() {
        assert_eq!(
            ThemeSetting::parse("light"),
            Some(ThemeSetting::mode(Mode::Light))
        );
        assert_eq!(
            ThemeSetting::parse(".rose"),
            Some(ThemeSetting::theme(Theme::ROSE))
        );
        assert_eq!(ThemeSetting::parse(""), None);
        assert_eq!(ThemeSetting::parse("light.nope"), None);
        assert_eq!(ThemeSetting::parse(&"x".repeat(5000)), None);
    }

    #[test]
    fn a_short_form_that_fails_contrast_is_no_theme() {
        let mut spec: ThemeSpec = *Theme::NAVREE.spec();
        spec.dark
            .set(crate::theme::Token::Fg, crate::theme::Rgb::hex(0x111111));
        let draft = Theme::unchecked(ThemeId::custom(), String::new(), spec);
        assert_eq!(Theme::from_short_form(draft.short_form()), None);
    }

    #[test]
    fn levels_lay_over_each_other_and_a_lock_keeps_the_theme() {
        let mut look = Appearance::default();
        ThemeSetting::theme(Theme::OCEAN).over(&mut look, false);
        assert_eq!(look, Appearance::new(Theme::OCEAN, Mode::Dark));
        ThemeSetting::appearance(Appearance::new(Theme::ROSE, Mode::Light)).over(&mut look, true);
        assert_eq!(
            look,
            Appearance::new(Theme::OCEAN, Mode::Light),
            "locked: the mode only"
        );
    }

    #[test]
    fn the_cookie_and_the_url_are_read_strictly() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("a=1; navree_ui_theme=light.ocean; b=2"),
        );
        let (cookie, url) = read_setting(&headers, Some("mode=system&theme=rose&x=1"));
        assert_eq!(cookie.theme, Some(Theme::OCEAN));
        assert_eq!(cookie.mode, Some(Mode::Light));
        assert_eq!(url.theme, Some(Theme::ROSE));
        assert_eq!(url.mode, Some(Mode::System));
        for hostile in [
            "navree_ui_theme=dark.ocean\"><script>",
            "navree_ui_theme=dark.ocean}body{x}",
            "navree_ui_theme=dark.ocean%3B",
            "navree_ui_theme=__proto__",
            "navree_ui_theme=dark.v1-;}",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(COOKIE, HeaderValue::from_str(hostile).unwrap());
            let (cookie, _) = read_setting(&headers, Some("theme=%3C%2Fstyle%3E&mode=%22"));
            assert_eq!(cookie.theme, None, "{hostile}");
        }
        let (_, url) = read_setting(&HeaderMap::new(), Some("theme=%3C%2Fstyle%3E&mode=%22"));
        assert_eq!(url, ThemeSetting::none());
    }

    #[test]
    fn the_settings_route_sets_the_cookie_and_goes_back_only_within_the_site() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
        let response = respond_setting(
            &headers,
            b"theme=ocean&mode=system&back=%2Fdeals%3Fpage%3D2",
        );
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/deals?page=2");
        let cookie = response.headers()[SET_COOKIE].to_str().unwrap();
        assert!(
            cookie.starts_with("navree_ui_theme=system.ocean; Path=/;"),
            "{cookie}"
        );
        assert!(
            cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"),
            "{cookie}"
        );
        // Off the site, or a scheme: home instead.
        for back in [
            "https://evil.example/",
            "//evil.example",
            "javascript:alert(1)",
            "\\\\evil",
        ] {
            let body = format!("theme=ocean&back={}", urlencode(back));
            let response = respond_setting(&headers, body.as_bytes());
            assert_eq!(response.headers()[LOCATION], "/", "{back}");
        }
        // The mode alone keeps the theme the cookie had.
        headers.insert(
            COOKIE,
            HeaderValue::from_static("navree_ui_theme=dark.rose"),
        );
        let response = respond_setting(&headers, b"mode=light");
        assert!(
            response.headers()[SET_COOKIE]
                .to_str()
                .unwrap()
                .starts_with("navree_ui_theme=light.rose;")
        );
        // Bad values are refused, not stored.
        for body in ["theme=nope", "mode=dim", "theme=%3Cscript%3E"] {
            assert_eq!(
                respond_setting(&headers, body.as_bytes()).status(),
                StatusCode::BAD_REQUEST,
                "{body}"
            );
        }
    }

    #[test]
    fn a_post_from_another_site_is_refused() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        assert_eq!(
            respond_setting(&headers, b"theme=ocean").status(),
            StatusCode::FORBIDDEN
        );
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, HeaderValue::from_static("https://evil.example"));
        headers.insert(HOST, HeaderValue::from_static("app.example"));
        assert_eq!(
            respond_setting(&headers, b"theme=ocean").status(),
            StatusCode::FORBIDDEN
        );
        headers.insert(ORIGIN, HeaderValue::from_static("https://app.example"));
        assert_eq!(
            respond_setting(&headers, b"theme=ocean").status(),
            StatusCode::SEE_OTHER
        );
    }

    #[test]
    fn the_cookie_is_secure_over_https_and_an_empty_back_goes_home() {
        let post = |pairs: &[(&'static str, &'static str)], body: &str| {
            let mut headers = HeaderMap::new();
            for (name, value) in pairs {
                headers.insert(*name, HeaderValue::from_static(value));
            }
            respond_setting(&headers, body.as_bytes())
        };
        let cookie =
            |response: &Response| response.headers()[SET_COOKIE].to_str().unwrap().to_owned();
        for plain in [
            &[][..],
            &[("origin", "http://app.example"), ("host", "app.example")][..],
            &[("x-forwarded-proto", "http")][..],
        ] {
            let response = post(plain, "theme=ocean");
            assert!(!cookie(&response).contains("Secure"), "{plain:?}");
        }
        for https in [
            &[("origin", "https://app.example"), ("host", "app.example")][..],
            &[("x-forwarded-proto", "https")][..],
            &[("x-forwarded-proto", "HTTPS, http")][..],
            &[("forwarded", "for=192.0.2.1;proto=https;by=203.0.113.4")][..],
        ] {
            let response = post(https, "theme=ocean");
            assert_eq!(response.status(), StatusCode::SEE_OTHER, "{https:?}");
            assert!(cookie(&response).ends_with("; Secure"), "{https:?}");
        }
        for body in ["theme=ocean&back=", "theme=ocean&back=%20", "theme=ocean"] {
            assert_eq!(post(&[], body).headers()[LOCATION], "/", "{body}");
        }
    }

    fn urlencode(text: &str) -> String {
        text.bytes().map(|b| format!("%{b:02X}")).collect()
    }
}
