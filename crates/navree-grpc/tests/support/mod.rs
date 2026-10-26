//! Shared fixtures: the generated code of the test protos, a `deals` table
//! with access rules, the deals service of the docs, a ping service for the
//! pipeline tests, authenticators and app builders.

#![allow(dead_code)]

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::request::Parts;
use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, Accessible, Authorized, READ, Read, Rules, Subject,
    UPDATE, Update, field,
};
use navree_contracts::{
    AuditEntry, AuditSink, AuthState, AuthVia, BoxError, BoxFuture, Principal, RequestId,
};
use navree_db::{Db, MigrationSet, ModuleBuilderDbExt, Repo, Uow};
use navree_events::{Event, EventBus, EventsModule};
use navree_grpc::{
    GrpcChannels, GrpcModule, ModuleBuilderGrpcExt, Request, Response, Status, Streaming, Valid,
    grpc_service,
};
use navree_http::{AppError, Authenticator, CurrentPrincipal, Guard, Result};
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::injectable;
use navree_testing::{TestApp, TestAppBuilder};
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{ActiveModelTrait, EntityTrait, QueryOrder, Set};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};
use uuid::Uuid;

/// The generated code (see `tests/codegen.rs`).
#[allow(clippy::all, clippy::pedantic)]
pub mod pb {
    pub mod deals {
        include!("pb/crm.deals.v1.rs");
    }

    pub mod ping {
        include!("pb/navree.test.v1.rs");
    }

    /// Both files' descriptors.
    pub const FILE_DESCRIPTOR_SET: &[u8] = include_bytes!("pb/deals_descriptor.bin");
}

use pb::deals::deals_server::{Deals, DealsServer};
use pb::ping::ping_server::{Ping, PingServer};

// ---------------------------------------------------------------------------
// The deals table and its access rules
// ---------------------------------------------------------------------------

/// The `deals` entity.
pub mod deal {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "deals")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub title: String,
        pub stage: String,
        pub amount_cents: i64,
        pub owner_id: Uuid,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// The subject the docs call `Deal`.
pub type Deal = deal::Entity;

impl Subject for deal::Entity {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}

/// Creates `deals`.
pub struct CreateDeals;

impl MigrationName for CreateDeals {
    fn name(&self) -> &str {
        "m20261026_000001_create_deals"
    }
}

#[async_trait]
impl MigrationTrait for CreateDeals {
    async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), sea_orm::DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("deals")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("title").string().not_null())
                    .col(ColumnDef::new("stage").string().not_null())
                    .col(ColumnDef::new("amount_cents").big_integer().not_null())
                    .col(ColumnDef::new("owner_id").uuid().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), sea_orm::DbErr> {
        manager
            .drop_table(Table::drop().table("deals").to_owned())
            .await
    }
}

/// Signed-in users read and update the deals they own.
pub fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    let Some(user) = &ctx.user else {
        return;
    };
    rules.can(READ, "Deal").when(field("owner_id").eq(user.id));
    rules
        .can(UPDATE, "Deal")
        .when(field("owner_id").eq(user.id));
}

/// The access options of the test apps.
pub fn access_options() -> AccessOptions {
    AccessOptions {
        abilities,
        super_admin_role: None,
    }
}

/// Inserts a deal owned by `owner`.
pub async fn insert_deal(app: &TestApp, title: &str, owner: Uuid) -> deal::Model {
    let db = app.get::<Db>().await;
    deal::ActiveModel {
        id: Set(Uuid::now_v7()),
        title: Set(title.to_owned()),
        stage: Set("lead".to_owned()),
        amount_cents: Set(1_200_000),
        owner_id: Set(owner),
    }
    .insert(db.conn())
    .await
    .expect("insert deal")
}

/// The deal with `id`, read outside any request.
pub async fn find_deal(app: &TestApp, id: Uuid) -> deal::Model {
    let db = app.get::<Db>().await;
    deal::Entity::find_by_id(id)
        .one(db.conn())
        .await
        .expect("query")
        .expect("the deal exists")
}

impl From<deal::Model> for pb::deals::Deal {
    fn from(model: deal::Model) -> Self {
        pb::deals::Deal {
            id: model.id.to_string(),
            title: model.title,
            amount_cents: model.amount_cents,
            currency: "USD".to_owned(),
            stage: model.stage,
            company_id: String::new(),
            owner_id: model.owner_id.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// The deals service
// ---------------------------------------------------------------------------

/// A deal moved (event `deal.updated`), for `WatchDeals`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DealUpdated {
    pub deal_id: Uuid,
    pub stage: String,
}

impl Event for DealUpdated {
    const NAME: &'static str = "deal.updated";
}

pub type DealEvents =
    Pin<Box<dyn Stream<Item = std::result::Result<pb::deals::DealEvent, Status>> + Send>>;

/// The docs' deals service: access rules, validation, the unit of work and
/// the three kinds of streaming (`WatchDeals` as the docs write it, from
/// the event bus).
#[injectable]
pub struct DealsGrpc {
    events: Arc<EventBus>,
}

#[grpc_service(server = DealsServer)]
#[authenticated]
impl Deals for DealsGrpc {
    #[can(Read, Deal, from = "id")]
    async fn get_deal(
        &self,
        _request: Request<pb::deals::GetDealRequest>,
        uow: Uow,
        deal: Authorized<Deal, Read>,
    ) -> Result<Response<pb::deals::Deal>> {
        assert!(Uow::current().is_some_and(|current| current.same_as(&uow)));
        Ok(Response::new(deal.into_inner().into()))
    }

    #[can(Read, Deal)]
    async fn list_deals(
        &self,
        _request: Request<pb::deals::ListDealsRequest>,
        uow: Uow,
        scope: Accessible<Deal, Read>,
    ) -> Result<Response<pb::deals::ListDealsResponse>> {
        let select = scope.apply(deal::Entity::find().order_by_asc(deal::Column::Title));
        let rows = select.all(&*uow.tx().await?).await?;
        Ok(Response::new(pb::deals::ListDealsResponse {
            total: rows.len() as u64,
            deals: rows.into_iter().map(Into::into).collect(),
        }))
    }

    #[can(Update, Deal, from = "id")]
    async fn move_deal(
        &self,
        Valid(request): Valid<Request<pb::deals::MoveDealRequest>>,
        uow: Uow,
        CurrentPrincipal(me): CurrentPrincipal,
        deal: Authorized<Deal, Update>,
    ) -> Result<Response<pb::deals::Deal>> {
        let input = request.into_inner();
        let mut model: deal::ActiveModel = deal.into_inner().into();
        model.stage = Set(input.stage.clone());
        let updated = model.update(&*uow.tx().await?).await?;
        if input.lost_reason.as_deref() == Some("fail after writing") {
            return Err(AppError::conflict("the move was undone"));
        }
        assert_eq!(updated.owner_id, me.user_id);
        self.events
            .emit(DealUpdated {
                deal_id: updated.id,
                stage: updated.stage.clone(),
            })
            .await?;
        Ok(Response::new(updated.into()))
    }

    type WatchDealsStream = DealEvents;

    #[can(Read, Deal)]
    async fn watch_deals(
        &self,
        _request: Request<pb::deals::WatchDealsRequest>,
    ) -> Result<Response<Self::WatchDealsStream>> {
        let updates = BroadcastStream::new(self.events.subscribe_json("deal.updated"));
        let stream = updates.filter_map(|update| {
            let value = update.ok()?;
            Some(Ok(pb::deals::DealEvent {
                deal_id: value["deal_id"].as_str()?.to_owned(),
                stage: value["stage"].as_str()?.to_owned(),
            }))
        });
        Ok(Response::new(Box::pin(stream) as DealEvents))
    }

    async fn import_deals(
        &self,
        request: Request<Streaming<pb::deals::Deal>>,
    ) -> std::result::Result<Response<pb::deals::ImportDealsResponse>, Status> {
        let mut deals = request.into_inner();
        let mut imported = 0;
        while let Some(deal) = deals.message().await? {
            if deal.title.is_empty() {
                return Err(Status::invalid_argument("a deal needs a title"));
            }
            imported += 1;
        }
        Ok(Response::new(pb::deals::ImportDealsResponse { imported }))
    }

    type EchoStream = DealEvents;

    async fn echo(
        &self,
        request: Request<Streaming<pb::deals::DealEvent>>,
    ) -> Result<Response<Self::EchoStream>> {
        let events = request.into_inner();
        Ok(Response::new(Box::pin(events) as DealEvents))
    }
}

/// The deals feature: its table, its subject and its service.
pub struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<deal::Entity>())
            .migrations(MigrationSet::new("deals").migration(CreateDeals))
            .grpc_service::<DealsGrpc>()
            .grpc_descriptors(pb::FILE_DESCRIPTOR_SET);
    }
}

/// The deals app: a temporary database, the access rules, `GrpcModule`
/// and the deals feature; requests are authenticated by the
/// `TestAuthenticator` (`acting_as`).
pub fn deals_app() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("http.cors.allowed_origins", "http://localhost:3000")
        .module(navree_db::DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(AccessModule::for_root(access_options()))
        .module(GrpcModule::from_config())
        .module(DealsModule)
}

// ---------------------------------------------------------------------------
// The ping service
// ---------------------------------------------------------------------------

/// Answers with what it saw: the message, the request id, the user, the
/// workspace and the authorization. Special messages fail in known ways;
/// `call:<client>` pings the service configured as `grpc.clients.<client>`.
#[injectable]
pub struct PingGrpc {
    channels: Arc<GrpcChannels>,
}

#[grpc_service(server = PingServer)]
#[public]
impl Ping for PingGrpc {
    async fn ping(
        &self,
        request: Request<pb::ping::PingRequest>,
        uow: Uow,
    ) -> Result<Response<pb::ping::PingReply>> {
        let sleep = request.get_ref().sleep_ms;
        if sleep > 0 {
            tokio::time::sleep(Duration::from_millis(sleep)).await;
        }
        let message = request.get_ref().message.clone();
        match message.as_str() {
            "conflict" => return Err(AppError::conflict("already pinged")),
            "boom" => return Err(AppError::internal("the secret connection string")),
            "upstream" => {
                // A status from another service, through `?`.
                Err::<(), _>(Status::failed_precondition("upstream says no"))?;
            }
            "missing" => {
                Repo::<deal::Entity>::find_or_fail(&uow, Uuid::now_v7()).await?;
            }
            _ => {}
        }
        if let Some(client) = message.strip_prefix("call:") {
            let mut upstream = pb::ping::ping_client::PingClient::new(self.channels.get(client)?);
            let reply = upstream
                .ping(pb::ping::PingRequest {
                    message: "from upstream".to_owned(),
                    sleep_ms: 0,
                })
                .await?;
            return Ok(Response::new(reply.into_inner()));
        }
        Ok(Response::new(reply_to(&request, message)))
    }
}

/// What a ping reply reports about `request`.
pub fn reply_to<T>(request: &Request<T>, message: String) -> pb::ping::PingReply {
    let metadata = |key: &str| {
        request
            .metadata()
            .get(key)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    pb::ping::PingReply {
        message,
        request_id: request
            .extensions()
            .get::<RequestId>()
            .map(|id| id.as_str().to_owned())
            .unwrap_or_default(),
        user_id: request
            .extensions()
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id.to_string())
            .unwrap_or_default(),
        workspace: metadata("x-workspace"),
        authorization: metadata("authorization"),
    }
}

/// The ping feature.
pub struct PingModule;

impl Module for PingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.grpc_service::<PingGrpc>()
            .grpc_descriptors(pb::FILE_DESCRIPTOR_SET);
    }
}

/// The ping app with the bearer authenticator and the deals table (for
/// `missing`).
pub fn ping_app() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(navree_db::DatabaseModule::from_config())
        .module(GrpcModule::from_config())
        .module(PingModule)
        .module(DealsTableModule)
        .override_value::<dyn Authenticator>(Arc::new(BearerAuthenticator))
}

/// Only the `deals` table.
pub struct DealsTableModule;

impl Module for DealsTableModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("deals").migration(CreateDeals));
    }
}

// ---------------------------------------------------------------------------
// The guarded ping: an interceptor, authentication and a guard
// ---------------------------------------------------------------------------

/// The docs' interceptor.
pub fn require_client_version(request: Request<()>) -> std::result::Result<Request<()>, Status> {
    match request.metadata().get("x-client-version") {
        Some(_) => Ok(request),
        None => Err(Status::failed_precondition("x-client-version is required")),
    }
}

/// A guard reading `x-api-version` from the headers (the call's metadata).
#[derive(Debug)]
pub struct ApiVersion;

impl Guard for ApiVersion {
    fn check<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, std::result::Result<(), AppError>> {
        Box::pin(async move {
            match parts
                .headers
                .get("x-api-version")
                .and_then(|v| v.to_str().ok())
            {
                Some("2") => Ok(()),
                _ => Err(AppError::forbidden()),
            }
        })
    }
}

/// The ping service behind an interceptor, authentication and a guard.
#[injectable]
pub struct GuardedPing;

#[grpc_service(server = PingServer, interceptor = require_client_version)]
#[authenticated]
#[guard(Arc::new(ApiVersion))]
impl Ping for GuardedPing {
    async fn ping(
        &self,
        request: Request<pb::ping::PingRequest>,
        CurrentPrincipal(me): CurrentPrincipal,
    ) -> Result<Response<pb::ping::PingReply>> {
        let mut reply = reply_to(&request, request.get_ref().message.clone());
        reply.user_id = me.user_id.to_string();
        Ok(Response::new(reply))
    }
}

/// The guarded ping feature.
pub struct GuardedPingModule;

impl Module for GuardedPingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.grpc_service::<GuardedPing>();
    }
}

// ---------------------------------------------------------------------------
// Authentication and auditing
// ---------------------------------------------------------------------------

/// `authorization: Bearer <uuid>` is that user; `Bearer invalid` is an
/// invalid token (401); no header is a guest.
#[derive(Debug, Default)]
pub struct BearerAuthenticator;

impl Authenticator for BearerAuthenticator {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, std::result::Result<Option<Principal>, AppError>> {
        Box::pin(async move {
            let Some(value) = parts.headers.get("authorization") else {
                return Ok(None);
            };
            let token = value
                .to_str()
                .ok()
                .and_then(|v| v.strip_prefix("Bearer "))
                .unwrap_or_default();
            let user_id = Uuid::parse_str(token)
                .map_err(|_| AppError::unauthenticated().with_detail("Invalid token"))?;
            Ok(Some(Principal {
                user_id,
                via: AuthVia::Test,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}

/// An `AuditSink` that keeps every entry.
#[derive(Clone, Default)]
pub struct RecordingAudit(Arc<Mutex<Vec<AuditEntry>>>);

impl RecordingAudit {
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.0.lock().unwrap().clone()
    }
}

impl AuditSink for RecordingAudit {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, std::result::Result<(), BoxError>> {
        self.0.lock().unwrap().push(entry);
        Box::pin(async { Ok(()) })
    }
}
