//! Browser sign-in for Navree UI pages, written as an app writes
//! it (the facade and its prelude): a page that needs a signed-in user
//! answers 401 with a page that links to sign in with `next`; the sign-in
//! form posts with its `_csrf` field; the page then shows the user; signing
//! out makes it 401 again. With workspaces on, the same browser also passes
//! a second factor and switches workspace.
//!
//! `cargo test -p navree --features sqlite,ui,auth,session,testing --test ui_auth_session`

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree::auth::{AuthModule, AuthPages, AuthService, ClientMeta, CurrentUser};
use navree::contracts::{Clock, FixedClock};
use navree::crypto::{CryptoModule, generate_key};
use navree::db::DatabaseModule;
use navree::http::{Controller, RouteDef};
use navree::prelude::*;
use navree::session::{SessionModule, SessionOptions};
use navree::testing::{TestApp, TestAppBuilder, TestClient, TestResponse};
use navree::ui::{UiModule, View};
use serde_json::Value;

const PASSWORD: &str = "correct horse battery";

/// `GET /dashboard`: a page for signed-in users, naming them.
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
                "!/dashboard",
                |CurrentUser(user): CurrentUser| async move {
                    View::new("Dashboard").body(format!("Signed in as {}", user.name))
                },
            )
            .authenticated()
            .page(),
        ]
    }
}

struct PagesModule;

impl Module for PagesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
    }
}

fn builder(clock: &Arc<FixedClock>) -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .override_value::<dyn Clock>(clock.clone())
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(AuthModule::from_config())
        .module(UiModule)
        .module(AuthPages::new().app_name("Agra CRM"))
        .module(PagesModule)
}

fn clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::new(chrono::Utc::now()))
}

async fn get(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

/// Posts `form` with the session's CSRF token as a page without
/// JavaScript does.
async fn post(client: &TestClient, path: &str, form: &[(&str, &str)]) -> TestResponse {
    let token = client
        .cookie("XSRF-TOKEN")
        .expect("a page handed out the token");
    let mut form = form.to_vec();
    form.push(("_csrf", token.as_str()));
    client
        .post(path)
        .header("accept", "text/html")
        .form(&form)
        .send()
        .await
}

// The whole loop a Navree UI app runs: 401 page with a sign-in
// link, the sign-in page, the posted form, the page showing the user, and
// signing out.
#[tokio::test]
async fn a_navree_ui_page_signs_a_browser_in_and_out_on_the_session() {
    let clock = clock();
    let app = builder(&clock).build().await;
    app.get::<AuthService>()
        .await
        .create_user("Anu Lakhotia", "anu@example.com", PASSWORD)
        .await
        .unwrap();
    let browser = app.client();

    let refused = get(&browser, "/dashboard").await.assert_status(401).text();
    assert!(
        refused.contains("/sign-in?next=%2Fdashboard"),
        "the 401 page links to sign in and back: {refused}"
    );

    let form = get(&browser, "/sign-in?next=%2Fdashboard")
        .await
        .assert_status(200)
        .text();
    assert!(form.contains("Agra CRM"), "{form}");
    assert!(form.contains(r#"name="_csrf""#), "{form}");

    // Without its token the form is refused.
    browser
        .post("/sign-in")
        .header("accept", "text/html")
        .form(&[("email", "anu@example.com"), ("password", PASSWORD)])
        .send()
        .await
        .assert_status(403);

    let signed_in = post(
        &browser,
        "/sign-in",
        &[
            ("email", "anu@example.com"),
            ("password", PASSWORD),
            ("next", "/dashboard"),
        ],
    )
    .await
    .assert_status(303);
    assert_eq!(signed_in.header("location"), Some("/dashboard"));
    let cookie = signed_in
        .set_cookie("__Host-navree_session")
        .expect("the session cookie");
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.secure(), Some(true));

    let page = get(&browser, "/dashboard").await.assert_status(200).text();
    assert!(page.contains("Signed in as Anu Lakhotia"), "{page}");

    let out = post(&browser, "/sign-out", &[]).await.assert_status(303);
    assert_eq!(out.header("location"), Some("/sign-in?status=signed-out"));
    get(&browser, "/dashboard").await.assert_status(401);
    let notice = get(&browser, "/sign-in?status=signed-out")
        .await
        .assert_status(200)
        .text();
    assert!(notice.contains("You are signed out."), "{notice}");
}

/// The TOTP code of `secret` (base32) at `clock`'s time.
fn totp(secret: &str, clock: &FixedClock) -> String {
    use navree::crypto::totp::{Secret, Totp};
    Totp::builder(Secret::from_base32(secret).expect("a base32 secret"))
        .build()
        .expect("a valid TOTP")
        .generate(u64::try_from(clock.now().timestamp()).unwrap())
}

// One browser signs in with a second factor, switches
// between its two workspaces, can't switch into a third, and signs out.
#[cfg(feature = "workspace")]
#[tokio::test]
async fn a_browser_signs_in_with_two_factor_switches_workspace_and_signs_out() {
    use navree::access::{AccessModule, AccessOptions};
    use navree::db::Db;
    use navree::workspace::{
        CurrentWorkspace, NewWorkspace, WorkspaceModule, WorkspaceSwitcher, Workspaces,
    };

    /// `GET /home`: the current workspace and the switcher.
    struct Home;

    impl Injectable for Home {
        fn construct(
            _r: Resolver,
        ) -> navree::BoxFuture<'static, Result<Self, navree::kernel::KernelError>> {
            Box::pin(async { Ok(Home) })
        }
    }

    impl Controller for Home {
        fn prefix(&self) -> &'static str {
            ""
        }

        fn routes(self: Arc<Self>) -> Vec<RouteDef> {
            vec![
                RouteDef::on(
                    Method::GET,
                    "!/home",
                    |ws: CurrentWorkspace, switcher: WorkspaceSwitcher| async move {
                        View::new("Home").body((format!("Working in {}", ws.name), switcher))
                    },
                )
                .authenticated()
                .page(),
            ]
        }
    }

    struct HomeModule;

    impl Module for HomeModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.controller::<Home>();
        }
    }

    let clock = clock();
    let app = builder(&clock)
        .config("workspace.resolvers", serde_json::json!(["session"]))
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(HomeModule)
        .build()
        .await;
    let auth = app.get::<AuthService>().await;
    let ada = auth
        .create_user("Anu Lakhotia", "anu@example.com", PASSWORD)
        .await
        .unwrap();
    let grace = auth
        .create_user("Gauri Hundal", "gauri@example.com", PASSWORD)
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
    workspaces
        .create(&uow, ada.id, NewWorkspace::named("Agra"))
        .await
        .unwrap();
    workspaces
        .create(&uow, ada.id, NewWorkspace::named("Gulati"))
        .await
        .unwrap();
    let ilaichi = workspaces
        .create(&uow, grace.id, NewWorkspace::named("Ilaichi"))
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let browser = app.client();
    get(&browser, "/sign-in").await.assert_status(200);
    let first = post(
        &browser,
        "/sign-in",
        &[
            ("email", "anu@example.com"),
            ("password", PASSWORD),
            ("next", "/home"),
        ],
    )
    .await
    .assert_status(303);
    assert_eq!(
        first.header("location"),
        Some("/sign-in/two-factor?next=%2Fhome")
    );
    get(&browser, "/home").await.assert_status(401);
    get(&browser, "/sign-in/two-factor?next=%2Fhome")
        .await
        .assert_status(200);
    let code = totp(&setup.secret, &clock);
    let second = post(
        &browser,
        "/sign-in/two-factor",
        &[("code", code.as_str()), ("next", "/home")],
    )
    .await
    .assert_status(303);
    assert_eq!(second.header("location"), Some("/home"));

    // No workspace chosen yet: the session names none.
    get(&browser, "/home").await.assert_status(400);
    post(
        &browser,
        "/workspace/switch",
        &[("workspace", "agra"), ("next", "/home")],
    )
    .await
    .assert_status(303);
    let home = get(&browser, "/home").await.assert_status(200).text();
    assert!(home.contains("Working in Agra"), "{home}");
    assert!(
        home.contains("Gulati") && !home.contains("Ilaichi"),
        "{home}"
    );
    post(&browser, "/workspace/switch", &[("workspace", "gulati")])
        .await
        .assert_status(303);
    let home = get(&browser, "/home").await.assert_status(200).text();
    assert!(home.contains("Working in Gulati"), "{home}");
    post(
        &browser,
        "/workspace/switch",
        &[("workspace", ilaichi.id.to_string().as_str())],
    )
    .await
    .assert_status(403);
    let home = get(&browser, "/home").await.assert_status(200).text();
    assert!(home.contains("Working in Gulati"), "{home}");

    post(&browser, "/sign-out", &[]).await.assert_status(303);
    get(&browser, "/home").await.assert_status(401);
}
