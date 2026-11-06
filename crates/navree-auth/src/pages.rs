//! [`AuthPages`] (feature `ui`): the pages a browser signs in and out with,
//! drawn with navree-ui on the session ([`AuthService::sign_in_session`]).

mod account;
mod passkey;
mod passwordless;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use http::request::Parts;
use http::{HeaderValue, Method, StatusCode};
use navree_contracts::AuthState;
use navree_http::ModuleBuilderHttpExt;
use navree_http::extract::{Query, RawQuery};
use navree_http::{AppError, Controller, IntoResponse, Response, RouteDef};
use navree_kernel::{
    BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_session::Session;
use navree_ui::form::{FieldErrors, FormView, Rejected};
use navree_ui::pages::{PagesOptions, ReturnTo};
use navree_ui::{
    Alert, AlertTone, Card, Container, ContainerSize, Form, LocalHref, Node, Reply, Stack, Submit,
    Text, Validate, View, el,
};
use serde::Deserialize;

use crate::session_auth::{SIGN_OUT_PATH, TWO_FACTOR_SETUP_PATH};
use crate::{AuthError, AuthService, ClientMeta, SessionSignIn, TwoFactorProof};

/// How many forgotten-password requests one client may send per minute.
const FORGOT_PER_MINUTE: u32 = 5;

/// The pages a browser signs in and out with, on the session cookie, drawn
/// with navree-ui: sign-in (with "remember me"), the second factor,
/// forgotten and reset password, and sign-out. A module: add it next to
/// `AuthModule`, `SessionModule` and `UiModule`.
///
/// | Route | What it does |
/// |---|---|
/// | `GET /sign-in?next=` | the sign-in form (a signed-in browser goes on to `next`) |
/// | `POST /sign-in` | [`AuthService::sign_in_session`]: on to `next`, or to the second factor |
/// | `GET`, `POST /sign-in/two-factor` | the code from the authenticator app, or a recovery code ([`AuthService::verify_two_factor_session`]) |
/// | `GET`, `POST /forgot-password` | mails a reset link ([`AuthService::send_password_reset_link`]); always the same answer |
/// | `GET /reset-password?token=&email=`, `POST /reset-password` | sets the new password ([`AuthService::reset_password`]), which signs every session out |
/// | `POST /sign-out` | [`AuthService::sign_out_session`], then the sign-in page |
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
///     .module(AuthPages::new().app_name("Agra CRM"))
///     .run()
/// # }
/// ```
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct AuthPages {
    app_name: Option<String>,
    sign_in: Option<String>,
    home: Option<String>,
}

impl AuthPages {
    /// The pages at their default paths.
    pub fn new() -> Self {
        Self::default()
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
        Paths {
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

/// A page holding one card.
fn page(paths: &Paths, title: &'static str, description: &'static str, body: Node) -> View {
    let heading = paths.app_name.clone().map(Text::muted);
    View::new(title).body(
        el("main").attr("data-auth-page", "").child(
            Container::new()
                .size(ContainerSize::Sm)
                .children(Stack::gap(4).children((
                    heading,
                    el("h1").slot("page-title").child(title),
                    Card::new().description(description).body(body),
                ))),
        ),
    )
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
                Node::from(form.submit("Sign in")),
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

fn two_factor_page(paths: &Paths, form: FormView<TwoFactorForm>) -> View {
    page(
        paths,
        "Two-factor authentication",
        "Enter the code your authenticator app shows now, or one of your recovery codes.",
        Stack::gap(4)
            .children((
                Node::from(form.submit("Continue")),
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
                Node::from(form.submit("Send the link")),
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
                Node::from(form.submit("Change the password")),
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
            let options = service.options();
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
            .name("auth.pages.sign_in"),
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
            .name("auth.pages.sign_in.submit"),
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
            .name("auth.pages.two_factor"),
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
            .name("auth.pages.two_factor.submit"),
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
            .name("auth.pages.forgot_password"),
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
            .name("auth.pages.forgot_password.submit"),
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
            .name("auth.pages.reset_password"),
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
            .name("auth.pages.reset_password.submit"),
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
                        let target = format!("{}?status=signed-out", paths.sign_in);
                        Ok::<_, AppError>(Reply::redirect(LocalHref::from(target)).into_response())
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.sign_out"),
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
        routes.extend(self.account_routes());
        routes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
