//! Organizations and partitions keep data apart: `CurrentOrganization`
//! with `OrgRepo`, workspace entry through organization grants, and
//! `PartitionScope` / `CurrentPartition` with `PartitionRepo`, on SQLite.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::Method;
use navree_db::Uow;
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_workspace::{
    CurrentOrganization, CurrentPartition, CurrentWorkspace, GrantedBy, InOrganization, OrgRepo,
    PartitionRepo, PartitionScope, PartitionSelector, PinnedWorkspace, Selector,
    WorkspaceSelector,
};
use sea_orm::sea_query::Query;
use sea_orm::{ActiveValue::Set, ConnectionTrait, DbErr, EntityTrait};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{World, contact, deal, org_builder};

struct ProbeController;

impl Injectable for ProbeController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ProbeController) })
    }
}

async fn organization(org: CurrentOrganization) -> Json<Value> {
    Json(json!({
        "id": org.id(),
        "slug": org.slug,
        "role": org.role().as_str(),
        "resolver": org.resolver().map(|r| r.as_str()),
        "permissions": org.permissions(),
        "workspaces": org.workspaces(),
        "workspace_ids": org.workspace_ids(),
        "can_read_reports": org.has_permission("reports.read"),
    }))
}

/// Every deal of the organization's workspaces the member reaches, by
/// title.
async fn report(org: CurrentOrganization, uow: Uow) -> Result<Json<Value>, AppError> {
    let tx = uow.tx().await?;
    let mut titles: Vec<String> = OrgRepo::<deal::Entity>::select(org.workspace_ids())
        .all(&*tx)
        .await?
        .into_iter()
        .map(|deal| deal.title)
        .collect();
    titles.sort();
    let scoped = org.scope(deal::Entity::find()).all(&*tx).await?.len();
    Ok(Json(json!({ "titles": titles, "scoped": scoped })))
}

async fn show_deal(
    org: CurrentOrganization,
    uow: Uow,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<deal::Model>, AppError> {
    Ok(Json(
        OrgRepo::<deal::Entity>::find_or_fail(&uow, org.workspace_ids(), id).await?,
    ))
}

async fn workspace(ws: CurrentWorkspace) -> Json<Value> {
    let via = match ws.granted_by() {
        GrantedBy::Membership => json!("membership"),
        GrantedBy::Organization {
            organization_id,
            role,
        } => json!({ "organization_id": organization_id, "role": role.as_str() }),
    };
    Json(json!({
        "id": ws.id(),
        "role": ws.role().as_str(),
        "user_id": ws.user_id(),
        "granted_by": via,
        "permissions": ws.permissions(),
    }))
}

async fn contacts(scope: PartitionScope, uow: Uow) -> Result<Json<Value>, AppError> {
    let mut names: Vec<String> = scope
        .scope(contact::Entity::find())
        .all(&*uow.tx().await?)
        .await?
        .into_iter()
        .map(|contact| contact.name)
        .collect();
    names.sort();
    Ok(Json(json!({
        "names": names,
        "selector": scope.selector(),
        "allowed": scope.allowed(),
        "current": scope.current().map(|p| p.slug.clone()),
    })))
}

async fn add_contact(
    ws: CurrentWorkspace,
    partition: CurrentPartition,
    uow: Uow,
    Json(input): Json<Value>,
) -> Result<Json<contact::Model>, AppError> {
    let created = PartitionRepo::<contact::Entity>::insert(
        &uow,
        ws.id(),
        partition.id(),
        contact::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set(input["name"].as_str().unwrap_or_default().to_owned()),
            ..Default::default()
        },
    )
    .await?;
    Ok(Json(created))
}

async fn partition(partition: CurrentPartition) -> Json<Value> {
    Json(json!({
        "id": partition.id(),
        "slug": partition.slug,
        "resolver": partition.resolver().as_str(),
        "workspace_id": partition.workspace_id(),
    }))
}

impl Controller for ProbeController {
    fn prefix(&self) -> &'static str {
        "/"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/org", organization).authenticated(),
            RouteDef::on(Method::GET, "/o/{organization}/org", organization).authenticated(),
            RouteDef::on(Method::GET, "/org/report", report).authenticated(),
            RouteDef::on(Method::GET, "/org/deals/{id}", show_deal).authenticated(),
            RouteDef::on(Method::GET, "/org/guarded", organization)
                .permissions(&["reports.read"])
                .guard(InOrganization::guard()),
            RouteDef::on(Method::GET, "/ws", workspace).authenticated(),
            RouteDef::on(Method::GET, "/contacts", contacts).authenticated(),
            RouteDef::on(Method::POST, "/contacts", add_contact).authenticated(),
            RouteDef::on(Method::GET, "/partition", partition).authenticated(),
        ]
    }
}

/// The probe routes and a test layer that pins requests sending
/// `x-test-pin: <workspace id>` to that workspace.
struct ProbeModule;

impl Module for ProbeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ProbeController>()
            .http_layer(navree_http::GlobalLayer::new(
                navree_http::LayerSlot::AuthContext,
                "test-pin",
                |_ctx, router| Ok(router.layer(axum::middleware::from_fn(pin_from_header))),
            ));
    }
}

async fn pin_from_header(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let pin = request
        .headers()
        .get("x-test-pin")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok());
    if let Some(pin) = pin {
        request.extensions_mut().insert(PinnedWorkspace(pin));
    }
    next.run(request).await
}

async fn world() -> World {
    World::with(
        org_builder()
            .config("workspace.organizations.base_domain", "partners.test")
            .module(ProbeModule),
    )
    .await
}

fn jwt(claims: Value) -> String {
    format!(
        "eyJhbGciOiJIUzI1NiJ9.{}.c2lnbmF0dXJl",
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}

/// Inserts a deal into `workspace`.
async fn deal(world: &World, workspace: Uuid, title: &str) -> Uuid {
    let id = Uuid::now_v7();
    world
        .db()
        .await
        .conn()
        .execute(
            Query::insert()
                .into_table("deals")
                .columns(["id", "workspace_id", "title"])
                .values_panic([id.into(), workspace.into(), title.into()]),
        )
        .await
        .unwrap();
    id
}

/// Inserts a contact into `workspace` and `partition`.
async fn contact_row(world: &World, workspace: Uuid, partition: Option<Uuid>, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    world
        .db()
        .await
        .conn()
        .execute(
            Query::insert()
                .into_table("contacts")
                .columns(["id", "workspace_id", "partition_id", "name"])
                .values_panic([id.into(), workspace.into(), partition.into(), name.into()]),
        )
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn every_resolver_finds_the_organization_and_errors_follow_the_shape() {
    let world = world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let ken = world.user("Ken", "ken@example.com").await;
    let acme = world.organization(ada, "Acme").await;
    let ws = world.workspace(ada, "Acme UK").await;
    let client = world.as_user(ada);

    for (request, resolver) in [
        (client.get("/api/org").header("x-organization", "acme"), "header"),
        (
            client
                .get("/api/org")
                .header("x-organization", acme.id.to_string().as_str()),
            "header",
        ),
        (client.get("/api/o/acme/org"), "path"),
        (client.get("/api/org").header("host", "acme.partners.test"), "subdomain"),
        (
            client
                .get("/api/org")
                .bearer(&jwt(json!({ "organization": "acme" }))),
            "claim",
        ),
    ] {
        request
            .send()
            .await
            .assert_status(200)
            .assert_json_path("id", json!(acme.id))
            .assert_json_path("role", json!("owner"))
            .assert_json_path("resolver", json!(resolver))
            .assert_json_path("permissions", json!(["all.*"]));
    }
    client
        .get("/api/org")
        .send()
        .await
        .assert_error(400, "The request names no organization.");
    client
        .get("/api/org")
        .header("x-organization", "nope")
        .send()
        .await
        .assert_error(404, "Organization not found.");
    world
        .as_user(ken)
        .get("/api/org")
        .header("x-organization", "acme")
        .send()
        .await
        .assert_error(403, "You are not a member of this organization.");
    world
        .app
        .client()
        .get("/api/org")
        .header("x-organization", "acme")
        .send()
        .await
        .assert_error(401, None);
    // A token pinned to a workspace stays out of organizations.
    client
        .get("/api/org")
        .header("x-organization", "acme")
        .header("x-test-pin", ws.id.to_string().as_str())
        .send()
        .await
        .assert_error(403, "This token is limited to a workspace.");
}

#[tokio::test]
async fn route_checks_use_the_organization_role_behind_the_guard() {
    let world = world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let ken = world.user("Ken", "ken@example.com").await;
    let acme = world.organization(ada, "Acme").await;
    world
        .grant(acme.id, grace, "admin", WorkspaceSelector::All)
        .await;
    world
        .grant(acme.id, ken, "partner", WorkspaceSelector::All)
        .await;

    // `partner` carries `reports.read`; the built-in `admin` doesn't.
    world
        .as_user(ken)
        .get("/api/org/guarded")
        .header("x-organization", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("role", json!("partner"))
        .assert_json_path("can_read_reports", json!(true));
    world
        .as_user(grace)
        .get("/api/org/guarded")
        .header("x-organization", "acme")
        .send()
        .await
        .assert_error(403, None);
    world
        .as_user(ada)
        .get("/api/org/guarded")
        .header("x-organization", "acme")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn organization_a_cannot_see_organization_b() {
    let world = world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let bob = world.user("Bob", "bob@example.com").await;
    let linus = world.user("Linus", "linus@example.com").await;
    let a = world.organization(ada, "Org A").await;
    let b = world.organization(bob, "Org B").await;
    let a1 = world.workspace(ada, "A One").await;
    let a2 = world.workspace(ada, "A Two").await;
    let b1 = world.workspace(bob, "B One").await;
    world.attach(a.id, a1.id).await;
    world.attach(a.id, a2.id).await;
    world.attach(b.id, b1.id).await;
    deal(&world, a1.id, "a1 deal").await;
    deal(&world, a2.id, "a2 deal").await;
    let b_deal = deal(&world, b1.id, "b1 deal").await;
    let outside = world.workspace(ada, "Outside").await;
    deal(&world, outside.id, "outside deal").await;
    world
        .grant(a.id, linus, "member", WorkspaceSelector::only([a2.id]))
        .await;

    let owner = world.as_user(ada);
    owner
        .get("/api/org/report")
        .header("x-organization", "org-a")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("titles", json!(["a1 deal", "a2 deal"]))
        .assert_json_path("scoped", json!(2));
    owner
        .get("/api/org/report")
        .header("x-organization", "org-b")
        .send()
        .await
        .assert_error(403, "You are not a member of this organization.");
    owner
        .get(&format!("/api/org/deals/{b_deal}"))
        .header("x-organization", "org-a")
        .send()
        .await
        .assert_status(404);
    world
        .as_user(bob)
        .get("/api/org/report")
        .header("x-organization", "org-b")
        .send()
        .await
        .assert_json_path("titles", json!(["b1 deal"]));

    // A member limited to one workspace reads that workspace only.
    world
        .as_user(linus)
        .get("/api/org/report")
        .header("x-organization", "org-a")
        .send()
        .await
        .assert_json_path("titles", json!(["a2 deal"]));
    world
        .as_user(linus)
        .get("/api/org")
        .header("x-organization", "org-a")
        .send()
        .await
        .assert_json_path("workspaces", json!([a2.id]))
        .assert_json_path("workspace_ids", json!([a2.id]));

    // Organization grants open the workspaces they reach, and no others.
    let member = world.as_user(linus);
    member
        .get("/api/ws")
        .header("x-workspace", "a-two")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("role", json!("member"))
        .assert_json_path("user_id", json!(linus))
        .assert_json_path(
            "granted_by",
            json!({ "organization_id": a.id, "role": "member" }),
        )
        .assert_json_path(
            "permissions",
            json!([
                "deals.*",
                "workspace.members.read",
                "workspace.partitions.read",
                "workspace.read"
            ]),
        );
    member
        .get("/api/ws")
        .header("x-workspace", "a-one")
        .send()
        .await
        .assert_error(403, "You are not a member of this workspace.");
    member
        .get("/api/ws")
        .header("x-workspace", "b-one")
        .send()
        .await
        .assert_error(403, "You are not a member of this workspace.");
    owner
        .get("/api/ws")
        .header("x-workspace", "a-two")
        .send()
        .await
        .assert_json_path("granted_by", json!("membership"));
    // The organization's owner reaches workspaces they don't belong to, as
    // their owner.
    let theirs = world.workspace(linus, "Linus Co").await;
    world.attach(a.id, theirs.id).await;
    owner
        .get("/api/workspaces/linus-co")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("role", json!("owner"))
        .assert_json_path("organization_id", json!(a.id));
    owner
        .get("/api/ws")
        .header("x-workspace", "linus-co")
        .send()
        .await
        .assert_json_path(
            "granted_by",
            json!({ "organization_id": a.id, "role": "owner" }),
        );
}

#[tokio::test]
async fn partition_grants_hold() {
    let world = world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let linus = world.user("Linus", "linus@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let other = world.workspace(ada, "Other").await;
    world.join(acme.id, grace, "member").await;
    world.join(acme.id, linus, "member").await;
    let uk = world.partition(acme.id, "acme.co.uk").await;
    let de = world.partition(acme.id, "acme.de").await;
    let foreign = world.partition(other.id, "other.com").await;
    contact_row(&world, acme.id, Some(uk.id), "uk contact").await;
    let de_contact = contact_row(&world, acme.id, Some(de.id), "de contact").await;
    contact_row(&world, acme.id, None, "unsorted contact").await;
    contact_row(&world, other.id, Some(foreign.id), "other contact").await;
    world
        .limit_partitions(acme.id, linus, PartitionSelector::only([uk.id]))
        .await;

    // A full member sees every partition, rows without one included.
    let full = world.as_user(grace);
    full.get("/api/contacts")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path(
            "names",
            json!(["de contact", "uk contact", "unsorted contact"]),
        )
        .assert_json_path("selector", json!("all"))
        .assert_json_path("current", json!(null));
    full.get("/api/contacts")
        .header("x-workspace", "acme")
        .header("x-partition", "acme-de")
        .send()
        .await
        .assert_json_path("names", json!(["de contact"]))
        .assert_json_path("selector", json!([de.id]))
        .assert_json_path("current", json!("acme-de"));

    // A limited member sees their partitions only.
    let limited = world.as_user(linus);
    limited
        .get("/api/contacts")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_json_path("names", json!(["uk contact"]))
        .assert_json_path("allowed", json!([uk.id]));
    limited
        .get("/api/contacts")
        .header("x-workspace", "acme")
        .header("x-partition", "acme-de")
        .send()
        .await
        .assert_error(403, "You don't have access to this partition.");
    limited
        .get("/api/contacts")
        .header("x-workspace", "acme")
        .header("x-partition", foreign.id.to_string().as_str())
        .send()
        .await
        .assert_error(404, "Partition not found.");

    // `CurrentPartition` needs a partition, then writes into it.
    limited
        .post("/api/contacts")
        .header("x-workspace", "acme")
        .json(&json!({ "name": "new" }))
        .send()
        .await
        .assert_error(400, "The request names no partition.");
    limited
        .post("/api/contacts")
        .header("x-workspace", "acme")
        .header("x-partition", "acme-co-uk")
        .json(&json!({ "name": "new" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("partition_id", json!(uk.id))
        .assert_json_path("workspace_id", json!(acme.id));
    limited
        .get("/api/partition")
        .header("x-workspace", "acme")
        .header("x-partition", "acme-co-uk")
        .send()
        .await
        .assert_json_path("slug", json!("acme-co-uk"))
        .assert_json_path("resolver", json!("header"));

    // `PartitionRepo` refuses rows outside the selector.
    let db = world.db().await;
    let uow = db.uow(None, None).await;
    let only_uk = Selector::only([uk.id]);
    let row = contact::Entity::find_by_id(de_contact)
        .one(&*uow.tx().await.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        PartitionRepo::<contact::Entity>::find_or_fail(&uow, acme.id, &only_uk, de_contact).await,
        Err(DbErr::RecordNotFound(_))
    ));
    assert!(matches!(
        PartitionRepo::<contact::Entity>::delete(&uow, acme.id, &only_uk, row.clone()).await,
        Err(DbErr::RecordNotFound(_))
    ));
    let moved = PartitionRepo::<contact::Entity>::update(
        &uow,
        acme.id,
        &only_uk,
        contact::ActiveModel {
            id: Set(de_contact),
            partition_id: Set(Some(uk.id)),
            ..Default::default()
        },
    )
    .await;
    assert!(matches!(moved, Err(DbErr::RecordNotFound(_))));
    let all = Selector::All;
    let renamed = PartitionRepo::<contact::Entity>::update(
        &uow,
        acme.id,
        &all,
        contact::ActiveModel {
            id: Set(de_contact),
            name: Set("de renamed".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(renamed.name, "de renamed");
    assert_eq!(
        PartitionRepo::<contact::Entity>::select(acme.id, &Selector::Only(vec![]))
            .all(&*uow.tx().await.unwrap())
            .await
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        OrgRepo::<deal::Entity>::select(&[])
            .all(&*uow.tx().await.unwrap())
            .await
            .unwrap()
            .len(),
        0
    );
    PartitionRepo::<contact::Entity>::delete(&uow, acme.id, &all, row)
        .await
        .unwrap();
    uow.rollback().await.unwrap();
}

#[tokio::test]
async fn organization_members_see_every_partition_of_the_workspaces_they_reach() {
    let world = world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let org = world.organization(ada, "Acme").await;
    let acme = world.workspace(ada, "Acme").await;
    world.attach(org.id, acme.id).await;
    world
        .grant(org.id, grace, "viewer", WorkspaceSelector::All)
        .await;
    let uk = world.partition(acme.id, "acme.co.uk").await;
    contact_row(&world, acme.id, Some(uk.id), "uk contact").await;
    contact_row(&world, acme.id, None, "unsorted contact").await;
    world
        .as_user(grace)
        .get("/api/contacts")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("names", json!(["uk contact", "unsorted contact"]))
        .assert_json_path("allowed", json!("all"));
}
