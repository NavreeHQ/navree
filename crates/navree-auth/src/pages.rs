//! [`AuthPages`] (feature `ui`): the pages a browser signs in, up and out with,
//! drawn with navree-ui on the session ([`AuthService::sign_in_session`]).

mod account;
mod passkey;
mod passwordless;

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use http::request::Parts;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use navree_contracts::AuthState;
use navree_db::Uow;
use navree_http::ModuleBuilderHttpExt;
use navree_http::extract::{Query, RawQuery};
use navree_http::{ApiMeta, AppError, Controller, IntoResponse, Response, RouteDef};
use navree_kernel::{
    BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_session::Session;
use navree_ui::form::{FieldErrors, FormView, Rejected};
use navree_ui::pages::{PagesOptions, ReturnTo};
use navree_ui::theme::{Appearance, Theme, ThemeMenu};
use navree_ui::{
    Alert, AlertTone, AppFont, AppSheet, AuthLayout, Card, Form, LocalHref, Node, Reply, Shell,
    Stack, Submit, Text, Validate, View, el,
};
use serde::Deserialize;

use crate::session_auth::{SIGN_OUT_PATH, TWO_FACTOR_SETUP_PATH};
use crate::{AuthError, AuthService, ClientMeta, NewUser, SessionSignIn, TwoFactorProof};

/// How many forgotten-password requests one client may send per minute.
const FORGOT_PER_MINUTE: u32 = 5;

/// How many sign-ups one client may send per minute.
const SIGN_UP_PER_MINUTE: u32 = 5;

/// The pages a browser signs in, up and out with, on the session cookie, drawn
/// with navree-ui: sign-in (with "remember me"), sign-up when asked for, the
/// second factor, forgotten and reset password, and sign-out. A module: add it
/// next to `AuthModule`, `SessionModule` and `UiModule`.
///
/// | Route | What it does |
/// |---|---|
/// | `GET /sign-in?next=` | the sign-in form (a signed-in browser goes on to `next`) |
/// | `POST /sign-in` | [`AuthService::sign_in_session`]: on to `next`, or to the second factor |
/// | `GET`, `POST /sign-in/two-factor` | the code from the authenticator app, or a recovery code ([`AuthService::verify_two_factor_session`]) |
/// | `GET`, `POST /forgot-password` | mails a reset link ([`AuthService::send_password_reset_link`]); always the same answer |
/// | `GET /reset-password?token=&email=`, `POST /reset-password` | sets the new password ([`AuthService::reset_password`]), which signs every session out |
/// | `GET`, `POST /sign-up` | with [`sign_up`](Self::sign_up) and `auth.registration` on: a new account ([`AuthService::register`]), signed in on this browser |
/// | `POST /sign-out` | [`AuthService::sign_out_session`], then the sign-in page, or [`after_sign_out`](Self::after_sign_out) |
/// | `GET`, `POST /account/two-factor`, `POST /account/two-factor/confirm` | sets two-factor authentication up: the secret as a picture and a key, the first code, the recovery codes shown once ([`AuthService::enable_two_factor`], [`AuthService::confirm_two_factor`]); a user a policy requires it of is sent here after signing in |
/// | `POST /impersonate/stop` | an impersonating administrator goes back to their own sign-in ([`AuthService::stop_impersonating_session`]) |
/// | `GET`, `POST /sign-in/link`, `GET`, `POST /sign-in/link/verify` | when `auth.magic_link.enabled`: mails a link; the link opens a page with one button that signs in ([`AuthService::login_with_magic_link_session`]); set `auth.magic_link.url` to `https://app.example.com/sign-in/link/verify` |
/// | `GET`, `POST /sign-in/code`, `POST /sign-in/code/verify` | when `auth.otp.enabled`: mails a code, then asks for it ([`AuthService::login_with_code_session`]) |
/// | `GET /sign-in/passkey`, `/sign-in/passkey.js`, `POST /sign-in/passkey/options`, `/sign-in/passkey/verify` | when `auth.passkeys.enabled`: the page, its script and the two JSON routes of the ceremony ([`AuthService::login_with_passkey_session`]) |
///
/// Every post carries the session's CSRF token (the forms hold it as
/// `_csrf`; navree-session checks it), every page works with JavaScript
/// off, and a `next` only ever leads to a page of this site. The sign-in
/// page is where navree-ui's [`SignInRequired`](navree_ui::SignInRequired)
/// and its 401 page send people: by default it follows `ui.sign_in`
/// (navree-ui's `PagesOptions`), and after signing in a browser goes to
/// `next`, else to `ui.home`. Point `auth.password_reset.url` at the reset
/// page (`https://app.example.com/reset-password`) so the reset mail links
/// to it.
///
/// The pages are drawn in the app's look: the registered [`Shell`]'s default
/// appearance, its stylesheets and its fonts, with the reader's own choice of
/// theme and mode laid over them; [`appearance`](Self::appearance),
/// [`stylesheet`](Self::stylesheet) and [`font`](Self::font) set them for an app
/// with no shell or a look of their own. [`aside`](Self::aside) puts the app's
/// panel beside the form and [`theme_menu`](Self::theme_menu) a menu of light
/// and dark in the corner ([`AuthLayout`]).
///
/// ```no_run
/// use navree_auth::{AuthModule, AuthPages};
/// use navree_kernel::Navree;
/// use navree_session::SessionModule;
/// use navree_ui::UiModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     // ConfigModule, CryptoModule, HttpModule, DatabaseModule ...
///     .module(SessionModule::from_config())
///     .module(AuthModule::from_config())
///     .module(UiModule)
///     .module(AuthPages::new().app_name("Agra CRM").sign_up().after_sign_out("/"))
///     .run()
/// # }
/// ```
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct AuthPages {
    app_name: Option<String>,
    sign_in: Option<String>,
    home: Option<String>,
    sign_up: bool,
    sign_up_path: Option<String>,
    after_sign_up: Option<String>,
    after_sign_out: Option<String>,
    appearance: Option<Appearance>,
    sheets: Vec<AppSheet>,
    fonts: Vec<AppFont>,
    aside: Option<Node>,
    theme_menu: bool,
    dress: Option<fn(View) -> View>,
}

impl AuthPages {
    /// The pages at their default paths.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the sign-up page, `GET` and `POST /sign-up` ([`sign_up_path`](Self::sign_up_path)
    /// moves it), when `auth.registration` is on: a name, an email address and a password
    /// typed twice, checked by [`AuthService::register`] (a unique address, the password
    /// policy, the sign-in policies), then the new account signed in on this browser and
    /// sent on to `next`, [`after_sign_up`](Self::after_sign_up) or home. The sign-in page
    /// links to it. Its post carries the session's CSRF token like every other, and a client
    /// may send at most five a minute.
    pub fn sign_up(mut self) -> Self {
        self.sign_up = true;
        self
    }

    /// Where the sign-up page is, in place of `/sign-up` (a path on this site). It turns
    /// the page on, as [`sign_up`](Self::sign_up) does.
    pub fn sign_up_path(mut self, path: impl Into<String>) -> Self {
        self.sign_up = true;
        self.sign_up_path = Some(path.into());
        self
    }

    /// Where a browser goes once its new account is made and signed in, when it has no
    /// `next`: a welcome page, in place of the home page.
    pub fn after_sign_up(mut self, path: impl Into<String>) -> Self {
        self.after_sign_up = Some(path.into());
        self
    }

    /// Where a browser goes after signing out (a path on this site, such as the public
    /// site's home page), in place of the sign-in page with "You are signed out".
    pub fn after_sign_out(mut self, path: impl Into<String>) -> Self {
        self.after_sign_out = Some(path.into());
        self
    }

    /// How the pages look before the reader chooses: a theme of the app's own and a mode.
    /// The reader's choice (the theme cookie, `?theme=`, `?mode=`) lies over it. Without it
    /// the pages take the default look of the app's [`Shell`], when one is registered, and
    /// keep to the themes it offers.
    pub fn appearance(mut self, appearance: Appearance) -> Self {
        self.appearance = Some(appearance);
        self
    }

    /// Links a stylesheet of the app's own on every page, after the registered
    /// [`Shell`]'s. See [`View::stylesheet`].
    pub fn stylesheet(mut self, sheet: &AppSheet) -> Self {
        if !self.sheets.contains(sheet) {
            self.sheets.push(*sheet);
        }
        self
    }

    /// Links a typeface of the app's own on every page, after the registered [`Shell`]'s.
    /// See [`View::font`].
    pub fn font(mut self, font: &AppFont) -> Self {
        if !self.fonts.contains(font) {
            self.fonts.push(*font);
        }
        self
    }

    /// Puts `aside` beside the form on wide screens: the app's mark, a line about it, a
    /// picture. On a phone the form stands alone. See [`AuthLayout`].
    pub fn aside(mut self, aside: impl Into<Node>) -> Self {
        self.aside = Some(aside.into());
        self
    }

    /// Puts a [`ThemeMenu`] in the pages' top corner, so a reader can choose light or dark
    /// before signing in (and a theme, when the app offers more than one). On when the
    /// registered [`Shell`] has one.
    pub fn theme_menu(mut self) -> Self {
        self.theme_menu = true;
        self
    }

    /// Hands every page to `dress` last, to set what the options above don't: its
    /// language, its density, another sheet.
    pub fn view(mut self, dress: fn(View) -> View) -> Self {
        self.dress = Some(dress);
        self
    }

    /// The app's name, shown above every form.
    pub fn app_name(mut self, name: impl Into<String>) -> Self {
        self.app_name = Some(name.into());
        self
    }

    /// Where the sign-in page is, in place of `ui.sign_in` (a path on this
    /// site, such as `/login`). The two-factor page is below it.
    pub fn sign_in_path(mut self, path: impl Into<String>) -> Self {
        self.sign_in = Some(path.into());
        self
    }

    /// Where a browser goes after signing in when it has no `next`, in place
    /// of `ui.home`.
    pub fn home(mut self, path: impl Into<String>) -> Self {
        self.home = Some(path.into());
        self
    }
}

impl Module for AuthPages {
    fn register(&self, m: &mut ModuleBuilder) {
        let pages = self.clone();
        m.factory(Factory::<AuthPages>::new(move |_| {
            let pages = pages.clone();
            async move { Ok(Arc::new(pages)) }
        }))
        .controller::<PagesController>();
        // A native client reads where the hand-over starts, where its code is exchanged,
        // where it comes back and where a second step is finished from the shell document.
        let pages = self.clone();
        m.multi(
            Factory::<dyn navree_ui::NativeShellPart>::new(move |r: Resolver| {
                let pages = pages.clone();
                async move {
                    let app = r.app();
                    let apps = r.get::<crate::AppsOptions>().await?;
                    let options = r.get::<crate::AuthOptions>().await?;
                    let sessions = app
                        .try_get::<navree_session::SessionOptions>()
                        .await
                        .is_some();
                    let api_prefix = match app.try_get::<navree_http::HttpOptions>().await {
                        Some(http) => http.api_prefix.clone(),
                        None => String::new(),
                    };
                    let ui = app.try_get::<PagesOptions>().await;
                    let paths = Paths::new(&pages, ui.as_deref());
                    Ok(Arc::new(HandOverShellPart {
                        // The routes are mounted only where sessions are and an app can take part.
                        handed_over: sessions && apps.has_hand_over(),
                        apps,
                        exchange: format!(
                            "/{}/{}/app/exchange",
                            api_prefix.trim_matches('/'),
                            options.routes_prefix.trim_matches('/')
                        )
                        .replace("//", "/"),
                        second_step: paths.two_factor,
                    })
                        as Arc<dyn navree_ui::NativeShellPart>)
                }
            })
            .deps(vec![
                ProviderKey::of::<crate::AppsOptions>(),
                ProviderKey::of::<crate::AuthOptions>(),
            ]),
        );
    }
}

/// What the shell document says of signing in from a native app: the hand-over's routes,
/// the callback of the app that asks and the page of a second step.
struct HandOverShellPart {
    handed_over: bool,
    apps: Arc<crate::AppsOptions>,
    exchange: String,
    second_step: String,
}

impl navree_ui::NativeShellPart for HandOverShellPart {
    fn contribute(
        &self,
        shell: &mut navree_ui::NativeShellParts,
        client: &navree_ui::NativeClient,
    ) {
        use navree_ui::NativeFamily;
        if !self.handed_over {
            return;
        }
        let mut routes = navree_ui::NativeSignIn::new(
            crate::hand_over::APP_SIGN_IN_PATH,
            self.exchange.as_str(),
        )
        .second_step(self.second_step.as_str());
        let family = match client.family {
            NativeFamily::Apple => Some("apple"),
            NativeFamily::Android => Some("android"),
            _ => None,
        };
        if let Some(callback) = family.and_then(|family| self.apps.first_callback(family)) {
            routes = routes.callback(callback);
        }
        shell.sign_in(routes);
    }
}

/// Whether `path` is a path on this site (`/x`, not `//x`, no `\`).
fn local(path: &str) -> bool {
    ReturnTo::parse(path).is_some() && !path.contains('?')
}

/// The paths and words of the pages, settled at boot.
#[derive(Clone, Debug)]
struct Paths {
    app_name: Option<String>,
    sign_in: String,
    two_factor: String,
    forgot: String,
    reset: String,
    home: String,
    magic_link: String,
    code: String,
    passkey: String,
    setup: String,
    /// The sign-up page, when it is on.
    sign_up: Option<String>,
    /// Where a new account goes without a `next`.
    after_sign_up: String,
    /// Where a browser goes after signing out.
    after_sign_out: String,
    /// How the pages look.
    dressing: Arc<Dressing>,
    /// Which ways other than a password the app turned on.
    has_magic_link: bool,
    has_code: bool,
    has_passkey: bool,
}

impl Paths {
    fn new(pages: &AuthPages, ui: Option<&PagesOptions>) -> Self {
        let sign_in = pages
            .sign_in
            .clone()
            .filter(|path| local(path))
            .or_else(|| ui.map(|ui| ui.sign_in.clone()).filter(|path| local(path)))
            .unwrap_or_else(|| "/sign-in".to_owned());
        let sign_in = match sign_in.trim_end_matches('/') {
            "" => "/sign-in".to_owned(),
            trimmed => trimmed.to_owned(),
        };
        let home = pages
            .home
            .clone()
            .filter(|path| ReturnTo::parse(path).is_some())
            .or_else(|| {
                ui.map(|ui| ui.home.clone())
                    .filter(|path| ReturnTo::parse(path).is_some())
            })
            .unwrap_or_else(|| "/".to_owned());
        let sign_up = pages.sign_up.then(|| {
            pages
                .sign_up_path
                .clone()
                .filter(|path| local(path))
                .map(|path| path.trim_end_matches('/').to_owned())
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| "/sign-up".to_owned())
        });
        let after_sign_up = pages
            .after_sign_up
            .clone()
            .filter(|path| ReturnTo::parse(path).is_some())
            .unwrap_or_else(|| home.clone());
        let after_sign_out = pages
            .after_sign_out
            .clone()
            .filter(|path| ReturnTo::parse(path).is_some())
            .unwrap_or_else(|| format!("{sign_in}?status=signed-out"));
        Paths {
            sign_up,
            after_sign_up,
            after_sign_out,
            dressing: Arc::new(Dressing {
                appearance: pages.appearance.clone(),
                shell: None,
                sheets: pages.sheets.clone(),
                fonts: pages.fonts.clone(),
                aside: pages.aside.clone(),
                theme_menu: pages.theme_menu,
                dress: pages.dress,
                app_name: pages.app_name.clone(),
            }),
            app_name: pages.app_name.clone(),
            two_factor: format!("{sign_in}/two-factor"),
            forgot: "/forgot-password".to_owned(),
            reset: "/reset-password".to_owned(),
            home,
            magic_link: format!("{sign_in}/link"),
            code: format!("{sign_in}/code"),
            passkey: format!("{sign_in}/passkey"),
            setup: TWO_FACTOR_SETUP_PATH.to_owned(),
            has_magic_link: false,
            has_code: false,
            has_passkey: false,
            sign_in,
        }
    }

    /// Where to go after signing in: `next` when it is a page of this site
    /// other than the sign-in pages, else home.
    fn after(&self, next: Option<&str>) -> LocalHref {
        ReturnTo::resolve_for(next, &self.sign_in, &self.home)
    }

    /// Where a new account goes: `next` when it is a page of this site other
    /// than the sign-in and sign-up pages, else [`AuthPages::after_sign_up`].
    fn after_sign_up(&self, next: Option<&str>) -> LocalHref {
        let next = next.filter(|next| {
            self.sign_up
                .as_deref()
                .is_none_or(|sign_up| ReturnTo::parse_for(next, sign_up).is_some())
        });
        ReturnTo::resolve_for(next, &self.sign_in, &self.after_sign_up)
    }

    /// `path` with `next` carried along when it is safe.
    fn with_next(&self, path: &str, next: Option<&str>) -> LocalHref {
        match next.and_then(|next| ReturnTo::parse_for(next, &self.sign_in)) {
            Some(next) => LocalHref::from(next.sign_in_href(path).as_str()),
            None => LocalHref::from(path),
        }
    }
}

/// The sign-in form.
#[derive(Debug, Deserialize, Validate, Form)]
struct SignInForm {
    /// Email
    #[validate(email, length(max = 255))]
    #[form(widget = email, autofocus, error = "Enter the email address of your account.")]
    email: String,
    /// Password
    #[validate(length(min = 1, max = 1024))]
    #[form(widget = password, error = "Enter your password.")]
    password: String,
    /// Keep me signed in on this device
    #[validate(skip)]
    #[serde(default)]
    remember: bool,
    #[validate(skip)]
    #[form(widget = hidden)]
    #[serde(default)]
    next: Option<String>,
}

/// The sign-up form.
#[derive(Debug, Deserialize, Validate, Form)]
struct SignUpForm {
    /// Your name
    #[validate(length(min = 1, max = 120))]
    #[form(autofocus, error = "Tell us what to call you.")]
    name: String,
    /// Email
    #[validate(email, length(max = 255))]
    #[form(widget = email, error = "Enter your email address.")]
    email: String,
    /// Password
    #[validate(length(min = 1, max = 1024))]
    #[form(widget = password, error = "Choose a password.")]
    password: String,
    /// Password again
    #[validate(length(min = 1, max = 1024))]
    #[form(widget = password, error = "Type the password again.")]
    password_confirmation: String,
    #[validate(skip)]
    #[form(widget = hidden)]
    #[serde(default)]
    next: Option<String>,
}

/// The second factor.
#[derive(Debug, Deserialize, Validate, Form)]
struct TwoFactorForm {
    /// Code from your authenticator app
    #[validate(length(max = 16))]
    #[form(autofocus)]
    #[serde(default)]
    code: Option<String>,
    /// Or a recovery code
    #[validate(length(max = 64))]
    #[form(help = "Each recovery code works once.")]
    #[serde(default)]
    recovery_code: Option<String>,
    #[validate(skip)]
    #[form(widget = hidden)]
    #[serde(default)]
    next: Option<String>,
}

/// The forgotten-password form.
#[derive(Debug, Deserialize, Validate, Form)]
struct ForgotForm {
    /// Email
    #[validate(email, length(max = 255))]
    #[form(widget = email, autofocus, error = "Enter the email address of your account.")]
    email: String,
}

/// The new-password form.
#[derive(Debug, Deserialize, Validate, Form)]
struct ResetForm {
    #[validate(length(min = 1, max = 255))]
    #[form(widget = hidden)]
    email: String,
    #[validate(length(min = 1, max = 512))]
    #[form(widget = hidden)]
    token: String,
    /// New password
    #[validate(length(min = 1, max = 1024))]
    #[form(widget = password, autofocus, error = "Choose a new password.")]
    password: String,
    /// New password again
    #[validate(length(min = 1, max = 1024))]
    #[form(widget = password, error = "Type the new password again.")]
    password_confirmation: String,
}

/// `?next=` and the notice the sign-in page shows.
#[derive(Debug, Default, Deserialize)]
struct SignInQuery {
    #[serde(default)]
    next: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

/// `?token=&email=` of the reset link.
#[derive(Debug, Default, Deserialize)]
struct ResetQuery {
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

/// Why the sign-in page shows a notice (`?status=`).
fn notice(status: Option<&str>) -> Option<Alert> {
    let (tone, text) = match status? {
        "signed-out" => (AlertTone::Success, "You are signed out."),
        "password-reset" => (
            AlertTone::Success,
            "Your password is changed. Sign in with the new one.",
        ),
        "expired" => (
            AlertTone::Warning,
            "Your sign-in took too long. Sign in again.",
        ),
        _ => return None,
    };
    Some(Alert::new(tone).title(text))
}

/// How the pages look, settled at boot: the options of [`AuthPages`] and the
/// app's [`Shell`].
#[derive(Clone, Debug, Default)]
struct Dressing {
    appearance: Option<Appearance>,
    shell: Option<Arc<Shell>>,
    sheets: Vec<AppSheet>,
    fonts: Vec<AppFont>,
    aside: Option<Node>,
    theme_menu: bool,
    dress: Option<fn(View) -> View>,
    app_name: Option<String>,
}

/// How one request's page looks: the reader's appearance, and the address the
/// theme menu comes back to.
#[derive(Clone, Debug)]
struct Look {
    appearance: Option<Appearance>,
    back: String,
    /// The page was drawn again after a post whose session had ended (`?status=expired`).
    expired: bool,
}

tokio::task_local! {
    /// The look of the request a page is drawn for: set around every route of
    /// the pages by [`Looked`].
    static LOOK: Look;
}

impl Dressing {
    /// The look of a request at `path` with `query` and `headers`: the app's
    /// appearance with the reader's choice over it. `None` when nothing says
    /// anything, so the page keeps the toolkit's default.
    fn look_for(&self, path: &str, query: Option<&str>, headers: &HeaderMap) -> Look {
        let appearance = match (&self.appearance, &self.shell) {
            (Some(own), _) => Some(own.clone().for_reader(headers, query)),
            (None, Some(shell)) => {
                let at = query.map_or_else(|| path.to_owned(), |query| format!("{path}?{query}"));
                Some(shell.for_request(at, headers).appearance())
            }
            (None, None) => {
                Some(Appearance::default().for_reader(headers, query)).filter(|a| !a.is_default())
            }
        };
        let kept: Vec<&str> = query
            .unwrap_or_default()
            .split('&')
            .filter(|pair| {
                let key = pair.split_once('=').map_or(*pair, |(key, _)| key);
                !pair.is_empty() && key != "theme" && key != "mode"
            })
            .collect();
        let expired = kept.contains(&"status=expired");
        let back = if kept.is_empty() {
            path.to_owned()
        } else {
            format!("{path}?{}", kept.join("&"))
        };
        Look {
            appearance,
            back,
            expired,
        }
    }

    /// The themes the theme menu offers: the shell's, else the page's own alone
    /// (so the menu holds the modes only).
    fn themes(&self, current: &Appearance) -> Vec<Theme> {
        match &self.shell {
            Some(shell) if self.appearance.is_none() => shell.offered_themes().to_vec(),
            _ => vec![current.theme().clone()],
        }
    }

    /// `title` as the browser's tab reads it: through the shell's title
    /// template, when there is one.
    fn title(&self, title: &str) -> String {
        match &self.shell {
            Some(shell) => shell.document_title(title),
            None => title.to_owned(),
        }
    }
}

/// Runs a route of the pages with its request's [`Look`] in [`LOOK`].
#[derive(Clone)]
struct Looked<S> {
    inner: S,
    dressing: Arc<Dressing>,
    paths: Arc<Paths>,
}

impl Paths {
    /// Where a page's form goes when it is posted without the session's cookie, as one
    /// left open past the session's lifetime is: the page's own `GET` with a notice, never
    /// with what was typed. Sign-out goes where it would have gone; a step with no page of
    /// its own (a code, a second factor) starts again at the sign-in page; a new password
    /// asks for a new link, since the old one's token isn't carried.
    fn after_expiry(&self, path: &str) -> String {
        if path == SIGN_OUT_PATH {
            return self.after_sign_out.clone();
        }
        let page = if path == self.reset {
            &self.forgot
        } else if [
            Some(&self.sign_in),
            self.sign_up.as_ref(),
            Some(&self.forgot),
            Some(&self.magic_link),
            Some(&self.code),
        ]
        .into_iter()
        .flatten()
        .any(|page| page == path)
        {
            path
        } else {
            &self.sign_in
        };
        format!("{page}?status=expired")
    }
}

/// Whether `headers` say the body is a form a page posted.
fn is_form_post(headers: &HeaderMap) -> bool {
    headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|kind| {
            let kind = kind.trim_start().to_ascii_lowercase();
            kind.starts_with("application/x-www-form-urlencoded")
                || kind.starts_with("multipart/form-data")
        })
}

/// Logs a request refused for coming without the session's cookie, as the session layer
/// logs its CSRF refusals: on `navree::security`, and in the audit trail as
/// `csrf.mismatch` with the reason `no_session`.
fn log_refusal(
    request: &navree_http::extract::Request,
) -> impl Future<Output = ()> + Send + 'static {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let ip = request
        .extensions()
        .get::<navree_http::ClientIp>()
        .map(|ip| ip.0);
    tracing::warn!(
        target: "navree::security",
        method,
        path,
        reason = "no_session",
        ip = ip.map(tracing::field::display),
        "CSRF token mismatch"
    );
    let ctx = request
        .extensions()
        .get::<navree_kernel::AppContext>()
        .cloned();
    let mut entry =
        navree_contracts::AuditEntry::new(navree_contracts::AuditKind::Security, "csrf.mismatch")
            .metadata(serde_json::json!({
                "method": method,
                "path": path,
                "reason": "no_session",
            }));
    if let Some(ip) = ip {
        entry = entry.ip(ip.to_string());
    }
    if let Some(request_id) = request.extensions().get::<navree_contracts::RequestId>() {
        entry = entry.request_id(request_id.as_str());
    }
    async move {
        let Some(ctx) = ctx else {
            return;
        };
        let Some(sink) = ctx.try_get::<dyn navree_contracts::AuditSink>().await else {
            return;
        };
        if let Err(error) = sink.record(entry).await {
            tracing::warn!(target: "navree::security", %error, "could not audit a CSRF failure");
        }
    }
}

impl<S> tower::Service<navree_http::extract::Request> for Looked<S>
where
    S: tower::Service<navree_http::extract::Request, Response = Response, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Response, Infallible>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: navree_http::extract::Request) -> Self::Future {
        // Login CSRF: every form of these pages is drawn by a GET that starts the session,
        // so a post without the session's cookie came from another page, perhaps a sibling
        // subdomain's or one whose browser says nothing of where it posts from, and could
        // sign the reader in to someone else's account. It is refused before anything runs.
        let safe = matches!(
            *request.method(),
            Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
        );
        let resumed = request
            .extensions()
            .get::<Session>()
            .is_some_and(Session::is_resumed);
        if !safe && !resumed {
            let logged = log_refusal(&request);
            // A form left open past the session's lifetime comes back the same way: its
            // page is drawn again with a notice, and nothing in the post is used.
            let answer = if is_form_post(request.headers()) {
                Reply::redirect(LocalHref::from(
                    self.paths.after_expiry(request.uri().path()),
                ))
                .into_response()
            } else {
                AppError::csrf_mismatch().into_response()
            };
            return Box::pin(async move {
                logged.await;
                Ok(answer)
            });
        }
        let look = self.dressing.look_for(
            request.uri().path(),
            request.uri().query(),
            request.headers(),
        );
        let answer = self.inner.call(request);
        Box::pin(LOOK.scope(look, answer))
    }
}

/// A page holding one card, in the app's look.
fn page(paths: &Paths, title: &'static str, description: &'static str, body: Node) -> View {
    let dressing = &paths.dressing;
    let look = LOOK.try_with(Clone::clone).unwrap_or_else(|_| Look {
        appearance: dressing.appearance.clone(),
        back: paths.sign_in.clone(),
        expired: false,
    });
    // The sign-in page says it in its own notice.
    let expired = (look.expired && title != "Sign in")
        .then(|| Alert::new(AlertTone::Warning).title("Your session expired. Please try again."));
    let heading = paths.app_name.clone().map(Text::muted);
    let card = Stack::gap(4).children((
        heading,
        el("h1").slot("page-title").child(title),
        expired,
        Card::new().description(description).body(body),
    ));
    let mut layout = AuthLayout::new(card);
    if let Some(aside) = &dressing.aside {
        layout = layout.aside(aside.clone());
        if let Some(name) = &dressing.app_name {
            layout = layout.aside_label(format!("About {name}"));
        }
    }
    let mut view = View::new(dressing.title(title));
    let menu = dressing.theme_menu
        || dressing
            .shell
            .as_ref()
            .is_some_and(|shell| shell.has_theme_menu());
    if menu {
        let current = look.appearance.clone().unwrap_or_default();
        let menu = ThemeMenu::new(&current, &dressing.themes(&current)).back(look.back.clone());
        for theme in menu.sheets() {
            view = view.theme_sheet(&theme);
        }
        layout = layout.corner(menu);
    }
    view = view.body(layout);
    if let Some(appearance) = look.appearance {
        view = view.appearance(appearance);
    }
    let shell = dressing.shell.as_deref();
    for font in shell
        .map(Shell::fonts)
        .unwrap_or_default()
        .iter()
        .chain(&dressing.fonts)
    {
        view = view.font(font);
    }
    for sheet in shell
        .map(Shell::stylesheets)
        .unwrap_or_default()
        .iter()
        .chain(&dressing.sheets)
    {
        view = view.stylesheet(sheet);
    }
    match dressing.dress {
        Some(dress) => dress(view),
        None => view,
    }
}

/// A link under a form.
fn link(text: &'static str, href: &str) -> Node {
    el("a").href(href).child(text).into()
}

/// A link to a page that loads a script of its own: the browser reloads the
/// whole page, so the script runs.
fn plain_link(text: &'static str, href: &str) -> Node {
    el("a")
        .href(href)
        .flag("data-ui-noboost", true)
        .child(text)
        .into()
}

/// The sign-in page around `form`; `next` travels to the other ways in.
fn sign_in_page(
    paths: &Paths,
    form: FormView<SignInForm>,
    notice: Option<Alert>,
    next: Option<&str>,
) -> View {
    let mut others = vec![link("Forgot your password?", &paths.forgot)];
    if let Some(sign_up) = &paths.sign_up {
        others.push(link(
            "New here? Create an account",
            paths.with_next(sign_up, next).as_str(),
        ));
    }
    if paths.has_passkey {
        others.push(plain_link(
            "Sign in with a passkey",
            paths.with_next(&paths.passkey, next).as_str(),
        ));
    }
    if paths.has_magic_link {
        others.push(link(
            "Email me a sign-in link",
            paths.with_next(&paths.magic_link, next).as_str(),
        ));
    }
    if paths.has_code {
        others.push(link(
            "Email me a sign-in code",
            paths.with_next(&paths.code, next).as_str(),
        ));
    }
    page(
        paths,
        "Sign in",
        "Use the email address and password of your account.",
        Stack::gap(4)
            .children((
                notice,
                Node::from(form.one_column().submit("Sign in")),
                Node::from(Stack::gap(2).children(others)),
            ))
            .into(),
    )
}

/// Where a browser that just signed in goes: to `next` (else home), or, when
/// a policy makes it set up two-factor authentication first, to the setup
/// page, which carries `next` on.
fn signed_in_target(
    service: &AuthService,
    session: &Session,
    paths: &Paths,
    next: Option<&str>,
) -> LocalHref {
    if service.two_factor_setup_required(session) {
        paths.with_next(&paths.setup, next)
    } else {
        paths.after(next)
    }
}

/// The redirect to [`signed_in_target`].
fn signed_in_redirect(
    service: &AuthService,
    session: &Session,
    paths: &Paths,
    next: Option<&str>,
) -> Response {
    Reply::redirect(signed_in_target(service, session, paths, next)).into_response()
}

/// The sign-up page around `form`.
fn sign_up_page(paths: &Paths, form: FormView<SignUpForm>, next: Option<&str>) -> View {
    page(
        paths,
        "Create an account",
        "Your name, your email address and a password.",
        Stack::gap(4)
            .children((
                Node::from(form.one_column().submit("Create the account")),
                link(
                    "Already have an account? Sign in",
                    paths.with_next(&paths.sign_in, next).as_str(),
                ),
            ))
            .into(),
    )
}

fn two_factor_page(paths: &Paths, form: FormView<TwoFactorForm>) -> View {
    page(
        paths,
        "Two-factor authentication",
        "Enter the code your authenticator app shows now, or one of your recovery codes.",
        Stack::gap(4)
            .children((
                Node::from(form.one_column().submit("Continue")),
                link("Sign in as someone else", &paths.sign_in),
            ))
            .into(),
    )
}

fn forgot_page(paths: &Paths, form: FormView<ForgotForm>, sent: bool) -> View {
    let sent = sent.then(|| {
        Alert::new(AlertTone::Success).title(
            "If an account uses that address, we have sent it a link to choose a new password.",
        )
    });
    page(
        paths,
        "Forgot your password?",
        "We will email you a link to choose a new one.",
        Stack::gap(4)
            .children((
                sent,
                Node::from(form.one_column().submit("Send the link")),
                link("Back to sign in", &paths.sign_in),
            ))
            .into(),
    )
}

fn reset_page(paths: &Paths, form: FormView<ResetForm>, refused: bool) -> View {
    let refused = refused.then(|| {
        Alert::new(AlertTone::Danger)
            .title("This link has expired or was used already.")
            .body(Text::new(
                "Ask for a new one from the forgotten password page.",
            ))
    });
    page(
        paths,
        "Choose a new password",
        "Changing it signs you out everywhere.",
        Stack::gap(4)
            .children((
                refused,
                Node::from(form.one_column().submit("Change the password")),
                link("Ask for a new link", &paths.forgot),
            ))
            .into(),
    )
}

/// One field error.
fn field_error(field: &str, message: impl Into<String>) -> FieldErrors {
    BTreeMap::from([(field.to_owned(), vec![message.into()])])
}

/// `response` as a 429 with `Retry-After`.
fn too_many(mut response: Response, retry_after: Duration) -> Response {
    *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
    if let Ok(value) = HeaderValue::from_str(&retry_after.as_secs().max(1).to_string()) {
        response
            .headers_mut()
            .insert(http::header::RETRY_AFTER, value);
    }
    response
}

/// The text of a throttled attempt.
fn wait_text(retry_after: Duration) -> String {
    format!(
        "Too many attempts. Try again in {} seconds.",
        retry_after.as_secs().max(1)
    )
}

/// The documentation of a route of the pages. They answer HTML (or a
/// redirect), so a page has no response schema.
fn api(summary: &str, responses: &[u16]) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Auth pages".to_owned()],
        responses: responses.to_vec(),
        ..ApiMeta::default()
    }
}

/// Whether the request is already signed in.
fn signed_in(parts: &Parts) -> bool {
    parts
        .extensions
        .get::<AuthState>()
        .is_some_and(|state| state.0.is_some())
}

/// The routes of [`AuthPages`].
struct PagesController {
    service: Arc<AuthService>,
    paths: Arc<Paths>,
}

impl Injectable for PagesController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<AuthService>(),
            ProviderKey::of::<AuthPages>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let service = r.get::<AuthService>().await?;
            let pages = r.get::<AuthPages>().await?;
            let ui = r.app().try_get::<PagesOptions>().await;
            let mut paths = Paths::new(&pages, ui.as_deref());
            let shell = r.app().try_get::<Shell>().await;
            if shell.is_some() {
                let mut dressing = (*paths.dressing).clone();
                dressing.shell = shell;
                paths.dressing = Arc::new(dressing);
            }
            let options = service.options();
            if !options.registration {
                paths.sign_up = None;
            }
            paths.has_magic_link = options.magic_link.enabled;
            paths.has_code = options.otp.enabled;
            paths.has_passkey = options.passkeys.enabled;
            Ok(PagesController {
                service,
                paths: Arc::new(paths),
            })
        })
    }
}

impl Controller for PagesController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let paths = self.paths.clone();
        let absolute = |path: &str| format!("!{path}");
        let mut routes = Vec::new();

        // GET sign-in
        let at = paths.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                absolute(&paths.sign_in),
                move |parts: Parts, session: Session, Query(query): Query<SignInQuery>| {
                    let paths = at.clone();
                    async move {
                        if signed_in(&parts) {
                            return Reply::redirect(paths.after(query.next.as_deref()))
                                .into_response();
                        }
                        let next = query
                            .next
                            .as_deref()
                            .and_then(|next| ReturnTo::parse_for(next, &paths.sign_in))
                            .map(|next| next.as_str().to_owned());
                        let form = FormView::<SignInForm>::new()
                            .post(paths.sign_in.as_str())
                            .csrf(session.csrf_token())
                            .of(&SignInForm {
                                email: String::new(),
                                password: String::new(),
                                remember: false,
                                next: next.clone(),
                            });
                        sign_in_page(
                            &paths,
                            form,
                            notice(query.status.as_deref()),
                            next.as_deref(),
                        )
                        .into_response()
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.sign_in")
            .api(ApiMeta {
                description: Some(
                    "The form, with \"remember me\"; a browser already signed in goes on to \
                     `next` (303)."
                        .to_owned(),
                ),
                ..api("The sign-in page", &[303])
            }),
        );

        // POST sign-in
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&paths.sign_in),
                move |parts: Parts,
                      session: Session,
                      meta: ClientMeta,
                      submitted: Result<Submit<SignInForm>, Rejected<SignInForm>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let token = session.csrf_token();
                        let redraw = |rejected: Rejected<SignInForm>| {
                            let paths = paths.clone();
                            let token = token.clone();
                            rejected.respond(move |form| {
                                sign_in_page(&paths, form.csrf(token), None, None)
                            })
                        };
                        let input = match submitted {
                            Ok(Submit(input)) => input,
                            Err(rejected) => return Ok(redraw(rejected)),
                        };
                        let outcome = service
                            .sign_in_session(
                                &session,
                                &input.email,
                                &input.password,
                                input.remember,
                                &meta,
                            )
                            .await;
                        match outcome {
                            Ok(SessionSignIn::SignedIn(_)) => Ok(signed_in_redirect(
                                &service,
                                &session,
                                &paths,
                                input.next.as_deref(),
                            )),
                            Ok(SessionSignIn::TwoFactorRequired) => Ok(Reply::redirect(
                                paths.with_next(&paths.two_factor, input.next.as_deref()),
                            )
                            .into_response()),
                            Err(AuthError::InvalidCredentials) => Ok(redraw(
                                Rejected::after_decode(
                                    &input,
                                    field_error(
                                        "email",
                                        "These details don't match an account. Check them and try again.",
                                    ),
                                    &parts,
                                ),
                            )),
                            Err(AuthError::TooManyAttempts { retry_after }) => Ok(too_many(
                                redraw(Rejected::after_decode(
                                    &input,
                                    field_error("email", wait_text(retry_after)),
                                    &parts,
                                )),
                                retry_after,
                            )),
                            Err(AuthError::SignInRefused { message, .. }) => {
                                let mut response = redraw(Rejected::after_decode(
                                    &input,
                                    field_error("email", message),
                                    &parts,
                                ));
                                *response.status_mut() = StatusCode::FORBIDDEN;
                                Ok(response)
                            }
                            Err(error) => Err::<Response, AppError>(error.into_app_error()),
                        }
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.sign_in.submit")
            .api(ApiMeta {
                description: Some(
                    "Goes on to `next`, or to the second factor when the account uses it (303). \
                     Details that don't match an account draw the form again with the error \
                     (422); a sign-in policy that refuses the account answers 403."
                        .to_owned(),
                ),
                ..api("Signs in with an email and a password", &[303, 403, 422, 429])
            }),
        );

        // GET two-factor
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::GET,
                absolute(&paths.two_factor),
                move |session: Session, Query(query): Query<SignInQuery>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        if !service.two_factor_pending(&session) {
                            return Reply::redirect(
                                paths.with_next(&paths.sign_in, query.next.as_deref()),
                            )
                            .into_response();
                        }
                        let form = FormView::<TwoFactorForm>::new()
                            .post(paths.two_factor.as_str())
                            .csrf(session.csrf_token())
                            .of(&TwoFactorForm {
                                code: None,
                                recovery_code: None,
                                next: query
                                    .next
                                    .as_deref()
                                    .and_then(|next| ReturnTo::parse_for(next, &paths.sign_in))
                                    .map(|next| next.as_str().to_owned()),
                            });
                        two_factor_page(&paths, form).into_response()
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.two_factor")
            .api(ApiMeta {
                description: Some(
                    "Asks for the code from the authenticator app, or a recovery code; with no \
                     sign-in waiting for one, goes back to the sign-in page (303)."
                        .to_owned(),
                ),
                ..api("The second-factor page", &[303])
            }),
        );

        // POST two-factor
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&paths.two_factor),
                move |parts: Parts,
                      session: Session,
                      meta: ClientMeta,
                      submitted: Result<Submit<TwoFactorForm>, Rejected<TwoFactorForm>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let token = session.csrf_token();
                        let redraw = |rejected: Rejected<TwoFactorForm>| {
                            let paths = paths.clone();
                            let token = token.clone();
                            rejected.respond(move |form| two_factor_page(&paths, form.csrf(token)))
                        };
                        let input = match submitted {
                            Ok(Submit(input)) => input,
                            Err(rejected) => return Ok(redraw(rejected)),
                        };
                        let filled = |value: &Option<String>| {
                            value
                                .as_deref()
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(str::to_owned)
                        };
                        let proof = match (filled(&input.code), filled(&input.recovery_code)) {
                            (Some(code), _) => TwoFactorProof::Code(code),
                            (None, Some(code)) => TwoFactorProof::RecoveryCode(code),
                            (None, None) => {
                                return Ok(redraw(Rejected::after_decode(
                                    &input,
                                    field_error(
                                        "code",
                                        "Enter the code from your authenticator app.",
                                    ),
                                    &parts,
                                )));
                            }
                        };
                        match service.verify_two_factor_session(&session, proof, &meta).await {
                            Ok(_) => Ok(signed_in_redirect(
                                &service,
                                &session,
                                &paths,
                                input.next.as_deref(),
                            )),
                            Err(AuthError::InvalidTwoFactorCode) => Ok(redraw(
                                Rejected::after_decode(
                                    &input,
                                    field_error(
                                        "code",
                                        "That code didn't work. Wait for the next one and try again.",
                                    ),
                                    &parts,
                                ),
                            )),
                            Err(AuthError::TooManyAttempts { retry_after }) => Ok(too_many(
                                redraw(Rejected::after_decode(
                                    &input,
                                    field_error("code", wait_text(retry_after)),
                                    &parts,
                                )),
                                retry_after,
                            )),
                            Err(AuthError::InvalidToken | AuthError::TokenExpired) => {
                                let target = format!("{}?status=expired", paths.sign_in);
                                Ok(Reply::redirect(LocalHref::from(target)).into_response())
                            }
                            Err(AuthError::SignInRefused { message, .. }) => {
                                let mut response = redraw(Rejected::after_decode(
                                    &input,
                                    field_error("code", message),
                                    &parts,
                                ));
                                *response.status_mut() = StatusCode::FORBIDDEN;
                                Ok(response)
                            }
                            Err(error) => Err::<Response, AppError>(error.into_app_error()),
                        }
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.two_factor.submit")
            .api(ApiMeta {
                description: Some(
                    "A code from the authenticator app, or a recovery code. Goes on to `next` \
                     (303); a code that didn't work draws the form again (422)."
                        .to_owned(),
                ),
                ..api("Finishes signing in with a two-factor code", &[303, 403, 422, 429])
            }),
        );

        // GET forgot-password
        let at = paths.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                absolute(&paths.forgot),
                move |session: Session| {
                    let paths = at.clone();
                    async move {
                        let form = FormView::<ForgotForm>::new()
                            .post(paths.forgot.as_str())
                            .csrf(session.csrf_token());
                        forgot_page(&paths, form, false).into_response()
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.forgot_password")
            .api(api("The forgotten-password page", &[])),
        );

        // POST forgot-password
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&paths.forgot),
                move |session: Session,
                      meta: ClientMeta,
                      submitted: Result<Submit<ForgotForm>, Rejected<ForgotForm>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let token = session.csrf_token();
                        let input = match submitted {
                            Ok(Submit(input)) => input,
                            Err(rejected) => {
                                return Ok(rejected.respond(move |form| {
                                    forgot_page(&paths, form.csrf(token), false)
                                }));
                            }
                        };
                        service
                            .send_password_reset_link(&input.email, &meta)
                            .await
                            .map_err(AuthError::into_app_error)?;
                        // The same answer for every address.
                        let form = FormView::<ForgotForm>::new()
                            .post(paths.forgot.as_str())
                            .csrf(token);
                        Ok::<_, AppError>(forgot_page(&paths, form, true).into_response())
                    }
                },
            )
            .public()
            .page()
            .throttle(FORGOT_PER_MINUTE, Duration::from_secs(60))
            .name("auth.pages.forgot_password.submit")
            .api(ApiMeta {
                description: Some(
                    "The same answer for every address, so the page never says whether an account \
                     uses it."
                        .to_owned(),
                ),
                ..api("Mails a link to choose a new password", &[403, 422, 429])
            }),
        );

        // GET reset-password
        let at = paths.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                absolute(&paths.reset),
                move |session: Session, Query(query): Query<ResetQuery>| {
                    let paths = at.clone();
                    async move {
                        let form = FormView::<ResetForm>::new()
                            .post(paths.reset.as_str())
                            .csrf(session.csrf_token())
                            .of(&ResetForm {
                                email: query.email.unwrap_or_default(),
                                token: query.token.unwrap_or_default(),
                                password: String::new(),
                                password_confirmation: String::new(),
                            });
                        reset_page(&paths, form, false).into_response()
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.reset_password")
            .api(ApiMeta {
                description: Some(
                    "The form for a new password, filled from the mailed link's `token` and \
                     `email`."
                        .to_owned(),
                ),
                ..api("The reset-password page", &[])
            }),
        );

        // POST reset-password
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&paths.reset),
                move |parts: Parts,
                      session: Session,
                      meta: ClientMeta,
                      submitted: Result<Submit<ResetForm>, Rejected<ResetForm>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let token = session.csrf_token();
                        let redraw = |rejected: Rejected<ResetForm>, refused: bool| {
                            let paths = paths.clone();
                            let token = token.clone();
                            rejected
                                .respond(move |form| reset_page(&paths, form.csrf(token), refused))
                        };
                        let input = match submitted {
                            Ok(Submit(input)) => input,
                            Err(rejected) => return Ok(redraw(rejected, false)),
                        };
                        if input.password != input.password_confirmation {
                            return Ok(redraw(
                                Rejected::after_decode(
                                    &input,
                                    field_error(
                                        "password_confirmation",
                                        "The two passwords are not the same.",
                                    ),
                                    &parts,
                                ),
                                false,
                            ));
                        }
                        match service
                            .reset_password(&input.email, &input.token, &input.password, &meta)
                            .await
                        {
                            Ok(()) => {
                                // Every session of the user is signed out; so is
                                // this browser, whoever it was signed in as.
                                service
                                    .sign_out_session(&session, &meta)
                                    .await
                                    .map_err(AuthError::into_app_error)?;
                                let target = format!("{}?status=password-reset", paths.sign_in);
                                Ok(Reply::redirect(LocalHref::from(target)).into_response())
                            }
                            Err(AuthError::InvalidResetToken) => {
                                let mut response = redraw(
                                    Rejected::after_decode(&input, FieldErrors::new(), &parts),
                                    true,
                                );
                                *response.status_mut() = StatusCode::BAD_REQUEST;
                                Ok(response)
                            }
                            Err(AuthError::Invalid { field, message }) => Ok(redraw(
                                Rejected::after_decode(&input, field_error(field, message), &parts),
                                false,
                            )),
                            Err(error) => Err::<Response, AppError>(error.into_app_error()),
                        }
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.reset_password.submit")
            .api(ApiMeta {
                description: Some(
                    "Signs every session of the user out, then goes to the sign-in page (303). A \
                     link that is no longer good answers 400."
                        .to_owned(),
                ),
                ..api("Sets a new password", &[303, 400, 403, 422])
            }),
        );

        // POST sign-out
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(SIGN_OUT_PATH),
                move |session: Session, meta: ClientMeta, RawQuery(_): RawQuery| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        service
                            .sign_out_session(&session, &meta)
                            .await
                            .map_err(AuthError::into_app_error)?;
                        // A native client forgets what it holds of the person.
                        Ok::<_, AppError>(
                            Reply::redirect(LocalHref::from(paths.after_sign_out.as_str()))
                                .signed_out()
                                .into_response(),
                        )
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.sign_out")
            .api(ApiMeta {
                description: Some(
                    "Ends the browser's sign-in, then goes to the sign-in page (or the page \
                     the app chose)."
                        .to_owned(),
                ),
                ..api("Signs out", &[303, 403])
            }),
        );
        if paths.has_magic_link {
            routes.extend(self.link_routes());
        }
        if paths.has_code {
            routes.extend(self.code_routes());
        }
        if paths.has_passkey {
            routes.extend(self.passkey_routes());
        }
        if paths.sign_up.is_some() {
            routes.extend(self.sign_up_routes());
        }
        routes.extend(self.account_routes());
        // Every page is drawn in the look of the request it answers.
        let dressing = paths.dressing.clone();
        for route in &mut routes {
            let (dressing, paths) = (dressing.clone(), paths.clone());
            route.handler = route
                .handler
                .clone()
                .layer(tower::layer::layer_fn(move |inner| Looked {
                    inner,
                    dressing: dressing.clone(),
                    paths: paths.clone(),
                }));
        }
        routes
    }
}

impl PagesController {
    /// `GET` and `POST` of the sign-up page.
    fn sign_up_routes(&self) -> Vec<RouteDef> {
        let paths = self.paths.clone();
        let Some(path) = paths.sign_up.clone() else {
            return Vec::new();
        };
        let absolute = format!("!{path}");
        let mut routes = Vec::new();

        let at = paths.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                absolute.clone(),
                move |parts: Parts, session: Session, Query(query): Query<SignInQuery>| {
                    let paths = at.clone();
                    async move {
                        if signed_in(&parts) {
                            return Reply::redirect(paths.after_sign_up(query.next.as_deref()))
                                .into_response();
                        }
                        let next = query
                            .next
                            .as_deref()
                            .and_then(|next| ReturnTo::parse_for(next, &paths.sign_in))
                            .map(|next| next.as_str().to_owned());
                        let sign_up = paths.sign_up.clone().unwrap_or_default();
                        let form = FormView::<SignUpForm>::new()
                            .post(sign_up.as_str())
                            .csrf(session.csrf_token())
                            .of(&SignUpForm {
                                name: String::new(),
                                email: String::new(),
                                password: String::new(),
                                password_confirmation: String::new(),
                                next: next.clone(),
                            });
                        sign_up_page(&paths, form, next.as_deref()).into_response()
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.sign_up")
            .api(ApiMeta {
                description: Some(
                    "The form for a new account; a browser already signed in goes on (303)."
                        .to_owned(),
                ),
                ..api("The sign-up page", &[303])
            }),
        );

        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute,
                move |parts: Parts,
                      session: Session,
                      meta: ClientMeta,
                      uow: Uow,
                      submitted: Result<Submit<SignUpForm>, Rejected<SignUpForm>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let token = session.csrf_token();
                        let redraw = |rejected: Rejected<SignUpForm>, next: Option<String>| {
                            let paths = paths.clone();
                            let token = token.clone();
                            rejected.respond(move |form| {
                                sign_up_page(&paths, form.csrf(token), next.as_deref())
                            })
                        };
                        let input = match submitted {
                            Ok(Submit(input)) => input,
                            Err(rejected) => return Ok(redraw(rejected, None)),
                        };
                        let refuse = |field: &str, message: String| {
                            redraw(
                                Rejected::after_decode(&input, field_error(field, message), &parts),
                                input.next.clone(),
                            )
                        };
                        if input.password != input.password_confirmation {
                            return Ok(refuse(
                                "password_confirmation",
                                "The two passwords are not the same.".to_owned(),
                            ));
                        }
                        let created = service
                            .register(
                                &uow,
                                NewUser {
                                    name: input.name.trim().to_owned(),
                                    email: input.email.trim().to_owned(),
                                    password: input.password.clone(),
                                },
                            )
                            .await;
                        match created {
                            Ok(_) => {}
                            Err(AuthError::EmailTaken) => {
                                return Ok(refuse(
                                    "email",
                                    "An account already uses this address. Sign in instead."
                                        .to_owned(),
                                ));
                            }
                            Err(AuthError::Invalid { field, message }) => {
                                let field = if field == "name" || field == "email" {
                                    field
                                } else {
                                    "password"
                                };
                                return Ok(refuse(field, message));
                            }
                            Err(AuthError::SignInRefused { message, .. }) => {
                                let mut response = refuse("email", message);
                                *response.status_mut() = StatusCode::FORBIDDEN;
                                return Ok(response);
                            }
                            Err(error) => return Err::<Response, AppError>(error.into_app_error()),
                        }
                        uow.commit().await?;
                        // The new account signs in as the sign-in page does: a policy
                        // may still ask for a second factor first.
                        match service
                            .sign_in_session(
                                &session,
                                input.email.trim(),
                                &input.password,
                                false,
                                &meta,
                            )
                            .await
                        {
                            Ok(SessionSignIn::SignedIn(_)) => {
                                let target = if service.two_factor_setup_required(&session) {
                                    paths.with_next(&paths.setup, input.next.as_deref())
                                } else {
                                    paths.after_sign_up(input.next.as_deref())
                                };
                                Ok(Reply::redirect(target).into_response())
                            }
                            Ok(SessionSignIn::TwoFactorRequired) => Ok(Reply::redirect(
                                paths.with_next(&paths.two_factor, input.next.as_deref()),
                            )
                            .into_response()),
                            Err(_) => Ok(Reply::redirect(LocalHref::from(paths.sign_in.as_str()))
                                .into_response()),
                        }
                    }
                },
            )
            .public()
            .page()
            .throttle(SIGN_UP_PER_MINUTE, Duration::from_secs(60))
            .name("auth.pages.sign_up.submit")
            .api(ApiMeta {
                description: Some(
                    "Makes the account ([`AuthService::register`]: a unique address, the \
                     password policy, the sign-in policies), signs it in on this browser and \
                     goes on to `next` (303). A form the checks refuse is drawn again with \
                     the errors (422); an address a sign-in policy keeps from passwords \
                     answers 403."
                        .to_owned(),
                ),
                ..api("Creates an account and signs it in", &[303, 403, 422, 429])
            }),
        );
        routes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(handed_over: bool) -> HandOverShellPart {
        let mut apps = crate::AppsOptions::default();
        let mut apple = crate::apps::AppleApp::default();
        apple.bundle_id = "com.example.orders".to_owned();
        apple.callback = Some("https://orders.example.in/app/apple".to_owned());
        apps.apple.insert("main".to_owned(), apple);
        HandOverShellPart {
            handed_over,
            apps: Arc::new(apps),
            exchange: "/api/auth/app/exchange".to_owned(),
            second_step: "/sign-in/two-factor".to_owned(),
        }
    }

    #[test]
    fn the_shell_document_names_the_hand_over_and_the_callback_of_the_family_that_asks() {
        use navree_ui::{NativeClient, NativeFamily, NativeShellPart, NativeShellParts};
        let said = |family| {
            let mut parts = NativeShellParts::default();
            part(true).contribute(&mut parts, &NativeClient::new(family));
            parts
        };
        let mut apple = NativeShellParts::default();
        apple.sign_in(
            navree_ui::NativeSignIn::new("/auth/app/sign-in", "/api/auth/app/exchange")
                .callback("https://orders.example.in/app/apple")
                .second_step("/sign-in/two-factor"),
        );
        assert_eq!(said(NativeFamily::Apple), apple);
        // An Android client has no app of its own here, so no callback: it knows its own.
        let mut android = NativeShellParts::default();
        android.sign_in(
            navree_ui::NativeSignIn::new("/auth/app/sign-in", "/api/auth/app/exchange")
                .second_step("/sign-in/two-factor"),
        );
        assert_eq!(said(NativeFamily::Android), android);
        // Where the hand-over routes are not mounted, nothing is said.
        let mut none = NativeShellParts::default();
        part(false).contribute(&mut none, &NativeClient::new(NativeFamily::Apple));
        assert_eq!(none, NativeShellParts::default());
    }

    #[test]
    fn the_paths_follow_the_ui_settings_unless_told() {
        let paths = Paths::new(&AuthPages::new(), None);
        assert_eq!(paths.sign_in, "/sign-in");
        assert_eq!(paths.two_factor, "/sign-in/two-factor");
        assert_eq!(paths.home, "/");
        let ui = PagesOptions::default().sign_in("/login").home("/dashboard");
        let paths = Paths::new(&AuthPages::new(), Some(&ui));
        assert_eq!(paths.sign_in, "/login");
        assert_eq!(paths.two_factor, "/login/two-factor");
        assert_eq!(paths.home, "/dashboard");
        let paths = Paths::new(
            &AuthPages::new().sign_in_path("/enter/").home("/app"),
            Some(&ui),
        );
        assert_eq!(paths.sign_in, "/enter");
        assert_eq!(paths.home, "/app");
        // A provider's address is not a page of this app: the default stays.
        let sso = PagesOptions::default().sign_in("https://sso.example/login");
        assert_eq!(
            Paths::new(&AuthPages::new(), Some(&sso)).sign_in,
            "/sign-in"
        );
    }

    // A `next` never leads off the site or back to the sign-in pages.
    #[test]
    fn next_stays_on_the_site_and_off_the_sign_in_pages() {
        let paths = Paths::new(&AuthPages::new(), None);
        assert_eq!(
            paths.after(Some("/deals/42?tab=notes")).as_str(),
            "/deals/42?tab=notes"
        );
        for bad in [
            "//evil.example",
            "/\\evil.example",
            "https://evil.example/x",
            "/sign-in",
            "/sign-in/two-factor",
            "javascript:alert(1)",
        ] {
            assert_eq!(paths.after(Some(bad)).as_str(), "/", "{bad}");
        }
        assert_eq!(
            paths
                .with_next("/sign-in/two-factor", Some("/deals"))
                .as_str(),
            "/sign-in/two-factor?next=%2Fdeals"
        );
        assert_eq!(
            paths
                .with_next("/sign-in/two-factor", Some("//evil.example"))
                .as_str(),
            "/sign-in/two-factor"
        );
    }

    #[test]
    fn notices_are_known_words_only() {
        assert!(notice(Some("signed-out")).is_some());
        assert!(notice(Some("password-reset")).is_some());
        assert!(notice(Some("expired")).is_some());
        assert!(notice(Some("<script>")).is_none());
        assert!(notice(None).is_none());
    }
}
