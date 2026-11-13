//! The sign-up page, where a browser goes after signing out, and the pages drawn in
//! the app's look: through the full router with navree-session's layer, as a
//! browser drives them.

use std::sync::Arc;

use navree_auth::AuthPages;
use navree_kernel::{Factory, Module, ModuleBuilder};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestApp, TestAppBuilder, TestClient, TestResponse};
use navree_ui::theme::{Appearance, Mode, Theme};
use navree_ui::{AppSheet, Shell, UiModule, el};

use crate::support::{self, PASSWORD, create_user};

const XSRF: &str = "XSRF-TOKEN";

static BRAND: AppSheet = AppSheet::new("brand", b"[data-slot=auth-aside]{color:red}", 1024);

/// `builder` with the session layer, navree-ui and `pages`.
fn with(builder: TestAppBuilder, pages: AuthPages) -> TestAppBuilder {
    builder
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(UiModule)
        .module(pages)
}

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

/// The fields of a sign-up.
fn sign_up<'a>(email: &'a str, password: &'a str, again: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("name", "Simran Kaur"),
        ("email", email),
        ("password", password),
        ("password_confirmation", again),
        ("next", "/deals/42"),
    ]
}

// A guest makes an account on the page, is signed in at once and goes on to
// where they were headed; the page refuses passwords that differ, an address
// already taken and a post without the token, and a client that keeps posting
// is slowed down.
#[tokio::test]
async fn a_guest_signs_up_and_is_signed_in() {
    let app = with(support::builder(), AuthPages::new().sign_up())
        .build()
        .await;
    let browser = app.client();

    let sign_in = open(&browser, "/sign-in").await.assert_status(200).text();
    assert!(sign_in.contains(r#"href="/sign-up""#), "{sign_in}");
    let page = open(&browser, "/sign-up?next=%2Fdeals%2F42")
        .await
        .assert_status(200)
        .text();
    assert!(page.contains(r#"action="/sign-up""#), "{page}");
    assert!(page.contains(r#"name="_csrf""#), "{page}");
    assert!(page.contains(r#"name="password_confirmation""#), "{page}");
    assert!(
        page.contains(r#"value="/deals/42""#),
        "the next travels: {page}"
    );
    assert!(
        page.contains(r#"href="/sign-in?next=%2Fdeals%2F42""#),
        "{page}"
    );

    // Without the token, nothing happens.
    browser
        .post("/sign-up")
        .header("accept", "text/html")
        .form(&sign_up("simran@example.com", PASSWORD, PASSWORD))
        .send()
        .await
        .assert_status(403);

    let differ = post_form(
        &browser,
        "/sign-up",
        &sign_up("simran@example.com", PASSWORD, "something else"),
    )
    .await
    .assert_status(422)
    .text();
    assert!(
        differ.contains("The two passwords are not the same."),
        "{differ}"
    );
    assert!(
        !differ.contains(PASSWORD),
        "a password is never drawn again: {differ}"
    );

    let made = post_form(
        &browser,
        "/sign-up",
        &sign_up("simran@example.com", PASSWORD, PASSWORD),
    )
    .await
    .assert_status(303);
    assert_eq!(made.header("location"), Some("/deals/42"));
    browser
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_status(200);
    // Signed in, the page sends the browser on.
    open(&browser, "/sign-up").await.assert_status(303);

    let other = app.client();
    let taken = post_form(
        &other,
        "/sign-up",
        &sign_up("Simran@Example.com", PASSWORD, PASSWORD),
    )
    .await
    .assert_status(422)
    .text();
    assert!(
        taken.contains("An account already uses this address."),
        "{taken}"
    );
    other
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_status(401);

    // A next off the site is never followed.
    let off = post_form(
        &other,
        "/sign-up",
        &[
            ("name", "Arjun Mehta"),
            ("email", "arjun@example.com"),
            ("password", PASSWORD),
            ("password_confirmation", PASSWORD),
            ("next", "//evil.example"),
        ],
    )
    .await
    .assert_status(303);
    assert_eq!(off.header("location"), Some("/"));

    // Five posts a minute from one client; the sixth waits.
    let third = app.client();
    post_form(
        &third,
        "/sign-up",
        &sign_up("kavya@example.com", PASSWORD, "x"),
    )
    .await
    .assert_status(422);
    post_form(
        &third,
        "/sign-up",
        &sign_up("kavya@example.com", PASSWORD, PASSWORD),
    )
    .await
    .assert_status(429);
}

// The page is there only when the app asks for it and accounts may be made.
#[tokio::test]
async fn the_sign_up_page_is_off_unless_asked_for_and_allowed() {
    let app = with(support::builder(), AuthPages::new()).build().await;
    open(&app.client(), "/sign-up").await.assert_status(404);
    let sign_in = open(&app.client(), "/sign-in").await.text();
    assert!(!sign_in.contains("/sign-up"), "{sign_in}");

    let closed = with(
        support::builder().config("auth.registration", false),
        AuthPages::new().sign_up_path("/join"),
    )
    .build()
    .await;
    open(&closed.client(), "/join").await.assert_status(404);

    let moved = with(
        support::builder(),
        AuthPages::new()
            .sign_up_path("/join")
            .after_sign_up("/welcome"),
    )
    .build()
    .await;
    let browser = moved.client();
    open(&browser, "/join").await.assert_status(200);
    let made = post_form(
        &browser,
        "/join",
        &[
            ("name", "Ishaan Verma"),
            ("email", "ishaan@example.com"),
            ("password", PASSWORD),
            ("password_confirmation", PASSWORD),
        ],
    )
    .await
    .assert_status(303);
    assert_eq!(made.header("location"), Some("/welcome"));
}

// Signing out goes where the app says: its public home page, say.
#[tokio::test]
async fn sign_out_goes_where_the_app_says() {
    let app = with(support::builder(), AuthPages::new().after_sign_out("/"))
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
    let out = post_form(&browser, "/sign-out", &[])
        .await
        .assert_status(303);
    assert_eq!(out.header("location"), Some("/"));
    browser
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_status(401);

    // A place off the site is not taken: the sign-in page stays.
    let app = with(
        support::builder(),
        AuthPages::new().after_sign_out("https://evil.example/"),
    )
    .build()
    .await;
    let out = post_form(&app.client(), "/sign-out", &[])
        .await
        .assert_status(303);
    assert_eq!(out.header("location"), Some("/sign-in?status=signed-out"));
}

/// Registers the app's shell: light by default, a stylesheet of its own, a theme menu and
/// a title template.
struct ShellModule;

impl Module for ShellModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(Factory::<Shell>::new(|_| async {
            Ok(Arc::new(
                Shell::new("Kesar")
                    .appearance(Appearance::new(Theme::OCEAN, Mode::Light))
                    .themes([Theme::OCEAN])
                    .stylesheet(&BRAND)
                    .theme_menu()
                    .title_template("{} · Kesar"),
            ))
        }))
        .exports::<Shell>();
        m.contribute(BRAND);
    }
}

// The pages are drawn in the app's look: the shell's theme, mode, stylesheet and title,
// the reader's own mode over them, the app's panel beside the form and a menu of modes
// (the shell offers one theme) that comes back to the page with its `next`.
#[tokio::test]
async fn the_pages_take_the_apps_look_and_the_readers_choice() {
    let app = with(
        support::builder().module(ShellModule),
        AuthPages::new()
            .app_name("Kesar")
            .sign_up()
            .aside(el("p").child("Sleep, heart and steps, every day.")),
    )
    .build()
    .await;
    let page = open(&app.client(), "/sign-in?next=%2Fdeals&mode=dark")
        .await
        .assert_status(200)
        .text();
    assert!(
        page.contains(r#"<html lang="en" data-theme="ocean" data-mode="dark""#),
        "the reader's mode over the app's theme: {page}"
    );
    assert!(page.contains("<title>Sign in · Kesar</title>"), "{page}");
    assert!(page.contains(&BRAND.url()), "{page}");
    assert!(
        page.contains(r#"<aside data-slot="auth-aside" aria-label="About Kesar"><p>Sleep, heart and steps, every day.</p></aside>"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<main data-slot="auth-main" id="main">"#),
        "{page}"
    );
    assert!(
        page.contains(r#"action="/_navree/ui/appearance""#),
        "{page}"
    );
    assert!(
        page.contains(r#"name="back" value="/sign-in?next=%2Fdeals""#),
        "{page}"
    );
    assert!(
        !page.contains(r#"name="theme""#),
        "one theme on offer, so modes only: {page}"
    );

    // With nothing chosen, the app's own look; the reader's cookie wins over it.
    let plain = open(&app.client(), "/sign-up").await.text();
    assert!(
        plain.contains(r#"data-theme="ocean" data-mode="light""#),
        "{plain}"
    );
    let reader = app.client();
    let chosen = reader
        .get("/sign-in")
        .header("accept", "text/html")
        .header("cookie", "navree_ui_theme=dark")
        .send()
        .await
        .text();
    assert!(
        chosen.contains(r#"data-theme="ocean" data-mode="dark""#),
        "{chosen}"
    );
}

// The reader's theme toggle posts without the session's token, from this site only,
// for a reader who has a session.
#[tokio::test]
async fn the_theme_menu_posts_from_this_site_without_the_token() {
    let app = with(support::builder(), AuthPages::new().theme_menu())
        .build()
        .await;
    let browser = app.client();
    token(&browser).await;
    let back = browser
        .post("/_navree/ui/appearance")
        .header("sec-fetch-site", "same-origin")
        .form(&[("mode", "light"), ("back", "/sign-in?next=%2Fdeals")])
        .send()
        .await
        .assert_status(303);
    assert_eq!(back.header("location"), Some("/sign-in?next=%2Fdeals"));
    browser
        .post("/_navree/ui/appearance")
        .header("sec-fetch-site", "cross-site")
        .form(&[("mode", "light"), ("back", "/sign-in")])
        .send()
        .await
        .assert_status(403);
    let _: &TestApp = &app;
}

/// A form's path, the page its post is sent back to, and its fields.
type Expired = (
    &'static str,
    &'static str,
    Vec<(&'static str, &'static str)>,
);

// Login CSRF: a page on a sibling subdomain, or a browser that says nothing of where it
// posts from, sends the attacker's details to the sign-in or sign-up form of a reader who
// has no session yet. Every form of the pages is drawn by a GET that starts the session,
// so a post without its cookie is refused before anything runs, and nobody is signed in.
#[tokio::test]
async fn a_post_with_no_session_cookie_signs_nobody_in() {
    let app = with(
        support::mail_builder()
            .config("auth.magic_link.enabled", true)
            .config(
                "auth.magic_link.url",
                "https://app.example.com/sign-in/link/verify",
            )
            .config("auth.otp.enabled", true),
        AuthPages::new().sign_up(),
    )
    .build()
    .await;
    create_user(&app, "attacker@example.com").await;
    // Each form, and the page its post is sent back to: its own, or where the step starts.
    let posts: [Expired; 7] = [
        (
            "/sign-in",
            "/sign-in?status=expired",
            vec![("email", "attacker@example.com"), ("password", PASSWORD)],
        ),
        (
            "/sign-up",
            "/sign-up?status=expired",
            sign_up("mallory@example.com", PASSWORD, PASSWORD),
        ),
        (
            "/forgot-password",
            "/forgot-password?status=expired",
            vec![("email", "attacker@example.com")],
        ),
        (
            "/reset-password",
            "/forgot-password?status=expired",
            vec![
                ("email", "attacker@example.com"),
                ("token", "x"),
                ("password", PASSWORD),
                ("password_confirmation", PASSWORD),
            ],
        ),
        (
            "/sign-in/link",
            "/sign-in/link?status=expired",
            vec![("email", "attacker@example.com")],
        ),
        (
            "/sign-in/code",
            "/sign-in/code?status=expired",
            vec![("email", "attacker@example.com")],
        ),
        (
            "/sign-in/code/verify",
            "/sign-in?status=expired",
            vec![("email", "attacker@example.com"), ("code", "123456")],
        ),
    ];
    for (path, back, fields) in &posts {
        for site in [Some("same-site"), Some("none"), None] {
            let victim = app.client();
            let mut request = victim.post(path).header("accept", "text/html");
            if let Some(site) = site {
                request = request.header("sec-fetch-site", site);
            }
            // Sent back to the form, as one left open past the session is: nothing ran.
            let answer = request.form(fields).send().await.assert_status(303);
            assert_eq!(answer.header("location"), Some(*back), "{path}");
            // A post that isn't a page's form gets the plain refusal (once a path: the
            // routes' throttles count these too).
            if site.is_none() {
                victim
                    .post(path)
                    .json(&serde_json::json!({ "email": "attacker@example.com" }))
                    .send()
                    .await
                    .assert_status(403);
            }
            victim
                .get("/api/probe/whoami")
                .send()
                .await
                .assert_status(401);
        }
    }
    let made = app
        .get::<navree_auth::AuthService>()
        .await
        .find_by_email("mallory@example.com")
        .await
        .unwrap();
    assert!(made.is_none(), "no account was made");
    assert!(
        support::sent_mail(&app).await.is_empty(),
        "no mail was sent"
    );

    // The same forms, drawn first as a browser does, work.
    let browser = app.client();
    post_form(
        &browser,
        "/sign-in",
        &[("email", "attacker@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);
    browser
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_status(200);
}

// A form left open past the session's lifetime comes back without the session's cookie (the
// browser let it go). It is sent back to its page, which says the session expired and holds
// none of what was typed; signing out then lands where signing out lands. Each refusal is
// logged in the audit trail as the session layer logs its own.
#[tokio::test]
async fn a_form_posted_after_the_session_expired_is_drawn_again_with_a_notice() {
    let sink = Arc::new(crate::support::RecordingSink::default());
    let app = with(
        support::builder().override_value::<dyn navree_contracts::AuditSink>(sink.clone()),
        AuthPages::new().sign_up().after_sign_out("/"),
    )
    .build()
    .await;
    let expired = app.client();
    let back = expired
        .post("/sign-up")
        .header("accept", "text/html")
        .form(&sign_up(
            "simran@example.com",
            "a secret typed here",
            "a secret typed here",
        ))
        .send()
        .await
        .assert_status(303);
    let to = back.header("location").expect("a page").to_owned();
    assert_eq!(to, "/sign-up?status=expired");
    let page = open(&expired, &to).await.assert_status(200).text();
    assert!(
        page.contains("Your session expired. Please try again."),
        "{page}"
    );
    assert!(
        !page.contains("simran@example.com") && !page.contains("a secret"),
        "{page}"
    );
    assert!(
        page.contains(r#"name="_csrf""#),
        "a new form to post: {page}"
    );
    // Drawn again, the form works.
    post_form(
        &expired,
        "/sign-up",
        &sign_up("simran@example.com", PASSWORD, PASSWORD),
    )
    .await
    .assert_status(303);

    // The sign-in page says it in its own words.
    let sign_in = open(&app.client(), "/sign-in?status=expired").await.text();
    assert!(sign_in.contains("Sign in again."), "{sign_in}");

    // Signing out with the session gone lands where signing out does.
    let out = app
        .client()
        .post("/sign-out")
        .header("accept", "text/html")
        .form(&[("x", "1")])
        .send()
        .await
        .assert_status(303);
    assert_eq!(out.header("location"), Some("/"));

    let refusals = sink.with_action("csrf.mismatch");
    assert_eq!(refusals.len(), 2, "{refusals:?}");
    assert!(
        refusals
            .iter()
            .all(|entry| entry.metadata["reason"] == "no_session")
    );
    assert_eq!(refusals[0].metadata["path"], "/sign-up");
}
