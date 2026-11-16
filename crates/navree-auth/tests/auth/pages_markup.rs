//! The markup of every page `AuthPages` draws, held byte for byte. The pins were taken from
//! the pages as they were before `AuthPages` and `AuthLayout` took options of their own (the
//! words, the form's look, the links' look, the side of the panel, its inset and surface, a
//! brand and a footer): an app that uses none of them gets exactly the pages it got before,
//! so each page is held here. What changes from one run to the next is masked: the session's
//! CSRF token, the hashes in the addresses of the toolkit's files, a two-factor secret, its
//! picture and the recovery codes.

use std::sync::Arc;

use navree_auth::{AuthPages, ModuleBuilderAuthExt, TwoFactorPolicy, User};
use navree_contracts::{BoxError, BoxFuture, FixedClock};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestAppBuilder, TestClient, TestResponse};
use navree_ui::{UiModule, el};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::support::{self, PASSWORD, create_user, enable_two_factor, sent_mail, totp_code};

const XSRF: &str = "XSRF-TOKEN";

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `html` with every run of exactly twelve hex digits (a file's hash in its address) made
/// `HASH`: what a page links changes with the toolkit's files, its markup must not.
fn unhashed(html: &str) -> String {
    let bytes = html.as_bytes();
    let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let word = |b: u8| b.is_ascii_alphanumeric();
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    while at < bytes.len() {
        let run = bytes[at..].iter().take_while(|b| hex(**b)).count();
        let before = at.checked_sub(1).is_some_and(|i| word(bytes[i]));
        let after = bytes.get(at + run).is_some_and(|b| word(*b));
        if run == 12 && !before && !after {
            out.push_str("HASH");
            at += run;
        } else if run > 0 {
            out.push_str(&html[at..at + run]);
            at += run;
        } else {
            let len = html[at..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&html[at..at + len]);
            at += len;
        }
    }
    out
}

/// `html` with the text between each `open` and the next `close` after it made `what`.
fn masked(html: &str, open: &str, close: &str, what: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find(open) {
        let from = start + open.len();
        let Some(len) = rest[from..].find(close) else {
            break;
        };
        out.push_str(&rest[..from]);
        out.push_str(what);
        rest = &rest[from + len..];
    }
    out.push_str(rest);
    out
}

/// A page as it is held: the hashes, the CSRF token, a secret's picture and the codes masked.
fn held(html: &str) -> String {
    let html = unhashed(html);
    let html = masked(&html, r#"name="_csrf" value=""#, "\"", "TOKEN");
    let html = masked(
        &html,
        r#"role="img" aria-label="QR code for your authenticator app">"#,
        "</div>",
        "QR",
    );
    masked(&html, r#"<code data-slot="code">"#, "</code>", "CODE")
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

/// The pages drawn, by name, in the order they were drawn.
#[derive(Default)]
struct Drawn(Vec<(&'static str, String)>);

impl Drawn {
    fn put(&mut self, name: &'static str, response: TestResponse, status: u16) {
        let html = response.assert_status(status).text();
        self.0.push((name, held(&html)));
    }
}

/// Every page an app with the default pages draws: sign-in with each notice and an error,
/// the second factor, the forgotten and reset password pages.
async fn plain_pages(drawn: &mut Drawn) {
    let clock = support::clock();
    let app = with(support::clocked(&clock), AuthPages::new())
        .build()
        .await;
    let user = create_user(&app, "anu@example.com").await;
    let guest = app.client();
    drawn.put("sign-in", open(&guest, "/sign-in").await, 200);
    drawn.put(
        "sign-in, signed out",
        open(&guest, "/sign-in?status=signed-out").await,
        200,
    );
    drawn.put(
        "sign-in, password changed",
        open(&guest, "/sign-in?status=password-reset").await,
        200,
    );
    drawn.put(
        "sign-in, expired",
        open(&guest, "/sign-in?status=expired&next=%2Fdeals").await,
        200,
    );
    drawn.put(
        "sign-in, wrong password",
        post_form(
            &guest,
            "/sign-in",
            &[("email", "anu@example.com"), ("password", "not it")],
        )
        .await,
        422,
    );
    drawn.put(
        "forgot password",
        open(&guest, "/forgot-password").await,
        200,
    );
    drawn.put(
        "forgot password, expired",
        open(&guest, "/forgot-password?status=expired").await,
        200,
    );
    drawn.put(
        "forgot password, sent",
        post_form(&guest, "/forgot-password", &[("email", "anu@example.com")]).await,
        200,
    );
    drawn.put(
        "reset password",
        open(&guest, "/reset-password?token=abc&email=anu%40example.com").await,
        200,
    );
    drawn.put(
        "reset password, refused",
        post_form(
            &guest,
            "/reset-password",
            &[
                ("email", "anu@example.com"),
                ("token", "abc"),
                ("password", "a new password 42"),
                ("password_confirmation", "a new password 42"),
            ],
        )
        .await,
        400,
    );

    enable_two_factor(&app, &clock, &user).await;
    let browser = app.client();
    post_form(
        &browser,
        "/sign-in",
        &[
            ("email", "anu@example.com"),
            ("password", PASSWORD),
            ("next", "/deals"),
        ],
    )
    .await
    .assert_status(303);
    drawn.put(
        "two-factor",
        open(&browser, "/sign-in/two-factor?next=%2Fdeals").await,
        200,
    );
    drawn.put(
        "two-factor, wrong code",
        post_form(&browser, "/sign-in/two-factor", &[("code", "000000")]).await,
        422,
    );
}

/// The setup of two-factor authentication, for a user a policy requires it of: the page
/// that offers it, the secret, a wrong code, the recovery codes and the page once it is on.
async fn setup_pages(drawn: &mut Drawn) {
    let clock = support::clock();
    let app = with(support::clocked(&clock), AuthPages::new())
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
    drawn.put(
        "two-factor setup, off",
        open(&browser, "/account/two-factor?next=%2Fdeals").await,
        200,
    );
    drawn.put(
        "two-factor setup, the secret",
        post_form(&browser, "/account/two-factor", &[("next", "/deals")]).await,
        200,
    );
    let wrong = post_form(
        &browser,
        "/account/two-factor/confirm",
        &[("code", "000000")],
    )
    .await
    .assert_status(422)
    .text();
    let secret = first_code(&wrong);
    drawn.0.push(("two-factor setup, wrong code", held(&wrong)));
    drawn.put(
        "two-factor setup, recovery codes",
        post_form(
            &browser,
            "/account/two-factor/confirm",
            &[("code", totp_code(&secret, &clock).as_str())],
        )
        .await,
        200,
    );
    drawn.put(
        "two-factor setup, on",
        open(&browser, "/account/two-factor").await,
        200,
    );
}

/// The text of the first `<code data-slot="code">`: the secret of a setup page.
fn first_code(html: &str) -> String {
    let marker = "<code data-slot=\"code\">";
    let start = html.find(marker).expect("a code") + marker.len();
    html[start..]
        .split("</code>")
        .next()
        .expect("a closed code")
        .to_owned()
}

/// Every page an app with every option of 1.7.0 draws: the app's name, sign-up, the panel
/// beside the form, the theme menu, and the ways in without a password.
async fn full_pages(drawn: &mut Drawn) {
    let clock = support::clock();
    let app = with(
        support::mail_builder()
            .override_value::<dyn navree_contracts::Clock>(clock.clone() as Arc<FixedClock>)
            .config("auth.magic_link.enabled", true)
            .config(
                "auth.magic_link.url",
                "https://app.example.com/sign-in/link/verify",
            )
            .config("auth.otp.enabled", true)
            .config("auth.passkeys.enabled", true)
            .config("auth.passkeys.rp_id", "example.com")
            .config("auth.passkeys.origins", json!(["https://app.example.com"])),
        AuthPages::new()
            .app_name("Agra CRM")
            .sign_up()
            .aside(el("p").child("Deals, contacts and the week ahead."))
            .theme_menu(),
    )
    .build()
    .await;
    create_user(&app, "anu@example.com").await;
    let guest = app.client();
    drawn.put(
        "all on: sign-in",
        open(&guest, "/sign-in?next=%2Fdeals").await,
        200,
    );
    drawn.put(
        "all on: sign-up",
        open(&guest, "/sign-up?next=%2Fdeals").await,
        200,
    );
    drawn.put(
        "all on: sign-up, passwords differ",
        post_form(
            &guest,
            "/sign-up",
            &[
                ("name", "Simran Kaur"),
                ("email", "simran@example.com"),
                ("password", "a long password 42"),
                ("password_confirmation", "another one 42"),
            ],
        )
        .await,
        422,
    );
    drawn.put(
        "all on: magic link",
        open(&guest, "/sign-in/link").await,
        200,
    );
    drawn.put(
        "all on: magic link, sent",
        post_form(&guest, "/sign-in/link", &[("email", "anu@example.com")]).await,
        200,
    );
    drawn.put(
        "all on: magic link, verify",
        open(&guest, "/sign-in/link/verify?token=abc&next=%2Fdeals").await,
        200,
    );
    drawn.put(
        "all on: magic link, refused",
        post_form(&guest, "/sign-in/link/verify", &[("token", "abc")]).await,
        400,
    );
    drawn.put("all on: code", open(&guest, "/sign-in/code").await, 200);
    drawn.put(
        "all on: code, sent",
        post_form(&guest, "/sign-in/code", &[("email", "anu@example.com")]).await,
        200,
    );
    drawn.put(
        "all on: code, wrong",
        post_form(
            &guest,
            "/sign-in/code/verify",
            &[("email", "anu@example.com"), ("code", "000000")],
        )
        .await,
        422,
    );
    drawn.put(
        "all on: passkey",
        open(&guest, "/sign-in/passkey").await,
        200,
    );
    drawn.put(
        "all on: forgot password",
        open(&guest, "/forgot-password").await,
        200,
    );
    drawn.put(
        "all on: reset password",
        open(&guest, "/reset-password?token=abc&email=anu%40example.com").await,
        200,
    );
    assert_eq!(sent_mail(&app).await.len(), 2, "a link and a code");
}

/// Each page's name and the SHA-256 of its markup as [`held`], as the pages were drawn
/// before `AuthPages` and `AuthLayout` took options of their own.
const PAGES: [(&str, &str); 30] = [
    (
        "sign-in",
        "3e7cc9e9eb25c5d84fdbb7a669682b48843edad269212b4aff8f4ac1d241bbee",
    ),
    (
        "sign-in, signed out",
        "76b09d7cdc3461a2caeb1e55124e60e71e2b61d866f32a79425bb2c0e4099fbd",
    ),
    (
        "sign-in, password changed",
        "b38a73a7c8f2cd0a3470302f4263d8f1260cead4e98f80ea3507fe423f6db9f7",
    ),
    (
        "sign-in, expired",
        "4ba15c11b0c93f2bcd96a0e9b50bc84be15383ca2a60c59d1a32bb562fd868ee",
    ),
    (
        "sign-in, wrong password",
        "f2d377c878bec7f4eed846d55ae49b775804e5f56b5e183ae117958c7fb4353e",
    ),
    (
        "forgot password",
        "6f1ee5698010d0339278bd0e90f9d982d49c1bdc088b0df3e14992b03cf8fa9f",
    ),
    (
        "forgot password, expired",
        "b3253ae4ed8aa7c13c718eff14af90a373d8141915fea659b9108f1d3fd83baf",
    ),
    (
        "forgot password, sent",
        "a209ac52f7fa4707dd09416a06eac7d094cafb5eadff1f71db325defb50bdfc5",
    ),
    (
        "reset password",
        "e3f720fa6231f1a8cd552a99143ad59603f54975879e0d202bf281d663dd9185",
    ),
    (
        "reset password, refused",
        "f67236361bacffd99595c7acc6993008df913ac071b764c28a708077cc5eaf9a",
    ),
    (
        "two-factor",
        "0a0b72a90c0ab4bf02ad7c46e4a74fbe73828fb8383a9052ba503a77bc684b15",
    ),
    (
        "two-factor, wrong code",
        "9ce1b11fb390a16517150eee6d99e61109824d6a15a622253d67ae6ba78dafd5",
    ),
    (
        "two-factor setup, off",
        "e930ea4c8416034a728bcc40c2515999aad8e458ddd3fb0ad27c60acd6fd2d62",
    ),
    (
        "two-factor setup, the secret",
        "744a15e29dff971f227d42bc150b622cc4a1bca720b91aeb1e2ac1c7574a7670",
    ),
    (
        "two-factor setup, wrong code",
        "51a5ca1b6941a031a0fdffba31c58f7f97e4e9710cd13c1621f64a20341b7b7c",
    ),
    (
        "two-factor setup, recovery codes",
        "066b498e5c61df2d266bfb4393050b1ca24373eb78b9dd07ef07973214cfdf5e",
    ),
    (
        "two-factor setup, on",
        "1db30b77011c6f378309716024f7d98b86500ab0557033a81ccb22bf39c6dd2a",
    ),
    (
        "all on: sign-in",
        "a4b9b81fa91719c8627772f9c4204d815e442dff0ddcd2334bd7ed88f7e32898",
    ),
    (
        "all on: sign-up",
        "67a7c67fd057a299b80544257a1c9c394137f165a48dacf4d94931255aedd359",
    ),
    (
        "all on: sign-up, passwords differ",
        "79726d754c66658a01e18c853a86bde87003bddfd0492c9c6f8651d4651678a4",
    ),
    (
        "all on: magic link",
        "45c8dc89ba9a2aab0c919b5937266fd72be171071554cd5e1b85bc24c7f3427f",
    ),
    (
        "all on: magic link, sent",
        "92d1d53fa8f33a3632fd31ff13bbb388bbffa7fd1751e4ac4c0247b31b22bf6e",
    ),
    (
        "all on: magic link, verify",
        "17e569c9c2ef76d46cb01c9fca820610850fecd6b5798966bc981fb5e3836256",
    ),
    (
        "all on: magic link, refused",
        "8f73ac6a4cd913f24a9bc6d8bfb72d6a0fe1dadcedb96b0f78829026984509c6",
    ),
    (
        "all on: code",
        "38aa54c4ac81dc5625f270e08ea94ef38421fefb6787ddf1bbb0b533e4eb9e37",
    ),
    (
        "all on: code, sent",
        "8273f750a483914317aa3e7bdc1c1914d6a584483d8aefb0a93b4419a8043d6d",
    ),
    (
        "all on: code, wrong",
        "7c2be840a20fe677e6821a0649542e6bc13d9cd411b67af676b84056435a1541",
    ),
    (
        "all on: passkey",
        "bcc10b1f9cb683ffd75c38e1c366246b11bcc73d5e489ea9f112557e45ebcca1",
    ),
    (
        "all on: forgot password",
        "4f7a0ea081b3614994a8c79e92091e3b27711e103ab48d13fc9094e4770525e8",
    ),
    (
        "all on: reset password",
        "0ae7fed1f5c17fbc8c6bfacde32ec2e2fa79ef36d93b744798a50c6f052add82",
    ),
];

#[tokio::test]
async fn the_pages_an_app_sets_nothing_new_on_are_drawn_as_before() {
    let mut drawn = Drawn::default();
    plain_pages(&mut drawn).await;
    setup_pages(&mut drawn).await;
    full_pages(&mut drawn).await;
    let names: Vec<&str> = drawn.0.iter().map(|(name, _)| *name).collect();
    let pinned: Vec<&str> = PAGES.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names, pinned,
        "every page is pinned, in the order it is drawn"
    );
    for ((name, html), (_, sum)) in drawn.0.iter().zip(PAGES) {
        assert_eq!(sha(html), sum, "{name}:\n{html}");
    }
}
