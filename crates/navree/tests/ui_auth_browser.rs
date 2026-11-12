//! The browser journey of a Navree UI app on the session cookie, in a real
//! Chrome: a guest meets the sign-in page, signs in with "remember me" and a
//! second factor, switches workspace from the switcher, sees the page follow
//! it, and signs out.
//!
//! `cargo test -p navree --features sqlite,ui,auth,session,workspace,testing,browser --test ui_auth_browser`
//!
//! It needs Chrome or Chromium (`NAVREE_BROWSER_CHROME`); without one it
//! prints a notice and passes, unless `NAVREE_TEST_REQUIRE_SERVICES=1`.

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree::access::{AccessModule, AccessOptions};
use navree::auth::{AuthModule, AuthPages, AuthService, ClientMeta};
use navree::browser::{Browser, BrowserError, TestServer};
use navree::contracts::{Clock, FixedClock};
use navree::crypto::{CryptoModule, generate_key};
use navree::db::{DatabaseModule, Db};
use navree::http::extract::Request;
use navree::http::middleware::{self, Next};
use navree::http::response::Response;
use navree::http::{Controller, GlobalLayer, LayerSlot, RouteDef};
use navree::prelude::*;
use navree::session::{Session, SessionModule, SessionOptions};
use navree::testing::TestApp;
use navree::ui::form::csrf_field;
use navree::ui::{UiModule, View, el};
use navree::workspace::{CurrentWorkspace, NewWorkspace, WorkspaceSwitcher, Workspaces};
use serde_json::{Value, json};

const PASSWORD: &str = "correct horse battery";

/// `GET /home` (the switcher and a sign-out button) and `GET /deals` (the
/// current workspace).
struct Pages;

impl Injectable for Pages {
    fn construct(
        _r: Resolver,
    ) -> navree::BoxFuture<'static, Result<Self, navree::kernel::KernelError>> {
        Box::pin(async { Ok(Pages) })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/home",
                |switcher: WorkspaceSwitcher, session: Session| async move {
                    let sign_out = el("form")
                        .attr("method", "post")
                        .attr("action", "/sign-out")
                        .children((
                            csrf_field(session.csrf_token()),
                            el("button").attr("type", "submit").child("Sign out"),
                        ));
                    View::new("Home").body((switcher, sign_out))
                },
            )
            .authenticated()
            .page(),
            RouteDef::on(Method::GET, "!/deals", |ws: CurrentWorkspace| async move {
                View::new("Deals").body(format!("Working in {}", ws.name))
            })
            .authenticated()
            .page(),
        ]
    }
}

struct PagesModule;

impl Module for PagesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
        m.http_layer(GlobalLayer::new(
            LayerSlot::Edge,
            "slow-switch",
            |_ctx, router| Ok(router.layer(middleware::from_fn(slow_switch))),
        ));
    }
}

/// How long a workspace switch takes here: as on a busy machine, longer than
/// opening a page in another tab.
const SWITCH_TAKES: Duration = Duration::from_secs(1);

/// Holds `POST /workspace/switch` for [`SWITCH_TAKES`]. Pressing a workspace
/// in the switcher on `/home` posts a form that comes back to `/home`, so the
/// path says nothing about the switch: a step that goes on before the switcher
/// names the new workspace races the post, and here it always loses.
async fn slow_switch(request: Request, next: Next) -> Response {
    if request.method() == Method::POST && request.uri().path() == "/workspace/switch" {
        tokio::time::sleep(SWITCH_TAKES).await;
    }
    next.run(request).await
}

/// The switcher's button, which names the workspace the page is in.
const SWITCHER: &str = "#ui-workspace-switcher-trigger";

/// The TOTP code of `secret` (base32) at `clock`'s time.
fn totp(secret: &str, clock: &FixedClock) -> String {
    use navree::crypto::totp::{Secret, Totp};
    Totp::builder(Secret::from_base32(secret).expect("a base32 secret"))
        .build()
        .expect("a valid TOTP")
        .generate(u64::try_from(clock.now().timestamp()).unwrap())
}

// Sign in, two-factor, switch workspace, sign out.
#[tokio::test]
async fn sign_in_two_factor_switch_workspace_and_sign_out() -> Result<(), BrowserError> {
    if let Err(error) = Browser::find_chrome() {
        if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") {
            panic!("ui_auth_browser: {error} (NAVREE_TEST_REQUIRE_SERVICES=1)");
        }
        eprintln!("skipped ui_auth_browser: {error}");
        return Ok(());
    }
    let clock = Arc::new(FixedClock::new(chrono::Utc::now()));
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.resolvers", json!(["session"]))
        .override_value::<dyn Clock>(clock.clone())
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        // The test server speaks plain HTTP on 127.0.0.1: no `Secure`, so
        // no `__Host-` prefix either (the HTTP tests check both).
        .module(SessionModule::for_root(SessionOptions {
            secure: false,
            ..SessionOptions::default()
        }))
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(UiModule)
        .module(AuthPages::new().app_name("Agra CRM"))
        .module(PagesModule)
        .build()
        .await;
    let auth = app.get::<AuthService>().await;
    let ada = auth
        .create_user("Anu Lakhotia", "anu@example.com", PASSWORD)
        .await
        .unwrap();
    let meta = ClientMeta::default();
    let setup = auth.enable_two_factor(ada.id, &meta).await.unwrap();
    auth.confirm_two_factor(ada.id, &totp(&setup.secret, &clock), &meta)
        .await
        .unwrap();
    clock.advance(Duration::from_secs(30));
    let db = app.get::<Db>().await;
    let workspaces = app.get::<Workspaces>().await;
    let uow = db.uow(None, None).await;
    for name in ["Agra", "Gulati"] {
        workspaces
            .create(&uow, ada.id, NewWorkspace::named(name))
            .await
            .unwrap();
    }
    uow.commit().await.unwrap();

    let server = TestServer::start(app.ctx()).await?;
    let browser = Browser::launch().await?;

    // A guest asking for a page is sent to sign in, and comes back after.
    let page = browser.visit(server.url("/home")).await?;
    page.assert_see("Sign in").await?;
    page.press("Sign in").await?;
    page.assert_path_is("/sign-in").await?;
    page.assert_see("Agra CRM").await?;
    page.type_into("Email", "anu@example.com").await?;
    page.type_into("Password", PASSWORD).await?;
    page.check("Keep me signed in on this device").await?;
    page.press("Sign in").await?;

    // The second factor.
    page.assert_path_is("/sign-in/two-factor").await?;
    page.type_into(
        "Code from your authenticator app",
        totp(&setup.secret, &clock),
    )
    .await?;
    page.press("Continue").await?;
    page.assert_path_is("/home").await?;
    page.assert_see("Choose a workspace").await?;
    let cookie = page
        .cookie("navree_session")
        .await?
        .expect("the session cookie");
    assert!(
        cookie.http_only,
        "JavaScript never reads the session cookie"
    );

    // Switch to Agra, then to Gulati; the page follows.
    page.click("Choose a workspace").await?;
    page.press("Agra").await?;
    page.assert_see_in(SWITCHER, "Agra").await?;
    page.assert_path_is("/home").await?;
    let deals = browser.visit(server.url("/deals")).await?;
    deals.assert_see("Working in Agra").await?;
    let home = browser.visit(server.url("/home")).await?;
    home.click("Agra").await?;
    home.press("Gulati").await?;
    home.assert_see_in(SWITCHER, "Gulati").await?;
    home.assert_path_is("/home").await?;
    let deals = browser.visit(server.url("/deals")).await?;
    deals.assert_see("Working in Gulati").await?;

    // Sign out: back to the sign-in page, and the pages are closed again.
    let home = browser.visit(server.url("/home")).await?;
    home.press("Sign out").await?;
    home.assert_path_is("/sign-in").await?;
    home.assert_see("You are signed out.").await?;
    let deals = browser.visit(server.url("/deals")).await?;
    deals.assert_see("Sign in").await?;
    deals.assert_dont_see("Working in").await?;
    Ok(())
}
