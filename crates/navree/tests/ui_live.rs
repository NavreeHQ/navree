//! A Navree UI page on the app's live hub, through the facade (MOD-004): the
//! page draws its live parts on the hub's stream, a job a queue worker runs
//! tells them, and a workspace topic reaches only the members reading in that
//! workspace, as the workspace module resolves it.
//!
//! `cargo test -p navree --features sqlite --test ui_live`
#![cfg(all(
    feature = "sqlite",
    feature = "ui",
    feature = "queue",
    feature = "workspace",
    feature = "testing"
))]

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use http_body_util::BodyExt;
use navree::access::AccessOptions;
use navree::contracts::{BoxError, Dispatcher, Job, JobContext};
use navree::db::Db;
use navree::http::body::Body;
use navree::http::{Controller, RouteDef};
use navree::prelude::*;
use navree::queue::{Queue, QueueDriver, QueueOptions};
use navree::realtime::{Live, STREAM_PATH, Topic};
use navree::testing::{TestApp, TestAppBuilder, principal};
use navree::ui::{LIVE_STREAM_PATH, LiveStream, Region, UiModule, View};
use navree::workspace::{NewWorkspace, Workspaces};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// The page's stream: one topic.
fn stream() -> LiveStream {
    LiveStream::topics(["deals"])
}

/// `GET /deals`: the deals, in a region the hub keeps current.
async fn deals_page() -> View {
    View::new("Deals").body(
        Region::new("deals")
            .src("/fragments/deals")
            .live_on(stream().on("deals"))
            .children("12 open deals"),
    )
}

/// Recounts the deals in the background, then tells the pages.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RecountDeals;

impl Job for RecountDeals {
    const NAME: &'static str = "deals.recount";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        // ... the recount, then:
        let live = ctx.app.get::<Live>().await?;
        live.nudge([Topic::public("deals")]).await
    }
}

struct Pages;

impl Injectable for Pages {
    fn dependencies() -> Vec<navree::kernel::ProviderKey> {
        Vec::new()
    }

    fn construct(
        _: Resolver,
    ) -> navree::contracts::BoxFuture<'static, Result<Self, navree::kernel::KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![RouteDef::on(Method::GET, "!/deals", deals_page).public()]
    }
}

struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>().job::<RecountDeals>();
    }
}

fn builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.users_table", "")
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Database,
            ..QueueOptions::default()
        }))
        .module(RealtimeModule::from_config())
        .module(UiModule)
        .module(DealsModule)
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

    /// The `event:` of the next event, or `None` when nothing came within `wait`.
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

const WAIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(300);

#[test]
fn the_page_and_the_hub_agree_on_where_the_stream_is() {
    // MOD-004
    assert_eq!(LIVE_STREAM_PATH, STREAM_PATH);
}

#[tokio::test]
async fn a_page_on_the_hub_hears_a_job_a_queue_worker_ran() {
    // MOD-004: the worker-to-page path.
    let app = builder().build().await;
    let html = app
        .client()
        .get("/deals")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(
        html.contains(r#"data-ui-live-src="/broadcasting/stream?topics=deals""#),
        "{html}"
    );
    assert!(html.contains(r#"data-ui-live-event="deals""#), "{html}");

    let mut page = Stream::new(
        app.client()
            .get("/broadcasting/stream?topics=deals")
            .send_stream()
            .await,
    );
    assert_eq!(page.next(WAIT).await.as_deref(), Some("ui:live"));

    let dispatcher = app.get::<dyn Dispatcher>().await;
    dispatcher.dispatch(RecountDeals).await.unwrap();
    assert_eq!(page.next(QUIET).await, None, "nothing before the job runs");
    let stats = app.get::<Queue>().await.run_pending().await.unwrap();
    assert_eq!(stats.failed, 0);
    assert_eq!(page.next(WAIT).await.as_deref(), Some("deals"));
}

#[tokio::test]
async fn a_workspace_topic_reaches_only_its_members_as_the_workspace_module_resolves_them() {
    // MOD-004: the stream finds the reader's workspace with the workspace
    // module's resolvers and membership rules (here the `X-Workspace` header).
    let app = builder().build().await;
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let db = app.get::<Db>().await;
    let workspaces = app.get::<Workspaces>().await;
    let uow = db.uow(None, None).await;
    let acme = workspaces
        .create(&uow, ada, NewWorkspace::named("Acme"))
        .await
        .unwrap();
    let globex = workspaces
        .create(&uow, bob, NewWorkspace::named("Globex"))
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let open = |user: Uuid, workspace: &str| {
        let request = app
            .acting_as(principal(user))
            .get("/broadcasting/stream?topics=deals")
            .header("x-workspace", workspace.to_owned());
        async move { Stream::new(request.send_stream().await) }
    };
    let mut ada_in_acme = open(ada, &acme.slug).await;
    let mut bob_in_globex = open(bob, &globex.slug).await;
    // Bob names Acme, where he is no member: his stream is in no workspace.
    let mut bob_in_acme = open(bob, &acme.slug).await;
    for page in [&mut ada_in_acme, &mut bob_in_globex, &mut bob_in_acme] {
        assert_eq!(page.next(WAIT).await.as_deref(), Some("ui:live"));
    }

    let live = app.get::<Live>().await;
    live.nudge([Topic::workspace(acme.id, "deals")])
        .await
        .unwrap();
    assert_eq!(ada_in_acme.next(WAIT).await.as_deref(), Some("deals"));
    assert_eq!(bob_in_globex.next(QUIET).await, None);
    assert_eq!(bob_in_acme.next(QUIET).await, None);

    live.nudge([Topic::workspace(globex.id, "deals")])
        .await
        .unwrap();
    assert_eq!(bob_in_globex.next(WAIT).await.as_deref(), Some("deals"));
    assert_eq!(ada_in_acme.next(QUIET).await, None);
}
