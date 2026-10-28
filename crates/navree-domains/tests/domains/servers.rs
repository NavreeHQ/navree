//! PostgreSQL (with TimescaleDB) and MySQL: the domains module on a real
//! server, so its migrations and every statement of its models run there:
//! adding (the unique host, the per-workspace count), verification and the
//! due query (optional booleans and timestamps that come back as
//! written), primary domains, certificates with sealed keys, challenges,
//! ACME accounts, reloading and removal.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL`, or `--features mysql` and
//! `NAVREE_TEST_MYSQL_URL`, to a URL of a throwaway server whose user may
//! create databases. Each test creates its own database and drops it at
//! the end.

#![cfg_attr(
    not(any(feature = "postgres", feature = "mysql")),
    allow(dead_code, unused_imports)
)]

use std::time::Duration;

use chrono::TimeDelta;
use navree_db::Db;
use navree_db::sea_orm::{ConnectionTrait, Database, DbBackend};
use navree_domains::{DomainStatus, DomainsError, NewDomain, TlsStatus, models};
use navree_orm::Model as _;
use serde_json::json;
use uuid::Uuid;

use crate::support::World;

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
                assert!(
                    std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                    "{test}: NAVREE_TEST_REQUIRE_SERVICES=1 but {var} is not set"
                );
                println!("SKIPPED {test}: set {var}");
                return None;
            }
        };
        let name = format!("navree_domains_{}", Uuid::now_v7().simple());
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

async fn flow(scratch: &Scratch) {
    let world = World::with(|builder| {
        builder
            .config("database.url", scratch.url.as_str())
            .config("database.migrate_on_boot", true)
            .config("domains.tls", "acme")
            .config("domains.acme.terms_agreed", true)
            .config("domains.max_per_workspace", 2)
            .config("workspace.resolvers", json!(["domain", "header"]))
    })
    .await;
    let db = world.app.get::<Db>().await;
    let expected = if scratch.mysql {
        DbBackend::MySql
    } else {
        DbBackend::Postgres
    };
    assert_eq!(db.backend(), expected);
    let domains = world.domains().await;
    let owner = Uuid::now_v7();
    let acme = world.workspace(owner, "Acme").await;
    let globex = world.workspace(Uuid::now_v7(), "Globex").await;

    // Adding: a claim of another workspace on the same host is no
    // obstacle, a workspace has one row per host, and the per-workspace
    // count.
    let crm = world.add(acme, "crm.acme-corp.test").await;
    assert_eq!(crm.created_at, world.now());
    let rival = domains
        .add(globex, NewDomain::new("crm.acme-corp.test"))
        .await
        .unwrap();
    assert!(matches!(
        domains
            .add(globex, NewDomain::new("crm.acme-corp.test"))
            .await,
        Err(DomainsError::Taken(_))
    ));
    domains.remove(rival.id).await.unwrap();
    let go = world.add(acme, "go.acme-corp.test").await;
    assert!(matches!(
        domains
            .add(acme, NewDomain::new("help.acme-corp.test"))
            .await,
        Err(DomainsError::LimitReached(2))
    ));

    // Verification: the due query, optional booleans, timestamps.
    world.publish(&crm);
    let checked = domains.verify_due().await.unwrap();
    assert_eq!(checked.len(), 2);
    let crm = domains.get(crm.id).await.unwrap();
    assert_eq!(crm.status, DomainStatus::Verified);
    assert_eq!(crm.points_to_app, Some(true));
    assert_eq!(crm.verified_at, Some(world.now()));
    let go = domains.get(go.id).await.unwrap();
    assert_eq!(go.points_to_app, Some(false));
    assert_eq!(go.next_check_at, Some(world.now() + TimeDelta::minutes(5)));
    assert!(domains.verify_due().await.unwrap().is_empty());
    world.advance(Duration::from_secs(5 * 60));
    assert_eq!(domains.verify_due().await.unwrap().len(), 1);

    // Resolution, the cache-less lookup and the ask query.
    let resolved = domains
        .resolve("crm.acme-corp.test")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.workspace_id, acme);
    world
        .as_user(owner)
        .get("/api/domains")
        .header("host", "crm.acme-corp.test")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.host", json!("crm.acme-corp.test"));

    // Primary domains.
    domains.make_primary(crm.id).await.unwrap();
    assert_eq!(domains.primary(acme).await.unwrap().unwrap().id, crm.id);

    // Certificates, challenges and the ACME account.
    world.fakes.ca.keep_challenges();
    let issued = domains.issue_pending().await.unwrap();
    assert_eq!(issued, ["crm.acme-corp.test"]);
    let (_, token, answer) = world.fakes.ca.challenges().remove(0);
    assert_eq!(
        domains.challenge_answer(&token).await.unwrap(),
        Some(answer)
    );
    assert_eq!(domains.get(crm.id).await.unwrap().tls, TlsStatus::Active);
    domains.certificates().remove("crm.acme-corp.test");
    assert_eq!(domains.reload_certificates().await.unwrap(), 1);
    let stored = models::Certificate::query()
        .where_(models::Certificate::HOST.eq("crm.acme-corp.test"))
        .first(&*db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.domain_id, Some(crm.id));

    // Removal.
    domains.remove(crm.id).await.unwrap();
    assert_eq!(domains.list(acme).await.unwrap().len(), 1);
    assert!(
        models::Certificate::query()
            .where_(models::Certificate::HOST.eq("crm.acme-corp.test"))
            .first(&*db)
            .await
            .unwrap()
            .is_none()
    );
    world.app.shutdown().await.unwrap();
}

/// Claims on a real server: the migration both ways, claims racing for
/// one host in separate connections (the unique index decides, nobody
/// gets a database error), the same through the routes (a request's unit
/// of work survives losing), and a transfer.
async fn claims(scratch: &Scratch) {
    let world = World::with(|builder| {
        builder
            .config("database.url", scratch.url.as_str())
            .config("database.migrate_on_boot", true)
            .config("domains.tls", "acme")
            .config("domains.acme.terms_agreed", true)
    })
    .await;
    crate::migration::roundtrip(&world).await;
    let domains = world.domains().await;

    // Four claims prove control at once, three times.
    for round in 0..3 {
        let host = format!("race{round}.acme-corp.test");
        let mut ids = Vec::new();
        for n in 0..4 {
            let workspace = world
                .workspace(Uuid::now_v7(), &format!("W{round}{n}"))
                .await;
            let claim = world.add(workspace, &host).await;
            world.publish(&claim);
            ids.push(claim.id);
        }
        let mut tasks = Vec::new();
        for id in &ids {
            let domains = domains.clone();
            let id = *id;
            tasks.push(tokio::spawn(async move { domains.verify(id).await }));
        }
        for task in tasks {
            task.await.unwrap().unwrap();
        }
        let mut verified = 0;
        for id in &ids {
            match domains.get(*id).await.unwrap().status {
                DomainStatus::Verified => verified += 1,
                other => assert_eq!(other, DomainStatus::Failed, "{host}"),
            }
        }
        assert_eq!(verified, 1, "{host}");
        assert!(domains.resolve(&host).await.unwrap().is_some());
    }

    // The same through the routes: each request has a unit of work.
    let (user_a, user_b) = (Uuid::now_v7(), Uuid::now_v7());
    let a = world.workspace(user_a, "RouteA").await;
    let b = world.workspace(user_b, "RouteB").await;
    let host = "routes.acme-corp.test";
    let claim_a = world.add(a, host).await;
    let claim_b = world.add(b, host).await;
    world.publish(&claim_a);
    world.publish(&claim_b);
    let (a, b) = (a.to_string(), b.to_string());
    let (client_a, client_b) = (world.as_user(user_a), world.as_user(user_b));
    let path_a = format!("/api/domains/{}/verify", claim_a.id);
    let path_b = format!("/api/domains/{}/verify", claim_b.id);
    let (first, second) = tokio::join!(
        client_a
            .post(&path_a)
            .header("x-workspace", a.as_str())
            .send(),
        client_b
            .post(&path_b)
            .header("x-workspace", b.as_str())
            .send()
    );
    let statuses: Vec<String> = [first, second]
        .into_iter()
        .map(|response| {
            let body = response.assert_status(200).json_value();
            body["status"].as_str().unwrap().to_owned()
        })
        .collect();
    assert!(
        statuses.contains(&"verified".to_owned()) && statuses.contains(&"failed".to_owned()),
        "{statuses:?}"
    );

    // A transfer needs the new owner's fresh token.
    let (old, new) = (
        world.workspace(Uuid::now_v7(), "Old").await,
        world.workspace(Uuid::now_v7(), "New").await,
    );
    let held = world.verified(old, "moved.acme-corp.test").await;
    domains.issue_pending().await.unwrap();
    let claim = domains.transfer("moved.acme-corp.test", new).await.unwrap();
    assert_eq!(claim.status, DomainStatus::Pending);
    assert_eq!(
        domains
            .resolve("moved.acme-corp.test")
            .await
            .unwrap()
            .unwrap()
            .id,
        held.id
    );
    world.publish(&claim);
    let moved = domains.transfer("moved.acme-corp.test", new).await.unwrap();
    assert_eq!(moved.status, DomainStatus::Verified);
    assert!(matches!(
        domains.get(held.id).await,
        Err(DomainsError::DomainNotFound)
    ));
    assert_eq!(
        domains
            .resolve("moved.acme-corp.test")
            .await
            .unwrap()
            .unwrap()
            .workspace_id,
        new
    );
    world.app.shutdown().await.unwrap();
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn domains_work_on_postgres() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL", "postgres", false).await else {
        return;
    };
    flow(&scratch).await;
    scratch.drop().await;
}

#[cfg(feature = "postgres")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn claims_work_on_postgres() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL", "postgres-claims", false).await
    else {
        return;
    };
    claims(&scratch).await;
    scratch.drop().await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn domains_work_on_mysql() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_MYSQL_URL", "mysql", true).await else {
        return;
    };
    flow(&scratch).await;
    scratch.drop().await;
}

#[cfg(feature = "mysql")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn claims_work_on_mysql() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_MYSQL_URL", "mysql-claims", true).await else {
        return;
    };
    claims(&scratch).await;
    scratch.drop().await;
}
