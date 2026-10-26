//! Workspace-limited clients: their tokens work in one workspace only.

use navree_oauth::NewClient;
use navree_oauth::testing::authorize;
use navree_workspace::{CurrentWorkspace, Members, NewWorkspace, WorkspaceModule, Workspaces};
use serde_json::json;
use uuid::Uuid;

use crate::support::{REDIRECT_URI, World, builder};

mod probe {
    use std::sync::Arc;

    use http::Method;
    use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
    use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
    use serde_json::{Value, json};

    use super::CurrentWorkspace;

    pub struct WorkspaceProbe;

    impl Injectable for WorkspaceProbe {
        fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
            Box::pin(async { Ok(WorkspaceProbe) })
        }
    }

    async fn current(ws: CurrentWorkspace) -> Json<Value> {
        Json(json!({ "slug": ws.slug }))
    }

    impl Controller for WorkspaceProbe {
        fn prefix(&self) -> &'static str {
            "/probe"
        }

        fn routes(self: Arc<Self>) -> Vec<RouteDef> {
            vec![RouteDef::on(Method::GET, "/workspace", current).authenticated()]
        }
    }

    pub struct WorkspaceProbeModule;

    impl Module for WorkspaceProbeModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.controller::<WorkspaceProbe>();
        }
    }
}

async fn world() -> World {
    World::with(
        builder()
            .module(WorkspaceModule::from_config())
            .module(probe::WorkspaceProbeModule),
    )
    .await
}

async fn workspace(world: &World, owner: Uuid, name: &str) -> Uuid {
    let workspaces = world.app.get::<Workspaces>().await;
    let name = name.to_owned();
    world
        .work(|uow| async move {
            Ok(workspaces
                .create(&uow, owner, NewWorkspace::named(name))
                .await?
                .id)
        })
        .await
}

#[tokio::test]
async fn a_workspace_client_s_tokens_are_pinned_to_it() {
    let world = world().await;
    let (ada, token) = world.user("ada@acme.test", &["deals.read"]).await;
    let acme = workspace(&world, ada.id, "Acme").await;
    workspace(&world, ada.id, "Globex").await;
    let client = world
        .register(
            NewClient::public("Acme reports")
                .redirect_uri(REDIRECT_URI)
                .scopes(["deals:read"])
                .workspace(acme),
        )
        .await;
    let tokens = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;

    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_json_path("oauth.workspace_id", json!(acme));
    // No workspace named: the pinned one.
    world
        .client()
        .get("/api/probe/workspace")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("slug", json!("acme"));
    world
        .client()
        .get("/api/probe/workspace")
        .header("x-workspace", "globex")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_error(403, "This token is limited to another workspace.");
    // The user's own sign-in isn't limited.
    world
        .client()
        .get("/api/probe/workspace")
        .header("x-workspace", "globex")
        .bearer(&token)
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn only_members_approve_a_workspace_client() {
    let world = world().await;
    let (ada, _) = world.user("ada@acme.test", &["deals.read"]).await;
    let (eve, eve_token) = world.user("eve@acme.test", &["deals.read"]).await;
    let acme = workspace(&world, ada.id, "Acme").await;
    let client = world
        .register(
            NewClient::public("Acme reports")
                .redirect_uri(REDIRECT_URI)
                .scopes(["deals:read"])
                .workspace(acme),
        )
        .await;
    let (_, challenge) = navree_oauth::testing::pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    world
        .client()
        .post(&format!("/api/oauth/requests/{request}/approve"))
        .bearer(&eve_token)
        .send()
        .await
        .assert_error(
            403,
            "This application is limited to a workspace you are not a member of.",
        );
    let members = world.app.get::<Members>().await;
    world
        .work(|uow| async move {
            Ok(members
                .add(&uow, acme, eve.id, "member".into())
                .await
                .map(|_| ())?)
        })
        .await;
    world.approve(&request, &eve_token).await;
}
