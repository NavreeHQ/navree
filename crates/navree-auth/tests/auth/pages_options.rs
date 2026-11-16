//! What an app says and shows on the sign-in pages with `AuthPages`' options: its own words
//! (each page's title and the line under it, and the fields' labels), a form with no card,
//! no line with the app's name, the links under a form drawn as buttons, the layout's own
//! options, and a Sign out button on the two-factor set-up page. What a page does is
//! unchanged: the same routes, the same posts with the same token, the same answers. A
//! browser that must set two-factor authentication up reaches the set-up routes and signing
//! out, and nothing else.

use std::collections::BTreeSet;

use http::Method;
use navree_auth::{AuthForm, AuthPages, AuthWords, ModuleBuilderAuthExt, TwoFactorPolicy, User};
use navree_contracts::{BoxError, BoxFuture};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestApp, TestAppBuilder, TestClient, TestResponse};
use navree_ui::marketing::Brand;
use navree_ui::{AuthLayout, ButtonVariant, UiModule, el};
use serde_json::json;

use crate::support::{self, PASSWORD, create_user, enable_two_factor};

const XSRF: &str = "XSRF-TOKEN";

/// The session's CSRF token, from the cookie the pages hand out.
async fn token(client: &TestClient) -> String {
    if client.cookie(XSRF).is_none() {
        open(client, "/sign-in").await.assert_status(200);
    }
    client.cookie(XSRF).expect("the XSRF-TOKEN cookie")
}

/// Posts a form as a browser without JavaScript does.
async fn post_form(client: &TestClient, path: &str, fields: &[(&str, &str)]) -> TestResponse {
    let csrf = token(client).await;
    let mut all = fields.to_vec();
    all.push(("_csrf", csrf.as_str()));
    client
        .post(path)
        .header("accept", "text/html")
        .form(&all)
        .send()
        .await
}

/// Opens `path` as a browser does.
async fn open(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

/// `builder` with the session layer, navree-ui and `pages`.
fn with(builder: TestAppBuilder, pages: AuthPages) -> TestAppBuilder {
    builder
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(UiModule)
        .module(pages)
}

/// Requires two-factor authentication of every user.
struct EveryoneTwoFactor;

impl Injectable for EveryoneTwoFactor {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(EveryoneTwoFactor) })
    }
}

impl TwoFactorPolicy for EveryoneTwoFactor {
    fn requires_two_factor<'a>(&'a self, _user: &'a User) -> BoxFuture<'a, Result<bool, BoxError>> {
        Box::pin(async { Ok(true) })
    }
}

struct EveryoneTwoFactorModule;

impl Module for EveryoneTwoFactorModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.two_factor_policy::<EveryoneTwoFactor>();
    }
}

/// Words of the app's own for every page and every field, each naming what it stands for.
fn words() -> AuthWords {
    AuthWords::new()
        .sign_in("Sign in to Agra CRM", "Deals, contacts and the week ahead.")
        .sign_up("Join Agra CRM", "A name, an address and a password.")
        .two_factor("One more step", "The code from your phone.")
        .forgot_password("Lost your password?", "A link to choose another.")
        .reset_password("A new password", "It signs you out on every device.")
        .magic_link("A link by email", "It signs you in once.")
        .magic_link_confirm("Almost in", "One press and you are in.")
        .code("A code by email", "Six digits, a few minutes.")
        .code_entry("Your code", "From the email we sent.")
        .passkey("Your passkey", "On this device or your phone.")
        .two_factor_setup("Protect your account", "A code as well as your password.")
        .two_factor_secret("Scan this", "Then type the code your app shows.")
        .recovery_codes("Keep these safe", "Each works once.")
        .two_factor_on("Protected", "Your account asks for a code.")
        .email_label("Work email")
        .password_label("Passphrase")
        .remember_label("Stay signed in")
        .name_label("Full name")
        .password_again_label("Passphrase again")
        .new_password_label("New passphrase")
        .new_password_again_label("New passphrase again")
        .authenticator_code_label("Authenticator code")
        .recovery_code_label("A recovery code instead")
        .emailed_code_label("Code from your inbox")
}

/// An app with every way in, sign-up and `pages`.
async fn every_way_in(pages: AuthPages) -> TestApp {
    with(
        support::mail_builder()
            .config("auth.magic_link.enabled", true)
            .config(
                "auth.magic_link.url",
                "https://app.example.com/sign-in/link/verify",
            )
            .config("auth.otp.enabled", true)
            .config("auth.passkeys.enabled", true)
            .config("auth.passkeys.rp_id", "example.com")
            .config("auth.passkeys.origins", json!(["https://app.example.com"])),
        pages.sign_up(),
    )
    .build()
    .await
}

/// `html`'s tab title, heading and the line under it.
fn says(html: &str, title: &str, lead: &str) {
    assert!(html.contains(&format!("<title>{title}</title>")), "{html}");
    assert!(
        html.contains(&format!(r#"<h1 data-slot="page-title">{title}</h1>"#)),
        "{html}"
    );
    assert!(html.contains(lead), "{html}");
}

/// The label of the field `name` in the form `form`.
fn label(html: &str, form: &str, name: &str, text: &str) {
    let label = format!(r#"<label data-slot="label" for="{form}-{name}">{text}</label>"#);
    let choice = format!(r#"id="{form}-{name}"><span>{text}</span></label>"#);
    assert!(
        html.contains(&label) || html.contains(&choice),
        "{name} labelled {text}: {html}"
    );
}

// Every page says the app's words: its tab, its heading, the line under it, its fields'
// labels; the forms post the same fields to the same routes, so signing in works as before.
#[tokio::test]
async fn every_page_says_the_apps_own_words_and_works_as_before() {
    let app = every_way_in(AuthPages::new().words(words())).await;
    create_user(&app, "anu@example.com").await;
    let guest = app.client();

    let sign_in = open(&guest, "/sign-in").await.assert_status(200).text();
    says(
        &sign_in,
        "Sign in to Agra CRM",
        "Deals, contacts and the week ahead.",
    );
    label(&sign_in, "sign-in-form", "email", "Work email");
    label(&sign_in, "sign-in-form", "password", "Passphrase");
    label(&sign_in, "sign-in-form", "remember", "Stay signed in");
    assert!(!sign_in.contains(">Email<") && !sign_in.contains(">Password<"));
    // What the form posts is as it was.
    assert!(sign_in.contains(r#"name="email""#) && sign_in.contains(r#"name="password""#));

    let sign_up = open(&guest, "/sign-up").await.assert_status(200).text();
    says(
        &sign_up,
        "Join Agra CRM",
        "A name, an address and a password.",
    );
    label(&sign_up, "sign-up-form", "name", "Full name");
    label(&sign_up, "sign-up-form", "email", "Work email");
    label(&sign_up, "sign-up-form", "password", "Passphrase");
    label(
        &sign_up,
        "sign-up-form",
        "password_confirmation",
        "Passphrase again",
    );

    let forgot = open(&guest, "/forgot-password")
        .await
        .assert_status(200)
        .text();
    says(&forgot, "Lost your password?", "A link to choose another.");
    label(&forgot, "forgot-form", "email", "Work email");

    let reset = open(&guest, "/reset-password?token=abc&email=anu%40example.com")
        .await
        .assert_status(200)
        .text();
    says(
        &reset,
        "A new password",
        "It signs you out on every device.",
    );
    label(&reset, "reset-form", "password", "New passphrase");
    label(
        &reset,
        "reset-form",
        "password_confirmation",
        "New passphrase again",
    );

    let link = open(&guest, "/sign-in/link")
        .await
        .assert_status(200)
        .text();
    says(&link, "A link by email", "It signs you in once.");
    label(&link, "link-form", "email", "Work email");
    let confirm = open(&guest, "/sign-in/link/verify?token=abc")
        .await
        .assert_status(200)
        .text();
    says(&confirm, "Almost in", "One press and you are in.");
    label(&confirm, "link-confirm-form", "remember", "Stay signed in");

    let code = open(&guest, "/sign-in/code")
        .await
        .assert_status(200)
        .text();
    says(&code, "A code by email", "Six digits, a few minutes.");
    label(&code, "code-request-form", "email", "Work email");
    let entry = post_form(&guest, "/sign-in/code", &[("email", "anu@example.com")])
        .await
        .assert_status(200)
        .text();
    says(&entry, "Your code", "From the email we sent.");
    label(&entry, "code-form", "code", "Code from your inbox");

    let passkey = open(&guest, "/sign-in/passkey")
        .await
        .assert_status(200)
        .text();
    says(&passkey, "Your passkey", "On this device or your phone.");

    // A wrong password draws the page again in the same words, and the right one signs in.
    let wrong = post_form(
        &guest,
        "/sign-in",
        &[("email", "anu@example.com"), ("password", "not it")],
    )
    .await
    .assert_status(422)
    .text();
    says(
        &wrong,
        "Sign in to Agra CRM",
        "Deals, contacts and the week ahead.",
    );
    label(&wrong, "sign-in-form", "email", "Work email");
    post_form(
        &guest,
        "/sign-in",
        &[("email", "anu@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);
    guest
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_status(200);
}

// The second factor and the set-up pages say the app's words too.
#[tokio::test]
async fn the_second_factor_and_its_set_up_say_the_apps_words() {
    let clock = support::clock();
    let app = with(support::clocked(&clock), AuthPages::new().words(words()))
        .build()
        .await;
    let user = create_user(&app, "anu@example.com").await;
    enable_two_factor(&app, &clock, &user).await;
    let browser = app.client();
    post_form(
        &browser,
        "/sign-in",
        &[("email", "anu@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);
    let page = open(&browser, "/sign-in/two-factor")
        .await
        .assert_status(200)
        .text();
    says(&page, "One more step", "The code from your phone.");
    label(&page, "two-factor-form", "code", "Authenticator code");
    label(
        &page,
        "two-factor-form",
        "recovery_code",
        "A recovery code instead",
    );

    let app = with(support::clocked(&clock), AuthPages::new().words(words()))
        .module(EveryoneTwoFactorModule)
        .build()
        .await;
    create_user(&app, "bal@example.com").await;
    let browser = app.client();
    post_form(
        &browser,
        "/sign-in",
        &[("email", "bal@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);
    let offer = open(&browser, "/account/two-factor")
        .await
        .assert_status(200)
        .text();
    says(
        &offer,
        "Protect your account",
        "A code as well as your password.",
    );
    let secret = post_form(&browser, "/account/two-factor", &[])
        .await
        .assert_status(200)
        .text();
    says(&secret, "Scan this", "Then type the code your app shows.");
    label(&secret, "setup-confirm-form", "code", "Authenticator code");
    let key = secret
        .split(r#"<code data-slot="code">"#)
        .nth(1)
        .and_then(|rest| rest.split("</code>").next())
        .expect("the key")
        .to_owned();
    let codes = post_form(
        &browser,
        "/account/two-factor/confirm",
        &[("code", support::totp_code(&key, &clock).as_str())],
    )
    .await
    .assert_status(200)
    .text();
    says(&codes, "Keep these safe", "Each works once.");
    let on = open(&browser, "/account/two-factor")
        .await
        .assert_status(200)
        .text();
    says(&on, "Protected", "Your account asks for a code.");
}

// Words are text: markup in them is escaped, in the heading, the line, a label and the tab.
#[tokio::test]
async fn the_apps_words_are_text() {
    let app = with(
        support::builder(),
        AuthPages::new().words(
            AuthWords::new()
                .sign_in("<b>Agra</b>", "<script>alert(1)</script>")
                .email_label("<i>Email</i>"),
        ),
    )
    .build()
    .await;
    let page = open(&app.client(), "/sign-in")
        .await
        .assert_status(200)
        .text();
    assert!(!page.contains("<b>Agra</b>") && !page.contains("<script>alert(1)"));
    assert!(!page.contains("<i>Email</i>"));
    assert!(page.contains("&lt;b&gt;Agra&lt;/b&gt;"), "{page}");
    assert!(
        page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{page}"
    );
    assert!(page.contains("&lt;i&gt;Email&lt;/i&gt;"), "{page}");
}

// A form with no card: the title, the line under it as quiet text, then the form, and no
// card on any page; the app's name line can go, while the panel keeps it in its name.
#[tokio::test]
async fn a_plain_form_has_no_card_and_the_name_line_can_go() {
    let app = every_way_in(
        AuthPages::new()
            .app_name("Agra CRM")
            .app_name_line(false)
            .form_style(AuthForm::Plain)
            .aside(el("p").child("Deals and the week ahead.")),
    )
    .await;
    create_user(&app, "anu@example.com").await;
    let guest = app.client();
    for path in [
        "/sign-in",
        "/sign-up",
        "/forgot-password",
        "/reset-password?token=abc&email=anu%40example.com",
        "/sign-in/link",
        "/sign-in/code",
        "/sign-in/passkey",
    ] {
        let page = open(&guest, path).await.assert_status(200).text();
        assert!(!page.contains(r#"data-slot="card""#), "{path}: {page}");
        assert!(
            !page.contains(r#"<p data-slot="text" data-variant="muted">Agra CRM</p>"#),
            "{path}: no name line: {page}"
        );
        assert!(
            page.contains(r#"aria-label="About Agra CRM""#),
            "{path}: the panel keeps the name: {page}"
        );
    }
    let sign_in = open(&guest, "/sign-in").await.text();
    assert!(
        sign_in.contains(r#"<div data-slot="stack" data-gap="6"><div data-slot="stack" data-gap="2"><h1 data-slot="page-title">Sign in</h1><p data-slot="text" data-variant="muted">Use the email address and password of your account.</p></div><div data-slot="stack" data-gap="4"><form data-slot="form" id="sign-in-form""#),
        "{sign_in}"
    );
    // With the name line kept, a plain form has it above the title.
    let app = every_way_in(
        AuthPages::new()
            .app_name("Agra CRM")
            .form_style(AuthForm::Plain),
    )
    .await;
    let page = open(&app.client(), "/sign-in").await.text();
    assert!(
        page.contains(r#"<div data-slot="stack" data-gap="2"><p data-slot="text" data-variant="muted">Agra CRM</p><h1 data-slot="page-title">Sign in</h1>"#),
        "{page}"
    );
    // In a card, the name line can go too.
    let app = every_way_in(AuthPages::new().app_name("Agra CRM").app_name_line(false)).await;
    let page = open(&app.client(), "/sign-in").await.text();
    assert!(page.contains(r#"data-slot="card""#), "{page}");
    assert!(!page.contains(">Agra CRM</p>"), "{page}");
}

// The links under a form are buttons of the chosen look, to the same places, the passkey's
// still a full load, and `next` still carried.
#[tokio::test]
async fn the_links_under_a_form_can_be_buttons() {
    let app = every_way_in(AuthPages::new().links_as(ButtonVariant::Ghost)).await;
    let guest = app.client();
    let page = open(&guest, "/sign-in?next=%2Fdeals")
        .await
        .assert_status(200)
        .text();
    for (href, text) in [
        ("/forgot-password", "Forgot your password?"),
        ("/sign-up?next=%2Fdeals", "New here? Create an account"),
        ("/sign-in/link?next=%2Fdeals", "Email me a sign-in link"),
        ("/sign-in/code?next=%2Fdeals", "Email me a sign-in code"),
    ] {
        let button = format!(
            r#"<a data-slot="button" data-variant="ghost" data-block href="{href}">{text}</a>"#
        );
        assert!(page.contains(&button), "{button}: {page}");
    }
    assert!(
        page.contains(r#"<a data-slot="button" data-variant="ghost" data-block href="/sign-in/passkey?next=%2Fdeals" data-ui-noboost>Sign in with a passkey</a>"#),
        "{page}"
    );
    assert!(
        !page.contains(r#"<a href="#),
        "no plain link is left: {page}"
    );
    for path in [
        "/sign-up",
        "/forgot-password",
        "/sign-in/link",
        "/sign-in/passkey",
    ] {
        let page = open(&guest, path).await.assert_status(200).text();
        assert!(
            page.contains(r#"<a data-slot="button" data-variant="ghost" data-block href="#),
            "{path}: {page}"
        );
        assert!(!page.contains(r#"<a href="#), "{path}: {page}");
    }
    // Another look is another variant.
    let app = every_way_in(AuthPages::new().links_as(ButtonVariant::Outline)).await;
    let page = open(&app.client(), "/sign-in").await.text();
    assert!(
        page.contains(r#"<a data-slot="button" data-variant="outline" data-block href="/forgot-password">Forgot your password?</a>"#),
        "{page}"
    );
}

// The layout's own options reach every page: the panel's side, inset and surface, a brand
// and a footer, the form's width.
#[tokio::test]
async fn every_page_takes_the_layouts_options() {
    let app = every_way_in(
        AuthPages::new()
            .aside(el("p").child("Deals and the week ahead."))
            .layout(|layout: AuthLayout| {
                layout
                    .aside_end()
                    .aside_inset()
                    .aside_inverse()
                    .brand(Brand::new("Agra CRM", "/"))
                    .footer("Every sign-in is checked.")
                    .body_width(400)
            }),
    )
    .await;
    let guest = app.client();
    for path in ["/sign-in", "/sign-up", "/forgot-password", "/sign-in/code"] {
        let page = open(&guest, path).await.assert_status(200).text();
        assert!(
            page.contains(r#"<div data-slot="auth-layout" data-aside="end inset" data-body="400"><main data-slot="auth-main" id="main"><div data-slot="auth-brand"><a data-slot="site-brand" href="/"><span data-slot="site-brand-name">Agra CRM</span></a></div>"#),
            "{path}: {page}"
        );
        assert!(
            page.contains(r#"<div data-slot="auth-footer">Every sign-in is checked.</div></main><aside data-slot="auth-aside" data-inverse aria-label="About">"#),
            "{path}: {page}"
        );
        assert!(
            page.contains("/_navree/ui/auth-layout-"),
            "{path}: its sheet"
        );
    }
}

/// The Sign out form of `html`: what it posts to and the token it carries.
fn sign_out_form(html: &str) -> Option<(String, String)> {
    let start = html.find(r#"<form method="post" action="/sign-out" data-ui-form>"#)?;
    let form = &html[start..];
    let form = &form[..form.find("</form>")?];
    let marker = r#"<input type="hidden" name="_csrf" value=""#;
    let token = form[form.find(marker)? + marker.len()..]
        .split('"')
        .next()?
        .to_owned();
    assert!(
        form.contains(r#"<button data-slot="button" type="submit">Sign out</button>"#),
        "{form}"
    );
    Some(("/sign-out".to_owned(), token))
}

// A browser a policy makes set two-factor authentication up can sign out from the set-up
// page: the button posts to the sign-out route with the session's token, as the user menu's
// does, and the browser is a guest after it. Without the option the page has no such form.
#[tokio::test]
async fn the_set_up_page_signs_out_a_browser_that_must_set_up() {
    let app = with(support::builder(), AuthPages::new().setup_sign_out())
        .module(EveryoneTwoFactorModule)
        .build()
        .await;
    create_user(&app, "anu@example.com").await;
    let browser = app.client();
    post_form(
        &browser,
        "/sign-in",
        &[("email", "anu@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);
    browser
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_error_code(403, "two_factor_required");
    let offer = open(&browser, "/account/two-factor")
        .await
        .assert_status(200)
        .text();
    let (action, token) = sign_out_form(&offer).expect("a Sign out form on the offer");
    // The page with the secret has one too.
    let secret = post_form(&browser, "/account/two-factor", &[])
        .await
        .assert_status(200)
        .text();
    assert!(sign_out_form(&secret).is_some(), "{secret}");
    // Without the session's token the post is refused, and nothing changes.
    browser
        .post(&action)
        .header("accept", "text/html")
        .form(&[("_csrf", "not the token")])
        .send()
        .await
        .assert_status(403);
    browser
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_error_code(403, "two_factor_required");
    let out = browser
        .post(&action)
        .header("accept", "text/html")
        .form(&[("_csrf", token.as_str())])
        .send()
        .await
        .assert_status(303);
    assert_eq!(out.header("location"), Some("/sign-in?status=signed-out"));
    browser
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_status(401);

    // An app that doesn't ask for it draws no Sign out on the page.
    let plain = with(support::builder(), AuthPages::new())
        .module(EveryoneTwoFactorModule)
        .build()
        .await;
    create_user(&plain, "bal@example.com").await;
    let browser = plain.client();
    post_form(
        &browser,
        "/sign-in",
        &[("email", "bal@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);
    let offer = open(&browser, "/account/two-factor").await.text();
    assert!(sign_out_form(&offer).is_none(), "{offer}");
}

// While two-factor authentication waits to be set up, the browser reaches the set-up routes
// and signing out only: every other route that needs a sign-in, page or JSON, refuses it with
// 403 `two_factor_required`, with the Sign out button on the page as without it.
#[tokio::test]
async fn a_browser_that_must_set_up_reaches_the_set_up_and_signing_out_and_nothing_else() {
    let app = with(support::builder(), AuthPages::new().setup_sign_out())
        .module(EveryoneTwoFactorModule)
        .build()
        .await;
    create_user(&app, "anu@example.com").await;
    let browser = app.client();
    post_form(
        &browser,
        "/sign-in",
        &[("email", "anu@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);
    let reachable: BTreeSet<(&str, &str)> = [
        ("GET", "/account/two-factor"),
        ("POST", "/account/two-factor"),
        ("POST", "/account/two-factor/confirm"),
        ("POST", "/api/auth/2fa/enable"),
        ("POST", "/api/auth/2fa/confirm"),
        ("GET", "/api/auth/me"),
        ("POST", "/api/auth/logout"),
        ("POST", "/api/auth/refresh"),
    ]
    .into();
    let routes = navree_http::route_table(app.ctx()).await.unwrap();
    let mut refused = 0;
    for route in &routes {
        if route.access.public
            || reachable.contains(&(route.method.as_str(), route.full_path.as_str()))
        {
            continue;
        }
        // Each `{name}` is an id; the refusal comes before the route reads it.
        let path: String = route
            .full_path
            .split('/')
            .map(|part| {
                if part.starts_with('{') {
                    "00000000-0000-0000-0000-000000000000"
                } else {
                    part
                }
            })
            .collect::<Vec<_>>()
            .join("/");
        let method = Method::from_bytes(route.method.as_bytes()).unwrap();
        let response = browser
            .request(method, &path)
            .header("accept", "application/json")
            .xsrf()
            .json(&json!({}))
            .send()
            .await;
        let label = format!("{} {}", route.method, route.full_path);
        assert_eq!(
            response.status().as_u16(),
            403,
            "{label}: {}",
            response.text()
        );
        assert!(
            response.text().contains("two_factor_required"),
            "{label}: {}",
            response.text()
        );
        refused += 1;
    }
    assert!(
        refused >= 10,
        "the routes that need a sign-in were tried: {refused}"
    );
    // The sign-out route is public, so a browser that must set up reaches it, and the set-up
    // page's button signs it out.
    let sign_out = routes
        .iter()
        .find(|route| route.method == "POST" && route.full_path == "/sign-out")
        .expect("the sign-out route");
    assert!(sign_out.access.public);
    let offer = open(&browser, "/account/two-factor")
        .await
        .assert_status(200)
        .text();
    let (action, token) = sign_out_form(&offer).expect("a Sign out form");
    browser
        .post(&action)
        .header("accept", "text/html")
        .form(&[("_csrf", token.as_str())])
        .send()
        .await
        .assert_status(303);
    browser
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_status(401);
}
