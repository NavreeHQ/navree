//! The pages that belong to no feature: sign in, sign out and the profile.
//!
//! | Route | Answers |
//! |---|---|
//! | `GET /sign-in` | the sign-in form; `?next=` is where to go afterwards |
//! | `POST /sign-in` | signs in, or draws the form again with what is wrong |
//! | `POST /sign-out` | signs out and goes to the sign-in page |
//! | `GET /profile` | who is signed in |

use std::time::Duration;

use http::Method;
use http::request::Parts;
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::prelude::*;
use navree::ui::form::{FieldErrors, Rejected};
use navree::ui::*;
use serde::Deserialize;

use super::auth::{Browser, CurrentAccount, SignInError};

/// What the sign-in page reads from its address.
#[derive(Deserialize)]
struct SignInQuery {
    next: Option<String>,
}

/// The sign-in form: who, the password, and where to go afterwards.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct SignInForm {
    /// Email
    #[validate(email)]
    #[form(
        placeholder = "name@example.com",
        autofocus,
        error = "Enter your email address."
    )]
    pub email: String,
    /// Password
    #[validate(length(min = 1))]
    #[form(error = "Enter your password.")]
    pub password: String,
    #[validate(skip)]
    #[form(widget = hidden)]
    pub next: Option<String>,
}

/// The sign-in page: on its own, with no sidebar, since a guest has no use for it.
fn sign_in_page(form: FormView<SignInForm>) -> View {
    View::new("Sign in").body(el("main").slot("page").children((
        el("h1").slot("page-title").child("Sign in"),
        Card::new().body(form),
    )))
}

/// The errors of a sign-in that failed, on the password field.
fn problem(message: &str) -> FieldErrors {
    FieldErrors::from([("password".to_owned(), vec![message.to_owned()])])
}

/// The form again, with what was typed and what is wrong. With the page's
/// script on, the errors appear under the fields instead.
fn again(rejected: Rejected<SignInForm>, browser: &Browser) -> Response {
    let token = browser.csrf_token();
    rejected.respond(|form| sign_in_page(form.submit("Sign in").csrf(token)))
}

/// `GET /sign-in`.
async fn sign_in_form(
    account: Option<CurrentAccount>,
    browser: Browser,
    Query(query): Query<SignInQuery>,
) -> Response {
    if account.is_some() {
        return Reply::redirect(ReturnTo::resolve_for(
            query.next.as_deref(),
            "/sign-in",
            "/",
        ))
        .into_response();
    }
    let next = query
        .next
        .as_deref()
        .and_then(|raw| ReturnTo::parse_for(raw, "/sign-in"))
        .map(|found| found.as_str().to_owned());
    let form = FormView::<SignInForm>::new()
        .of(&SignInForm {
            email: String::new(),
            password: String::new(),
            next,
        })
        .post("/sign-in")
        .submit("Sign in")
        .csrf(browser.csrf_token());
    sign_in_page(form).into_response()
}

/// `POST /sign-in`.
async fn sign_in(
    parts: Parts,
    browser: Browser,
    sent: Result<Submit<SignInForm>, Rejected<SignInForm>>,
) -> Response {
    let form = match sent {
        Ok(Submit(form)) => form,
        Err(rejected) => return again(rejected, &browser),
    };
    match browser.sign_in(&form.email, &form.password).await {
        Ok(_) => Reply::redirect(ReturnTo::resolve_for(form.next.as_deref(), "/sign-in", "/"))
            .into_response(),
        Err(SignInError::Wrong) => {
            let errors = problem("Check your email and password, then try again.");
            again(Rejected::after_decode(&form, errors, &parts), &browser)
        }
        Err(SignInError::Refused(reason)) => again(
            Rejected::after_decode(&form, problem(reason), &parts),
            &browser,
        ),
        Err(SignInError::Failed(error)) => (*error).into_response(),
    }
}

/// `POST /sign-out`.
async fn sign_out(browser: Browser) -> Reply {
    browser.sign_out();
    Reply::redirect("/sign-in")
}

/// `GET /profile`.
async fn profile(shell: AppShell, account: CurrentAccount) -> AppPage {
    shell
        .page("Profile")
        .crumb("Home", "/")
        .body(FactGrid::new().cols(2).cards([
            FactCard::new("Name", account.name).icon(icons::USER),
            FactCard::new("Email", account.email).icon(icons::MAIL),
        ]))
}

/// The routes above.
#[injectable]
pub struct AccountPages;

impl Controller for AccountPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/sign-in", sign_in_form)
                .name("sign-in")
                .public()
                .page(),
            // Thirty tries a minute from one address: it slows guessing without
            // locking out the people behind one office router.
            RouteDef::on(Method::POST, "!/sign-in", sign_in)
                .name("sign-in.submit")
                .public()
                .page()
                .throttle(30, Duration::from_secs(60)),
            RouteDef::on(Method::POST, "!/sign-out", sign_out)
                .name("sign-out")
                .public()
                .page(),
            RouteDef::on(Method::GET, "!/profile", profile)
                .name("profile")
                .authenticated()
                .page(),
        ]
    }
}
