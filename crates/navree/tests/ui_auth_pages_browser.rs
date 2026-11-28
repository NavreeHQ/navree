//! The sign-in pages in an app's own words and a design's layout, in a real Chrome: the app's
//! heading and labels, a form with no card, the links under it drawn as buttons as wide as the
//! form, the panel after the form, inset and on the inverse surface, the brand in the form's
//! column that stays on a phone, and a person whom a policy makes set two-factor
//! authentication up signing out from the set-up page with its Sign out button.
//!
//! `cargo test -p navree --features access,auth,browser,crypto,db,http,session,sqlite,testing,ui --test ui_auth_pages_browser`
//!
//! It needs Chrome or Chromium (`NAVREE_BROWSER_CHROME`); without one it
//! prints a notice and passes, unless `NAVREE_TEST_REQUIRE_SERVICES=1`.

use navree::access::{AccessModule, AccessOptions};
use navree::auth::{
    AuthForm, AuthModule, AuthPages, AuthService, AuthWords, TwoFactorPolicy, User,
};
use navree::browser::{Browser, BrowserError, TestServer};
use navree::crypto::{CryptoModule, generate_key};
use navree::db::DatabaseModule;
use navree::kernel::{Injectable, KernelError, Resolver};
use navree::prelude::*;
use navree::session::{SessionModule, SessionOptions};
use navree::testing::TestApp;
use navree::ui::marketing::Brand;
use navree::ui::{AuthLayout, ButtonVariant, Text, UiModule, el};
use navree::{BoxError, BoxFuture};
use serde_json::Value;

const PASSWORD: &str = "correct horse battery";

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

struct PolicyModule;

impl Module for PolicyModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.two_factor_policy::<EveryoneTwoFactor>();
    }
}

/// The pages as a design draws them.
fn pages() -> AuthPages {
    AuthPages::new()
        .app_name("Agra CRM")
        .app_name_line(false)
        .form_style(AuthForm::Plain)
        .links_as(ButtonVariant::Ghost)
        .words(
            AuthWords::new()
                .sign_in(
                    "Sign in to Agra CRM",
                    "Your deals, your contacts and the week ahead.",
                )
                .email_label("Work email"),
        )
        .aside(el("p").child("Deals, contacts and the week ahead, in one place."))
        .layout(|layout: AuthLayout| {
            layout
                .brand(Brand::new("Agra CRM", "/"))
                .footer(Text::muted("Every sign-in is checked for a new device."))
                .aside_end()
                .aside_inset()
                .aside_inverse()
                .body_width(400)
        })
        .setup_sign_out()
}

/// What the page draws, measured in the browser.
const MEASURE: &str = r#"
    const box = (selector) => {
        const found = document.querySelector(selector);
        if (!found) return null;
        const r = found.getBoundingClientRect();
        return { x: Math.round(r.left), right: Math.round(r.right), width: Math.round(r.width), shown: getComputedStyle(found).display !== "none" && r.width > 0 };
    };
    const aside = document.querySelector("[data-slot=auth-aside]");
    const style = getComputedStyle(aside);
    return {
        cards: document.querySelectorAll("[data-slot=auth-body] [data-slot=card]").length,
        nameLine: [...document.querySelectorAll("[data-slot=auth-body] [data-slot=text]")].some((p) => p.textContent === "Agra CRM"),
        links: [...document.querySelectorAll("[data-slot=auth-body] a")].map((a) => [a.getAttribute("data-slot"), a.getAttribute("data-variant"), Math.round(a.getBoundingClientRect().width)]),
        body: box("[data-slot=auth-body]"),
        main: box("[data-slot=auth-main]"),
        aside: box("[data-slot=auth-aside]"),
        brand: box("[data-slot=auth-brand]"),
        footer: box("[data-slot=auth-footer]"),
        margin: style.marginTop,
        page: getComputedStyle(document.querySelector("[data-slot=auth-layout]")).backgroundColor,
        panel: style.backgroundColor,
    };
"#;

#[tokio::test]
async fn the_sign_in_pages_in_the_apps_words_and_a_designs_layout() -> Result<(), BrowserError> {
    if let Err(error) = Browser::find_chrome() {
        if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") {
            panic!("ui_auth_pages_browser: {error} (NAVREE_TEST_REQUIRE_SERVICES=1)");
        }
        eprintln!("skipped ui_auth_pages_browser: {error}");
        return Ok(());
    }
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        // The test server speaks plain HTTP on 127.0.0.1: no `Secure`, so no `__Host-`
        // prefix either.
        .module(SessionModule::for_root(SessionOptions {
            secure: false,
            ..SessionOptions::default()
        }))
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(PolicyModule)
        .module(UiModule)
        .module(pages())
        .build()
        .await;
    let auth = app.get::<AuthService>().await;
    auth.create_user("Anu Lakhotia", "anu@example.com", PASSWORD)
        .await
        .unwrap();

    let server = TestServer::start(app.ctx()).await?;
    let browser = Browser::launch().await?;
    let page = browser.visit(server.url("/sign-in")).await?;
    page.resize(1440, 980).await?;
    page.assert_title("Sign in to Agra CRM").await?;
    page.assert_see("Your deals, your contacts and the week ahead.")
        .await?;

    // No card, the links as ghost buttons as wide as the form, 400 px; the form's column
    // first, the panel after it, 12 px off the edges, on the inverse surface.
    let wide: Value = page.script(MEASURE).await?;
    assert_eq!(wide["cards"], 0, "{wide}");
    assert_eq!(wide["nameLine"], false, "{wide}");
    assert_eq!(
        wide["links"],
        serde_json::json!([["button", "ghost", 400]]),
        "{wide}"
    );
    assert_eq!(wide["body"]["width"], 400, "{wide}");
    assert_eq!(wide["main"]["x"], 0, "{wide}");
    assert_eq!(
        wide["aside"]["x"].as_i64(),
        wide["main"]["right"].as_i64().map(|right| right + 12),
        "{wide}"
    );
    assert_eq!(wide["margin"], "12px", "{wide}");
    assert_ne!(wide["panel"], wide["page"], "{wide}");
    assert_eq!(wide["brand"]["shown"], true, "{wide}");
    assert_eq!(wide["footer"]["shown"], true, "{wide}");

    // On a phone the panel is hidden and the brand stays.
    page.resize(390, 844).await?;
    let narrow: Value = page.script(MEASURE).await?;
    assert_eq!(narrow["aside"]["shown"], false, "{narrow}");
    assert_eq!(narrow["brand"]["shown"], true, "{narrow}");
    page.resize(1440, 980).await?;

    // The form posts as before under its own labels; a policy sends the person to set
    // two-factor authentication up, where the Sign out button signs them out.
    page.type_into("Work email", "anu@example.com").await?;
    page.type_into("Password", PASSWORD).await?;
    page.press("Sign in").await?;
    page.assert_path_is("/account/two-factor").await?;
    page.assert_see("Set up two-factor authentication").await?;
    page.press("Sign out").await?;
    page.assert_path_is("/sign-in").await?;
    page.assert_see("You are signed out.").await?;
    let again = browser.visit(server.url("/account/two-factor")).await?;
    again
        .assert_dont_see("Set up two-factor authentication")
        .await?;
    page.assert_no_console_errors().await?;
    Ok(())
}
