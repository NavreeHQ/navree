//! PostgreSQL (with TimescaleDB) and MySQL: the connection lifecycle on a
//! real server, so the migration and every statement of the
//! `IntegrationConnection` model run on each: the upsert of a reconnect,
//! the owner chain's `OR` groups, token and health updates, the `IS NULL
//! OR <` lookup of due checks, deletes, and timestamps that come back as
//! they were written.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL`, or `--features mysql` and
//! `NAVREE_TEST_MYSQL_URL`, to a URL of a throwaway server whose user may
//! create databases. Each test creates its own database and drops it at
//! the end.

use std::time::Duration;

use chrono::TimeDelta;
use navree_db::Db;
use navree_db::sea_orm::{ConnectionTrait, Database, DbBackend};
use navree_integrations::{
    ConnectCallback, ConnectOptions, ConnectionStatus, CredentialKind, FakeGrant,
    IntegrationConnection, Owner,
};
use navree_orm::Model as _;
use uuid::Uuid;

use crate::support::{Harness, OTHER, harness, start, with_workspaces};

const TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

/// A database of its own on the test server.
struct Scratch {
    admin: String,
    name: String,
    url: String,
    mysql: bool,
}

impl Scratch {
    async fn new(var: &str, test: &str, mysql: bool) -> Option<Scratch> {
        let admin = match std::env::var(var) {
            Ok(url) if !url.is_empty() => url,
            _ => {
                println!("SKIPPED {test}: set {var}");
                return None;
            }
        };
        let name = format!("navree_integrations_{}", Uuid::now_v7().simple());
        let conn = Database::connect(&admin).await.unwrap();
        let create = if mysql {
            format!("CREATE DATABASE `{name}` CHARACTER SET utf8mb4")
        } else {
            format!("CREATE DATABASE {name}")
        };
        conn.execute_unprepared(&create).await.unwrap();
        conn.close().await.unwrap();
        let (base, rest) = admin.rsplit_once('/').unwrap();
        let options = rest.find('?').map_or("", |at| &rest[at..]);
        let url = format!("{base}/{name}{options}");
        Some(Scratch {
            admin,
            name,
            url,
            mysql,
        })
    }

    async fn drop(self) {
        let conn = Database::connect(&self.admin).await.unwrap();
        let drop = if self.mysql {
            format!("DROP DATABASE IF EXISTS `{}`", self.name)
        } else {
            format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name)
        };
        conn.execute_unprepared(&drop).await.unwrap();
    }
}

/// Connects `owner` to HubSpot with a token that lasts ten minutes.
async fn connect_hubspot(h: &Harness, owner: Owner, code: &str, token: &str) -> Uuid {
    h.fake.on_code(
        "hubspot",
        code,
        FakeGrant::new()
            .access_token(token)
            .refresh_token("refresh-secret")
            .expires_in(600)
            .external_id("hub-portal-1"),
    );
    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(owner, "hubspot", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    integrations
        .complete(owner, "hubspot", &ConnectCallback::code(state, code), None)
        .await
        .unwrap()
        .connection
        .id
}

/// Connect, inherit, reconnect, refresh, check, list and disconnect on
/// `scratch`.
async fn flow(scratch: &Scratch) {
    let h = harness(|b| {
        with_workspaces(b)
            .config("database.url", scratch.url.as_str())
            .config("database.migrate_on_boot", true)
            .config("integrations.check_interval_mins", 60)
    })
    .await;
    let db = h.app.get::<Db>().await;
    let expected = if scratch.mysql {
        DbBackend::MySql
    } else {
        DbBackend::Postgres
    };
    assert_eq!(db.backend(), expected);
    let integrations = h.integrations().await;

    let acme = h.workspace(OTHER, "acme").await;
    let group = h.organization(OTHER, "acme-group", &[acme.id]).await;
    let workspace = Owner::Workspace(acme.id);
    let organization = Owner::Organization(group);

    // The organization's admin consent; the workspace's own OAuth install.
    let microsoft = h.connect_microsoft(organization, TENANT).await;
    assert_eq!(microsoft.kind, CredentialKind::AppOnly);
    assert_eq!(microsoft.external_id.as_deref(), Some(TENANT));
    assert_eq!(microsoft.owner(), organization);
    assert_eq!(
        (microsoft.created_at, microsoft.updated_at),
        (start(), start())
    );
    let hubspot = connect_hubspot(&h, workspace, "code-1", "first").await;

    // The workspace sees its own connection, then its organization's.
    let visible = integrations.connections(workspace).await.unwrap();
    let names: Vec<(&str, Owner)> = visible
        .iter()
        .map(|info| (info.provider.as_str(), info.owner()))
        .collect();
    assert_eq!(names, [("hubspot", workspace), ("microsoft", organization)]);
    let inherited = integrations
        .connection(workspace, "microsoft", "default")
        .await
        .unwrap();
    assert_eq!(inherited.id, microsoft.id);
    assert_eq!(
        integrations.connections(organization).await.unwrap().len(),
        1
    );

    // Connecting again replaces the row in place.
    h.advance(Duration::from_secs(60));
    let again = connect_hubspot(&h, workspace, "code-2", "second").await;
    assert_eq!(again, hubspot);
    let row = integrations.find(hubspot).await.unwrap();
    assert_eq!(row.created_at, start());
    assert_eq!(row.updated_at, start() + TimeDelta::seconds(60));
    assert_eq!(
        integrations.token(hubspot).await.unwrap().expose(),
        "second"
    );

    // The credentials and token are stored encrypted.
    let stored = IntegrationConnection::find(&*db, hubspot)
        .await
        .unwrap()
        .unwrap();
    assert!(!stored.credentials.contains("refresh-secret"));
    assert!(!stored.access_token.as_deref().unwrap().contains("second"));
    assert_eq!(
        stored.token_expires_at,
        Some(start() + TimeDelta::seconds(660))
    );

    // An expired token is refreshed once and stored.
    h.advance(Duration::from_secs(600));
    let refreshed = integrations.token(hubspot).await.unwrap().expose().clone();
    assert_ne!(refreshed, "second");
    assert_eq!(
        integrations.token(hubspot).await.unwrap().expose(),
        &refreshed
    );
    assert_eq!(h.fake.minted().len(), 1);

    // Checks: a refusal turns a connection unhealthy, a success restores it.
    h.fake.refuse_tokens("microsoft", "unauthorized_client");
    let health = integrations.check(microsoft.id).await.unwrap();
    assert_eq!(health.status, ConnectionStatus::Unhealthy);
    let unhealthy = IntegrationConnection::query()
        .where_(IntegrationConnection::STATUS.eq("unhealthy"))
        .pluck(&*db, IntegrationConnection::ID)
        .await
        .unwrap();
    assert_eq!(unhealthy, [microsoft.id]);
    h.fake.restore_tokens("microsoft");
    let health = integrations.check(microsoft.id).await.unwrap();
    assert_eq!(health.status, ConnectionStatus::Healthy);
    let checked_at = start() + TimeDelta::seconds(660);
    assert_eq!(
        integrations
            .find(microsoft.id)
            .await
            .unwrap()
            .last_checked_at,
        Some(checked_at)
    );

    // Due checks: HubSpot was never checked; Microsoft is due an hour on.
    let summary = integrations.check_due().await.unwrap();
    assert_eq!((summary.checked, summary.healthy), (1, 1));
    assert_eq!(integrations.check_due().await.unwrap().checked, 0);
    h.advance(Duration::from_secs(61 * 60));
    assert_eq!(integrations.check_due().await.unwrap().checked, 2);

    // Every connection, by owner type, owner, provider and scope set.
    let all = integrations.all().await.unwrap();
    assert_eq!(
        all.iter().map(|info| info.id).collect::<Vec<_>>(),
        [microsoft.id, hubspot]
    );

    integrations.disconnect(hubspot).await.unwrap();
    assert!(integrations.find(hubspot).await.is_err());
    assert_eq!(integrations.forget(organization).await.unwrap(), 1);
    assert_eq!(IntegrationConnection::query().count(&*db).await.unwrap(), 0);
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn connections_work_on_postgres() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL", "postgres", false).await else {
        return;
    };
    flow(&scratch).await;
    scratch.drop().await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn connections_work_on_mysql() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_MYSQL_URL", "mysql", true).await else {
        return;
    };
    flow(&scratch).await;
    scratch.drop().await;
}
