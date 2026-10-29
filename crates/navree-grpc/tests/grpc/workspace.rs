//! Workspaces in gRPC calls: `CurrentWorkspace` resolves the call's
//! workspace from the `x-workspace` metadata, checks membership and
//! answers with the status of the same meaning.

use navree_access::{AccessModule, AccessOptions};
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{DatabaseModule, Db, Executor as _, MigrationSet, ModuleBuilderDbExt, Statement};
use navree_events::EventsModule;
use navree_grpc::testing::TestChannel;
use navree_grpc::{Code, GrpcModule, ModuleBuilderGrpcExt, Request, Response, grpc_service};
use navree_http::Result;
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::injectable;
use navree_mail::{MailModule, MailOptions, TransportConfig};
use navree_testing::{TestApp, principal};
use navree_view::{ViewModule, ViewOptions};
use navree_workspace::{CurrentWorkspace, NewWorkspace, Workspace, WorkspaceModule, Workspaces};
use serde_json::Value;
use uuid::Uuid;

use crate::support::pb::ping::ping_client::PingClient;
use crate::support::pb::ping::ping_server::{Ping, PingServer};
use crate::support::pb::ping::{PingReply, PingRequest};
use crate::support::reply_to;

/// Answers with the slug and the id of the call's workspace.
#[injectable]
pub struct WorkspacePing;

#[grpc_service(server = PingServer)]
#[authenticated]
impl Ping for WorkspacePing {
    async fn ping(
        &self,
        request: Request<PingRequest>,
        workspace: CurrentWorkspace,
    ) -> Result<Response<PingReply>> {
        let mut reply = reply_to(&request, workspace.slug.clone());
        reply.workspace = workspace.id().to_string();
        Ok(Response::new(reply))
    }
}

/// `users(id, name, email)`, the table navree-auth creates.
struct CreateUsers;

impl Migration for CreateUsers {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("users", |t| {
                t.uuid("id").primary();
                t.string("name");
                t.string("email").unique();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("users").await
    }
}

struct WorkspacePingModule;

impl Module for WorkspacePingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("auth").migration(SchemaMigration::new(
            "m20261026_000001_create_users",
            CreateUsers,
        )))
        .grpc_service::<WorkspacePing>();
    }
}

async fn app() -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(ViewModule::for_root(ViewOptions {
            dirs: vec![],
            reload: false,
        }))
        .module(MailModule::for_root(MailOptions {
            from: "Navree <hello@navree.test>".into(),
            transport: TransportConfig::Array,
            ..MailOptions::default()
        }))
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(GrpcModule::from_config())
        .module(WorkspacePingModule)
        .build()
        .await
}

async fn user(app: &TestApp, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    app.get::<Db>()
        .await
        .pool()
        .execute(&Statement::portable(
            "INSERT INTO users (id, name, email) VALUES (?, ?, ?)",
            [id.into(), name.into(), format!("{name}@example.com").into()],
        ))
        .await
        .unwrap();
    id
}

async fn workspace(app: &TestApp, owner: Uuid, name: &str) -> Workspace {
    let uow = app.get::<Db>().await.uow(None, None).await;
    let workspaces = app.get::<Workspaces>().await;
    let created = uow
        .scope(workspaces.create(&uow, owner, NewWorkspace::named(name)))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    created
}

fn ping() -> PingRequest {
    PingRequest {
        message: String::new(),
        sleep_ms: 0,
    }
}

#[tokio::test]
async fn current_workspace_comes_from_the_x_workspace_metadata() {
    let app = app().await;
    let ada = user(&app, "ada").await;
    let bob = user(&app, "bob").await;
    let acme = workspace(&app, ada, "Acme").await;
    let member = || TestChannel::new(&app).acting_as(principal(ada));

    // By slug and by id.
    let mut by_slug = PingClient::new(member().metadata("x-workspace", &acme.slug));
    let reply = by_slug.ping(ping()).await.unwrap().into_inner();
    assert_eq!(reply.message, acme.slug);
    assert_eq!(reply.workspace, acme.id.to_string());
    let mut by_id = PingClient::new(member().metadata("x-workspace", &acme.id.to_string()));
    let reply = by_id.ping(ping()).await.unwrap().into_inner();
    assert_eq!(reply.message, acme.slug);

    // A caller outside the workspace, an unknown workspace, none at all.
    let stranger = TestChannel::new(&app)
        .acting_as(principal(bob))
        .metadata("x-workspace", &acme.slug);
    let status = PingClient::new(stranger).ping(ping()).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(status.message(), "You are not a member of this workspace.");

    let unknown = member().metadata("x-workspace", "nowhere");
    let status = PingClient::new(unknown).ping(ping()).await.unwrap_err();
    assert_eq!(status.code(), Code::NotFound);

    let status = PingClient::new(member()).ping(ping()).await.unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
    assert_eq!(status.message(), "The request names no workspace.");

    app.shutdown().await.unwrap();
}
