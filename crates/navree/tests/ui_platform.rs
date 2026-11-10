//! A Navree UI app with the platform modules working together:
//! sign-in on the session, two workspaces with a different role in each,
//! the workspace a page opens in, what the role may do, the live hub telling
//! only the workspace that changed, and the audit trail of it all; an export
//! queued from a page that follows its operation to the file; and the
//! toolkit's words and money in Indian English and Hindi.
//!
//! `cargo test -p navree --features sqlite,i18n,queue --test ui_platform`

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use http::Method;
use http_body_util::BodyExt;
use navree::access::{AccessModule, AccessOptions};
use navree::auth::{AuthModule, AuthPages, AuthService};
use navree::contracts::{AuditEntry, AuditSink, BoxError, BoxFuture, Job, JobContext};
use navree::crypto::{CryptoModule, generate_key};
use navree::db::{DatabaseModule, Db, Uow};
use navree::http::body::Body;
use navree::http::response::{IntoResponse, Response};
use navree::http::{AppError, Controller, CurrentPrincipal, Path, RouteDef};
use navree::i18n::ui::{FromLocale, MoneyIn};
use navree::prelude::*;
use navree::queue::ui::OperationProgress;
use navree::queue::{Operation, Operations, Queue, QueueModule};
use navree::realtime::{Live, STREAM_PATH, Topic};
use navree::session::{SessionModule, SessionOptions};
use navree::testing::{TestApp, TestClient, TestResponse};
use navree::ui::{Money, TableLabels, UiModule, View};
use navree::workspace::{
    CurrentWorkspace, Members, NewWorkspace, WorkspaceRole, WorkspaceSwitcher, Workspaces,
};
use serde_json::{Value, json};
use uuid::Uuid;

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
        m.controller::<Exports>();
        m.job::<ExportDeals>();
    }
}

/// The deals of a workspace, as an export writes them.
fn deals_csv(workspace: &str) -> String {
    format!("workspace,deal\n{workspace},Basmati for the {workspace} mandi\n")
}

/// The export of a workspace's deals, the work of operation `operation`: it
/// writes the file and finishes the operation with the link to it.
#[derive(serde::Serialize, serde::Deserialize)]
struct ExportDeals {
    operation: Uuid,
    workspace: String,
}

impl Job for ExportDeals {
    const NAME: &'static str = "deals.export";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let operations = ctx.app.get::<Operations>().await?;
        let url = format!("/deals/exports/{}", self.operation);
        operations
            .complete(
                self.operation,
                json!({ "url": url, "csv": deals_csv(&self.workspace) }),
            )
            .await?;
        Ok(())
    }
}

/// `POST /deals/export` queues an export of the deals of the workspace the page
/// is in and answers a page that follows it; `GET /deals/exports/{id}` hands the
/// finished file to its owner; `GET /deals/words` says the toolkit's words and a
/// price in the reader's language.
struct Exports {
    operations: Arc<Operations>,
}

impl Injectable for Exports {
    fn dependencies() -> Vec<navree::kernel::ProviderKey> {
        vec![navree::kernel::ProviderKey::of::<Operations>()]
    }

    fn construct(
        r: Resolver,
    ) -> navree::BoxFuture<'static, Result<Self, navree::kernel::KernelError>> {
        Box::pin(async move {
            Ok(Exports {
                operations: r.get::<Operations>().await?,
            })
        })
    }
}

/// The page of a queued export: the region that follows its operation.
async fn export(
    operations: Arc<Operations>,
    uow: Uow,
    principal: CurrentPrincipal,
    ws: CurrentWorkspace,
) -> Result<View, AppError> {
    let operation = Operation::new("deals.export")
        .owner(principal.0.user_id)
        .workspace(ws.id());
    let record = operations
        .start(&uow, operation)
        .await
        .map_err(AppError::internal)?;
    let job = ExportDeals {
        operation: record.id,
        workspace: ws.slug.clone(),
    };
    operations
        .dispatch(&uow, &record, job)
        .await
        .map_err(AppError::internal)?;
    Ok(View::new("Export").body(OperationProgress::new(record.id)))
}

/// The file of a finished export, for its owner only.
async fn download(
    operations: Arc<Operations>,
    principal: CurrentPrincipal,
    id: Uuid,
) -> Result<Response, AppError> {
    let record = operations
        .find(id)
        .await
        .map_err(AppError::internal)?
        .filter(|record| record.owner_id == Some(principal.0.user_id))
        .ok_or_else(AppError::not_found)?;
    let csv = record
        .result
        .as_ref()
        .and_then(|result| result["csv"].as_str())
        .ok_or_else(AppError::not_found)?
        .to_owned();
    let headers = [(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/csv"),
    )];
    Ok((headers, csv).into_response())
}

impl Controller for Exports {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let operations = self.operations.clone();
        let start = move |uow: Uow, principal: CurrentPrincipal, ws: CurrentWorkspace| {
            export(operations.clone(), uow, principal, ws)
        };
        let operations = self.operations.clone();
        let file = move |principal: CurrentPrincipal, Path(id): Path<Uuid>| {
            download(operations.clone(), principal, id)
        };
        let words = |locale: Locale| async move {
            let labels = TableLabels::from_locale(&locale);
            let price = Money(10_000_000).display_in(&locale, "INR");
            View::new("Words").body(format!("search={} price={price}", labels.search))
        };
        vec![
            RouteDef::on(Method::POST, "!/deals/export", start)
                .authenticated()
                .page(),
            RouteDef::on(Method::GET, "!/deals/exports/{id}", file).authenticated(),
            RouteDef::on(Method::GET, "!/deals/words", words)
                .authenticated()
                .page(),
        ]
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
        .config("queue.driver", "memory")
        .config("i18n.locales", json!(["en-IN", "hi"]))
        .config("i18n.default_locale", "en-IN")
        .config("i18n.dir", "target/ui-platform-has-no-locale-files")
        .config("i18n.routes", false)
        .override_value::<dyn AuditSink>(trail.clone())
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(RealtimeModule::from_config())
        .module(QueueModule::from_config())
        .module(I18nModule::from_config())
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

// Anu owns Agra and is a viewer in Gulati. She signs in and her first page is
// in Agra (the oldest of her workspaces), where she may create deals; after
// she switches, her page is in Gulati, where she may not; the live hub tells
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
    let agra = workspaces
        .create(
            &uow,
            ada.id,
            NewWorkspace::named("Ludhiana desk").slug("ludhiana"),
        )
        .await
        .unwrap();
    let gulati = workspaces
        .create(
            &uow,
            grace.id,
            NewWorkspace::named("Jalandhar desk").slug("jalandhar"),
        )
        .await
        .unwrap();
    members
        .add(&uow, gulati.id, ada.id, WorkspaceRole::Viewer)
        .await
        .unwrap();
    // A workspace of somebody else's, that Anu does not belong to.
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
    live.nudge([Topic::workspace(gulati.id, "deals")])
        .await
        .unwrap();
    assert_eq!(stream.next(QUIET).await, None, "not the other workspace");
    live.nudge([Topic::workspace(agra.id, "deals")])
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

// Simran signs in, and her page in Amritsar queues an export: the page follows
// the operation, a worker runs it, the region ends as a link, and the link
// hands her the file of her workspace and nobody else. Her pages say the
// toolkit's words and prices in the language she reads: Hindi, or Indian
// English with its lakh grouping.
#[tokio::test]
async fn a_queued_export_follows_its_operation_to_the_file_in_the_readers_language() {
    let trail = Arc::new(Trail::default());
    let app = app(&trail).await;
    let auth = app.get::<AuthService>().await;
    let simran = auth
        .create_user("Simran Kaur", "simran.kaur@example.com", PASSWORD)
        .await
        .unwrap();
    auth.create_user("Karan Mehta", "karan.mehta@example.com", PASSWORD)
        .await
        .unwrap();
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    app.get::<Workspaces>()
        .await
        .create(
            &uow,
            simran.id,
            NewWorkspace::named("Amritsar desk").slug("amritsar"),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let browser = app.client();
    get(&browser, "/sign-in").await.assert_status(200);
    post(
        &browser,
        "/sign-in",
        &[("email", "simran.kaur@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);

    // The page queues the export and draws the region that follows it.
    let page = post(&browser, "/deals/export", &[])
        .await
        .assert_status(200)
        .text();
    let id = page
        .split("id=\"operation-")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| panic!("no operation region: {page}"))
        .to_owned();
    let progress = format!("/operations/{id}/progress");
    let waiting = get(&browser, &progress).await.assert_status(200).text();
    assert!(waiting.contains("data-state=\"pending\""), "{waiting}");
    assert!(waiting.contains("data-ui-poll"), "{waiting}");

    // A worker runs it: the region ends as the link to the file.
    app.get::<Queue>().await.run_pending().await.unwrap();
    let done = get(&browser, &progress).await.assert_status(200).text();
    assert!(done.contains("data-state=\"succeeded\""), "{done}");
    assert!(
        done.contains(&format!("href=\"/deals/exports/{id}\"")),
        "{done}"
    );
    assert!(!done.contains("data-ui-poll"), "finished: {done}");
    let file = get(&browser, &format!("/deals/exports/{id}"))
        .await
        .assert_status(200);
    assert!(
        file.header("content-type")
            .is_some_and(|kind| kind.starts_with("text/csv")),
        "{:?}",
        file.header("content-type")
    );
    assert_eq!(file.text(), deals_csv("amritsar"));

    // Somebody else gets neither the region nor the file.
    let other = app.client();
    get(&other, "/sign-in").await.assert_status(200);
    post(
        &other,
        "/sign-in",
        &[("email", "karan.mehta@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);
    get(&other, &progress).await.assert_status(404);
    get(&other, &format!("/deals/exports/{id}"))
        .await
        .assert_status(404);

    // The toolkit's words and money follow the reader's language.
    let words = |language: &'static str| {
        let browser = &browser;
        async move {
            browser
                .get("/deals/words")
                .header("accept", "text/html")
                .header("accept-language", language)
                .send()
                .await
                .assert_status(200)
                .text()
        }
    };
    let hindi = words("hi").await;
    assert!(hindi.contains("search=खोजें price=₹1,00,000.00"), "{hindi}");
    let india = words("en-IN").await;
    assert!(
        india.contains("search=Search price=₹1,00,000.00"),
        "{india}"
    );
}
