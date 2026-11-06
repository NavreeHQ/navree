//! A Navree UI app with the platform modules working together:
//! sign-in on the session, two workspaces with a different role in each,
//! the workspace a page opens in, what the role may do, the live hub telling
//! only the workspace that changed, and the audit trail of it all.
//!
//! `cargo test -p navree --features sqlite --test ui_platform`

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use http::Method;
use http_body_util::BodyExt;
use navree::access::{AccessModule, AccessOptions};
use navree::auth::{AuthModule, AuthPages, AuthService};
use navree::contracts::{AuditEntry, AuditSink, BoxError, BoxFuture};
use navree::crypto::{CryptoModule, generate_key};
use navree::db::{DatabaseModule, Db};
use navree::http::body::Body;
use navree::http::{Controller, RouteDef};
use navree::prelude::*;
use navree::realtime::{Live, STREAM_PATH, Topic};
use navree::session::{SessionModule, SessionOptions};
use navree::testing::{TestApp, TestClient, TestResponse};
use navree::ui::{UiModule, View};
use navree::workspace::{
    CurrentWorkspace, Members, NewWorkspace, WorkspaceRole, WorkspaceSwitcher, Workspaces,
};
use serde_json::{Value, json};

const PASSWORD: &str = "correct horse battery";
const WAIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(300);

/// Records the audit trail in memory.
#[derive(Default)]
struct Trail(Mutex<Vec<AuditEntry>>);

impl Trail {
    fn actions(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|entry| entry.action.clone())
            .collect()
    }
}

impl AuditSink for Trail {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry);
        Box::pin(async { Ok(()) })
    }
}

/// `GET /deals`: the workspace the page is in, the role in it, and whether
/// that role may create deals.
struct Deals;

impl Injectable for Deals {
    fn construct(
        _r: Resolver,
    ) -> navree::BoxFuture<'static, Result<Self, navree::kernel::KernelError>> {
        Box::pin(async { Ok(Deals) })
    }
}

impl Controller for Deals {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/deals",
                |ws: CurrentWorkspace, switcher: WorkspaceSwitcher| async move {
                    View::new("Deals").body((
                        format!(
                            "workspace={} role={} create={}",
                            ws.slug,
                            ws.role().as_str(),
                            ws.has_permission("deals.create")
                        ),
                        switcher,
                    ))
                },
            )
            .authenticated()
            .page(),
        ]
    }
}

struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Deals>();
    }
}

async fn app(trail: &Arc<Trail>) -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.resolvers", json!(["session"]))
        .config("workspace.session_default", true)
        .config(
            "workspace.roles.member.permissions",
            json!(["deals.read", "deals.create"]),
        )
        .config("workspace.roles.viewer.permissions", json!(["deals.read"]))
        .override_value::<dyn AuditSink>(trail.clone())
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(RealtimeModule::from_config())
        .module(UiModule)
        .module(AuthPages::new().app_name("Sutlej Supply"))
        .module(DealsModule)
        .build()
        .await
}

async fn get(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

/// Posts `form` with the session's CSRF token, as a page without JavaScript does.
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

/// A stream read off a response, event by event.
struct Stream {
    body: Body,
    buffer: String,
}

impl Stream {
    fn new(response: http::Response<Body>) -> Self {
        assert_eq!(response.status(), 200);
        Stream {
            body: response.into_body(),
            buffer: String::new(),
        }
    }

    /// The `event:` of the next event, or `None` when nothing came within
    /// `wait` or the stream ended.
    async fn next(&mut self, wait: Duration) -> Option<String> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if let Some(end) = self.buffer.find("\n\n") {
                let block: String = self.buffer.drain(..end + 2).collect();
                if let Some(name) = block.lines().find_map(|line| line.strip_prefix("event: ")) {
                    return Some(name.to_owned());
                }
                continue;
            }
            let frame = tokio::time::timeout_at(deadline, self.body.frame())
                .await
                .ok()??;
            if let Ok(data) = frame.ok()?.into_data() {
                self.buffer.push_str(&String::from_utf8_lossy(&data));
            }
        }
    }
}

// Ada owns Acme and is a viewer in Globex. She signs in and her first page is
// in Acme (the oldest of her workspaces), where she may create deals; after
// she switches, her page is in Globex, where she may not; the live hub tells
// her window only about the workspace she is in; and the audit trail has the
// sign-in and the switch.
#[tokio::test]
async fn sign_in_workspaces_roles_the_live_hub_and_the_audit_trail_work_together() {
    let trail = Arc::new(Trail::default());
    let app = app(&trail).await;
    let auth = app.get::<AuthService>().await;
    let ada = auth
        .create_user("Gurpreet Kaur", "gurpreet.kaur@example.com", PASSWORD)
        .await
        .unwrap();
    let grace = auth
        .create_user("Harjot Singh", "harjot.singh@example.com", PASSWORD)
        .await
        .unwrap();
    let db = app.get::<Db>().await;
    let workspaces = app.get::<Workspaces>().await;
    let members = app.get::<Members>().await;
    let uow = db.uow(None, None).await;
    let acme = workspaces
        .create(
            &uow,
            ada.id,
            NewWorkspace::named("Ludhiana desk").slug("ludhiana"),
        )
        .await
        .unwrap();
    let globex = workspaces
        .create(
            &uow,
            grace.id,
            NewWorkspace::named("Jalandhar desk").slug("jalandhar"),
        )
        .await
        .unwrap();
    members
        .add(&uow, globex.id, ada.id, WorkspaceRole::Viewer)
        .await
        .unwrap();
    // A workspace of somebody else's, that Ada does not belong to.
    workspaces
        .create(
            &uow,
            grace.id,
            NewWorkspace::named("Patiala desk").slug("patiala"),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let live = app.get::<Live>().await;

    // Not signed in: the page asks for a sign-in.
    let browser = app.client();
    get(&browser, "/deals").await.assert_status(401);
    get(&browser, "/sign-in").await.assert_status(200);
    post(
        &browser,
        "/sign-in",
        &[
            ("email", "gurpreet.kaur@example.com"),
            ("password", PASSWORD),
        ],
    )
    .await
    .assert_status(303);

    // Nothing chosen yet: the page opens in the first workspace, where she owns.
    let page = get(&browser, "/deals").await.assert_status(200).text();
    assert!(
        page.contains("workspace=ludhiana role=owner create=true"),
        "{page}"
    );
    assert!(
        page.contains("Jalandhar desk"),
        "the switcher lists both: {page}"
    );

    // Her window hears the workspace she is in.
    let mut stream = Stream::new(
        browser
            .get(&format!("{STREAM_PATH}?topics=deals"))
            .send_stream()
            .await,
    );
    assert_eq!(stream.next(WAIT).await.as_deref(), Some("ui:live"));
    live.nudge([Topic::workspace(globex.id, "deals")])
        .await
        .unwrap();
    assert_eq!(stream.next(QUIET).await, None, "not the other workspace");
    live.nudge([Topic::workspace(acme.id, "deals")])
        .await
        .unwrap();
    assert_eq!(stream.next(WAIT).await.as_deref(), Some("deals"));

    // The other workspace: a viewer may read there and not create.
    post(&browser, "/workspace/switch", &[("workspace", "jalandhar")])
        .await
        .assert_status(303);
    let page = get(&browser, "/deals").await.assert_status(200).text();
    assert!(
        page.contains("workspace=jalandhar role=viewer create=false"),
        "{page}"
    );

    // A workspace she does not belong to cannot be entered.
    post(&browser, "/workspace/switch", &[("workspace", "patiala")])
        .await
        .assert_status(403);

    // Signing out ends it.
    post(&browser, "/sign-out", &[]).await.assert_status(303);
    get(&browser, "/deals").await.assert_status(401);

    let actions = trail.actions();
    for expected in ["auth.login", "workspace.switched", "auth.logout"] {
        assert!(
            actions.iter().any(|action| action == expected),
            "{expected} in {actions:?}"
        );
    }
}
