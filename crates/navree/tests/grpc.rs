//! The gRPC docs page (`microservices/grpc`) through the facade: the deals
//! service written as the page writes it (`navree::grpc::{Request,
//! Response, grpc_service}` and `navree::prelude::*`), its interceptor and
//! client examples, and the page's test.
//!
//! Run with `cargo test -p navree --features grpc --test grpc`.

// The page's code declares public items, as an app crate would.
#![allow(missing_docs)]

use std::sync::Arc;

use navree::grpc::testing::TestChannel;
use navree::grpc::{Code, GrpcChannels, GrpcOptions, Request, Response, Status, grpc_service};
use navree::http::CurrentPrincipal;
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{ActiveModelTrait, EntityTrait, QueryOrder, Set};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use uuid::Uuid;

/// The page's `grpc::pb`: the code `build.rs` generates (here, navree-grpc's
/// checked-in copy of it).
#[allow(clippy::all, missing_docs)]
pub mod pb {
    include!("../../navree-grpc/tests/support/pb/crm.deals.v1.rs");

    pub const FILE_DESCRIPTOR_SET: &[u8] =
        include_bytes!("../../navree-grpc/tests/support/pb/deals_descriptor.bin");
}

use pb::deals_server::{Deals, DealsServer};

// ---------------------------------------------------------------------------
// What `crm_core` provides on the page: the entity, its subject and the
// `Move` action.
// ---------------------------------------------------------------------------

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
        pub owner_id: Uuid,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub type Deal = deal::Entity;

impl Subject for deal::Entity {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}

/// The `move` action.
pub struct Move;

impl ActionMarker for Move {
    const NAME: &'static str = "move";
}

struct CreateDeals;

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
                    .col(ColumnDef::new("owner_id").uuid().not_null())
                    .to_owned(),
            )
            .await
    }
}

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    if let Some(user) = &ctx.user {
        rules.can(READ, "Deal").when(field("owner_id").eq(user.id));
        rules
            .can("move", "Deal")
            .when(field("owner_id").eq(user.id));
    }
}

// ---------------------------------------------------------------------------
// The deals service's own code: `DealService` and the conversions.
// ---------------------------------------------------------------------------

pub struct DealView(deal::Model);

pub struct DealPage(Vec<deal::Model>);

pub struct MoveInput {
    stage: String,
}

impl TryFrom<pb::MoveDealRequest> for MoveInput {
    type Error = AppError;

    fn try_from(request: pb::MoveDealRequest) -> Result<Self> {
        match request.stage.as_str() {
            "lead" | "qualified" | "won" | "lost" => Ok(MoveInput {
                stage: request.stage,
            }),
            other => Err(AppError::bad_request(format!("unknown stage `{other}`"))),
        }
    }
}

impl From<DealView> for pb::Deal {
    fn from(DealView(deal): DealView) -> Self {
        pb::Deal {
            id: deal.id.to_string(),
            title: deal.title,
            stage: deal.stage,
            owner_id: deal.owner_id.to_string(),
            ..pb::Deal::default()
        }
    }
}

impl From<DealPage> for pb::ListDealsResponse {
    fn from(DealPage(deals): DealPage) -> Self {
        pb::ListDealsResponse {
            total: deals.len() as u64,
            deals: deals.into_iter().map(|d| DealView(d).into()).collect(),
        }
    }
}

#[injectable]
pub struct DealService;

impl DealService {
    pub async fn view(&self, _uow: &Uow, deal: deal::Model) -> Result<DealView> {
        Ok(DealView(deal))
    }

    pub async fn of_company_page(
        &self,
        uow: &Uow,
        scope: Accessible<Deal, Read>,
        _input: &pb::ListDealsRequest,
    ) -> Result<DealPage> {
        let select = scope.apply(deal::Entity::find().order_by_asc(deal::Column::Title));
        Ok(DealPage(select.all(&*uow.tx().await?).await?))
    }

    pub async fn move_to(
        &self,
        uow: &Uow,
        _actor: Uuid,
        deal: deal::Model,
        input: MoveInput,
    ) -> Result<DealView> {
        let mut model: deal::ActiveModel = deal.into();
        model.stage = Set(input.stage);
        Ok(DealView(model.update(&*uow.tx().await?).await?))
    }
}

// ---------------------------------------------------------------------------
// The page's service, as written there.
// ---------------------------------------------------------------------------

#[injectable]
pub struct DealsGrpc {
    deals: Arc<DealService>,
}

type DealEvents = std::pin::Pin<
    Box<dyn tokio_stream::Stream<Item = std::result::Result<pb::DealEvent, Status>> + Send>,
>;

#[grpc_service(server = DealsServer, interceptor = require_client_version)]
#[authenticated]
impl Deals for DealsGrpc {
    #[can(Read, Deal, from = "id")]
    async fn get_deal(
        &self,
        _request: Request<pb::GetDealRequest>,
        uow: Uow,
        deal: Authorized<Deal, Read>,
    ) -> Result<Response<pb::Deal>> {
        let view = self.deals.view(&uow, deal.into_inner()).await?;
        Ok(Response::new(view.into()))
    }

    #[can(Read, Deal)]
    async fn list_deals(
        &self,
        request: Request<pb::ListDealsRequest>,
        uow: Uow,
        scope: Accessible<Deal, Read>,
    ) -> Result<Response<pb::ListDealsResponse>> {
        let input = request.into_inner();
        let page = self.deals.of_company_page(&uow, scope, &input).await?;
        Ok(Response::new(page.into()))
    }

    #[can(Move, Deal, from = "id")]
    async fn move_deal(
        &self,
        Valid(request): Valid<Request<pb::MoveDealRequest>>,
        uow: Uow,
        CurrentPrincipal(me): CurrentPrincipal,
        deal: Authorized<Deal, Move>,
    ) -> Result<Response<pb::Deal>> {
        let input = request.into_inner().try_into()?;
        let view = self
            .deals
            .move_to(&uow, me.user_id, deal.into_inner(), input)
            .await?;
        Ok(Response::new(view.into()))
    }

    type WatchDealsStream = DealEvents;

    #[can(Read, Deal)]
    async fn watch_deals(
        &self,
        _request: Request<pb::WatchDealsRequest>,
    ) -> Result<Response<Self::WatchDealsStream>> {
        Ok(Response::new(Box::pin(tokio_stream::empty()) as DealEvents))
    }

    async fn import_deals(
        &self,
        _request: Request<navree::grpc::Streaming<pb::Deal>>,
    ) -> Result<Response<pb::ImportDealsResponse>> {
        Err(AppError::from_status(
            navree::axum::http::StatusCode::NOT_IMPLEMENTED,
        ))
    }

    type EchoStream = DealEvents;

    async fn echo(
        &self,
        request: Request<navree::grpc::Streaming<pb::DealEvent>>,
    ) -> Result<Response<Self::EchoStream>, Status> {
        Ok(Response::new(Box::pin(request.into_inner()) as DealEvents))
    }
}

/// The page's interceptor.
fn require_client_version(request: Request<()>) -> std::result::Result<Request<()>, Status> {
    match request.metadata().get("x-client-version") {
        Some(_) => Ok(request),
        None => Err(Status::failed_precondition("x-client-version is required")),
    }
}

#[module(providers = [DealService], register = Self::wire)]
pub struct DealsModule;

impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<deal::Entity>())
            .migrations(MigrationSet::new("deals").migration(CreateDeals))
            .grpc_service::<DealsGrpc>()
            .grpc_descriptors(pb::FILE_DESCRIPTOR_SET);
    }
}

/// The page's client example.
#[injectable]
pub struct DealLookup {
    channels: Arc<GrpcChannels>,
}

impl DealLookup {
    pub async fn get(&self, id: Uuid) -> Result<pb::Deal> {
        let mut deals = pb::deals_client::DealsClient::new(self.channels.get("deals")?);
        let reply = deals
            .get_deal(pb::GetDealRequest { id: id.to_string() })
            .await?;
        Ok(reply.into_inner())
    }
}

/// The page's `GrpcModule::for_root` example.
#[allow(dead_code)]
fn configured() -> std::result::Result<impl Module, BoxError> {
    Ok(GrpcModule::for_root(GrpcOptions {
        addr: Some("0.0.0.0:50051".parse()?),
        ..GrpcOptions::default()
    }))
}

/// The page's test (with the interceptor's metadata).
#[tokio::test]
async fn get_deal_checks_access() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
        .module(GrpcModule::from_config())
        .module(DealsModule)
        .build()
        .await;
    let rep = Uuid::now_v7();
    let deal_id = Uuid::now_v7();
    deal::ActiveModel {
        id: Set(deal_id),
        title: Set("Acme renewal".into()),
        stage: Set("lead".into()),
        owner_id: Set(rep),
    }
    .insert(app.get::<navree::db::Db>().await.conn())
    .await
    .unwrap();
    let request = || pb::GetDealRequest {
        id: deal_id.to_string(),
    };
    let channel = || TestChannel::new(&app).metadata("x-client-version", "1.0");

    let mut guest = pb::deals_client::DealsClient::new(channel());
    let status = guest.get_deal(request()).await.unwrap_err();
    assert_eq!(status.code(), Code::Unauthenticated);

    let mut stranger =
        pb::deals_client::DealsClient::new(channel().acting_as(principal(Uuid::now_v7())));
    let status = stranger.get_deal(request()).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);

    let mut owner = pb::deals_client::DealsClient::new(channel().acting_as(principal(rep)));
    let deal = owner.get_deal(request()).await.unwrap().into_inner();
    assert_eq!(deal.owner_id, rep.to_string());

    let moved = owner
        .move_deal(pb::MoveDealRequest {
            id: deal_id.to_string(),
            stage: "won".into(),
            position: Some(1),
            lost_reason: None,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(moved.stage, "won");
    let status = owner
        .move_deal(pb::MoveDealRequest {
            id: deal_id.to_string(),
            stage: "sideways".into(),
            position: None,
            lost_reason: None,
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument, "a failed TryFrom");

    let list = owner
        .list_deals(pb::ListDealsRequest::default())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(list.total, 1);

    let status =
        pb::deals_client::DealsClient::new(TestChannel::new(&app).acting_as(principal(rep)))
            .get_deal(request())
            .await
            .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition, "the interceptor");

    // The client example, outside any call: no credentials to forward.
    let lookup = DealLookup {
        channels: app.get::<GrpcChannels>().await,
    };
    let error = lookup
        .get(deal_id)
        .await
        .err()
        .expect("no client named deals");
    assert_eq!(error.status.as_u16(), 500);

    app.shutdown().await.unwrap();
}
