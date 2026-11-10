//! Shared fixtures: deal events, a fixed clock, a fake HTTP client and the
//! app builder.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_contracts::{Clock, FixedClock};
use navree_db::{DatabaseModule, Db};
use navree_events::{Event, EventBus, EventsModule};
use navree_http_client::{FakeServer, HttpClient, HttpClientModule};
use navree_kernel::{BoxError, BoxFuture, Module, ModuleBuilder};
use navree_orm::Model as _;
use navree_queue::{Queue, QueueModule};
use navree_testing::{TestApp, TestAppBuilder};
use navree_webhooks::{
    CreatedEndpoint, Delivery, ModuleBuilderWebhooksExt, NewEndpoint, WebhookContext, WebhookEvent,
    Webhooks, WebhooksModule,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// A deal moved to won.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DealWon {
    pub deal_id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub title: String,
}

impl Event for DealWon {
    const NAME: &'static str = "deal.won";
}

impl WebhookEvent for DealWon {
    const DESCRIPTION: &'static str = "A deal moved to won.";

    fn workspace_id(&self) -> Option<Uuid> {
        self.workspace_id
    }

    fn payload<'a>(&'a self, ctx: &'a WebhookContext) -> BoxFuture<'a, Result<Value, BoxError>> {
        Box::pin(async move {
            // Payloads can read the database.
            let endpoints = navree_webhooks::entity::endpoint::Model::query()
                .count(ctx.db())
                .await?;
            Ok(json!({
                "deal": { "id": self.deal_id, "title": self.title },
                "endpoints_seen": endpoints,
            }))
        })
    }
}

/// A deal was lost (the default payload: the event itself).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DealLost {
    pub deal_id: Uuid,
    pub reason: String,
}

impl Event for DealLost {
    const NAME: &'static str = "deal.lost";
}

impl WebhookEvent for DealLost {
    const DESCRIPTION: &'static str = "A deal was lost.";
}

/// The deals feature: two webhook events and one type dispatched by name.
pub struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.imports((
            WebhooksModule::for_feature::<DealWon>(),
            WebhooksModule::for_feature::<DealLost>(),
        ))
        .webhook_type("export.ready", "A CSV export finished.");
    }
}

/// 2026-10-26T08:00:00Z.
pub fn start() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-26T08:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

/// A test app with the outgoing stack and its fakes.
pub struct Harness {
    pub app: TestApp,
    pub server: FakeServer,
    pub clock: Arc<FixedClock>,
}

/// The outgoing stack: database, events, memory queue, fake HTTP client,
/// fixed clock, webhooks (routes off) and the deals module; `configure`
/// adds configuration and modules.
pub fn builder(client: HttpClient, clock: Arc<FixedClock>) -> TestAppBuilder {
    crate::servers::database(TestApp::builder())
        .config("queue.driver", "memory")
        .config("webhooks.routes", false)
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(QueueModule::from_config())
        .module(HttpClientModule::from_config())
        .module(WebhooksModule::from_config())
        .module(DealsModule)
        .override_value(Arc::new(client))
        .override_value::<dyn Clock>(clock)
}

pub async fn harness(configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder) -> Harness {
    let (client, server) = HttpClient::fake();
    let clock = Arc::new(FixedClock::new(start()));
    let app = configure(builder(client, clock.clone())).build().await;
    Harness { app, server, clock }
}

impl Harness {
    pub async fn webhooks(&self) -> Arc<Webhooks> {
        self.app.get::<Webhooks>().await
    }

    /// Creates an endpoint in its own committed unit of work.
    pub async fn endpoint(&self, new: NewEndpoint) -> CreatedEndpoint {
        let uow = self.app.get::<Db>().await.uow(None, None).await;
        let created = self
            .webhooks()
            .await
            .create_endpoint(&uow, new)
            .await
            .unwrap();
        uow.commit().await.unwrap();
        created
    }

    pub async fn emit<E: Event>(&self, event: E) {
        self.app.get::<EventBus>().await.emit(event).await.unwrap();
    }

    pub async fn run_queue(&self) {
        self.app.get::<Queue>().await.run_pending().await.unwrap();
    }

    /// Moves the clock forward, then runs what became due.
    pub async fn advance(&self, by: Duration) {
        self.clock.advance(by);
        self.run_queue().await;
    }

    /// The newest delivery of `endpoint_id`.
    pub async fn last_delivery(&self, endpoint_id: Uuid) -> Delivery {
        self.webhooks()
            .await
            .deliveries(endpoint_id, navree_db::PageQuery::default())
            .await
            .unwrap()
            .data
            .into_iter()
            .next()
            .expect("a delivery")
    }
}

pub fn deal_won(workspace_id: Option<Uuid>) -> DealWon {
    DealWon {
        deal_id: Uuid::now_v7(),
        workspace_id,
        title: "Lakhotia renewal".to_owned(),
    }
}
