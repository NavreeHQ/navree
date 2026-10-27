//! PostgreSQL (with TimescaleDB) and MySQL: the directory on a real server,
//! so the migrations and every statement of the models run on each: SCIM
//! users and groups (case-insensitive user names, uniqueness, memberships,
//! deletes), sync runs (inserts and updates in chunks, booleans,
//! optional booleans and timestamps that come back as written, the due
//! lookup, held runs, recorded changes, pruning) and connection deletes.
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

use navree_db::Db;
use navree_db::sea_orm::{ConnectionTrait, Database, DbBackend};
use navree_directory::{
    ChangeStatus, DirectoryGroup, DirectoryUser, Owner, RunStatus, SyncOptions, models,
};
use navree_orm::Model as _;
use serde_json::json;
use uuid::Uuid;

use crate::support::{harness, start};

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
        let name = format!("navree_directory_{}", Uuid::now_v7().simple());
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

const USER: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
const PATCH: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";

async fn flow(scratch: &Scratch) {
    // Queued syncs stay queued: nothing runs behind the test's back.
    let queue = navree_queue::QueueFake::new();
    let dispatcher = queue.dispatcher();
    let h = harness(|b| {
        b.config("database.url", scratch.url.as_str())
            .config("database.migrate_on_boot", true)
            .config("directory.sync.chunk_size", 2)
            .override_value(dispatcher)
    })
    .await;
    let db = h.app.get::<Db>().await;
    let expected = if scratch.mysql {
        DbBackend::MySql
    } else {
        DbBackend::Postgres
    };
    assert_eq!(db.backend(), expected);
    let directory = h.directory().await;

    // -- SCIM ------------------------------------------------------------------
    let created = h.scim(Owner::Workspace(Uuid::now_v7())).await;
    assert_eq!(created.connection.created_at, start());
    let token = created.scim_token.unwrap().expose().clone();
    let client = h.app.client();
    let ada = client
        .post("/scim/v2/Users")
        .bearer(&token)
        .json(&json!({
            "schemas": [USER], "externalId": "e-ada", "userName": "Ada@Acme.test",
            "name": { "givenName": "Ada", "familyName": "Lovelace" },
            "emails": [{ "value": "ada@acme.test", "type": "work", "primary": true }]
        }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let ada_id = ada["id"].as_str().unwrap().to_owned();
    client
        .post("/scim/v2/Users")
        .bearer(&token)
        .json(&json!({ "schemas": [USER], "userName": "ada@ACME.test" }))
        .send()
        .await
        .assert_status(409);
    client
        .get("/scim/v2/Users?filter=userName%20eq%20%22ada%40acme.test%22")
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("totalResults", json!(1))
        .assert_json_path("Resources.0.id", json!(ada_id));
    let group = client
        .post("/scim/v2/Groups")
        .bearer(&token)
        .json(&json!({ "displayName": "Engineering", "members": [{ "value": ada_id }] }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    client
        .get("/scim/v2/Groups?filter=displayName%20eq%20%22ENGINEERING%22")
        .bearer(&token)
        .send()
        .await
        .assert_json_path("totalResults", json!(1))
        .assert_json_path("Resources.0.members.0.value", json!(ada_id));
    client
        .patch(&format!("/scim/v2/Users/{ada_id}"))
        .bearer(&token)
        .json(&json!({ "schemas": [PATCH], "Operations": [{ "op": "Replace", "path": "active", "value": "False" }] }))
        .send()
        .await
        .assert_status(200);
    let row = models::User::query()
        .where_(models::User::ID.eq(Uuid::parse_str(&ada_id).unwrap()))
        .first(&*db)
        .await
        .unwrap()
        .unwrap();
    assert!(!row.active);
    assert_eq!(row.deactivated_at, Some(start()));
    assert_eq!(row.local_id.as_deref(), Some("local-e-ada"));
    client
        .delete(&format!("/scim/v2/Users/{ada_id}"))
        .bearer(&token)
        .send()
        .await
        .assert_status(204);
    client
        .get(&format!(
            "/scim/v2/Groups/{}",
            group["id"].as_str().unwrap()
        ))
        .bearer(&token)
        .send()
        .await
        .assert_json_path("members", json!([]));

    // -- sync runs -------------------------------------------------------------
    let people: Vec<DirectoryUser> = (0..15)
        .map(|n| {
            DirectoryUser::new(format!("u{n}"), format!("p{n}@acme.test"))
                .name("Person", format!("{n}"))
                .email(format!("p{n}@acme.test"))
        })
        .collect();
    h.fake().users(people.clone()).group(
        DirectoryGroup::new("g1", "Everyone").members(people.iter().map(|p| p.external_id.clone())),
    );
    let id = h.connection("ldap").await;
    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded, "{run:?}");
    assert_eq!((run.done, run.total), (16, 16));
    assert_eq!(run.complete, Some(true));
    assert_eq!(run.started_at, Some(start()));
    assert_eq!(directory.users(id).await.unwrap().len(), 15);
    assert_eq!(
        directory.groups(id).await.unwrap()[0].group.members.len(),
        15
    );

    // A mistaken filter: held, then forced.
    h.fake().users(people[..2].to_vec());
    let held = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(held.status, RunStatus::Held);
    let planned = directory.changes(held.id, 0, 100).await.unwrap();
    assert!(planned.iter().all(|c| c.status == ChangeStatus::Planned));
    let forced = SyncOptions {
        force: true,
        ..SyncOptions::default()
    };
    let run = directory.sync_now(id, forced).await.unwrap();
    assert_eq!(run.summary.users_deactivated, 13);
    assert_eq!(run.summary.memberships_removed, 13);
    let runs = directory.runs(id, 10).await.unwrap();
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].id, run.id, "newest first");

    // Due: the hourly schedule.
    let connection = directory.connection(id).await.unwrap();
    assert!(connection.next_sync_at.is_some());
    h.clock.advance(Duration::from_secs(61 * 60));
    let due = directory.sync_due().await.unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].status, RunStatus::Queued);
    queue.assert_dispatched_times::<navree_directory::SyncDirectory>(1);

    h.clock.advance(Duration::from_secs(100 * 24 * 3600));
    assert!(directory.prune_runs().await.unwrap() >= 3);
    directory.delete_connection(id).await.unwrap();
    assert_eq!(
        models::Membership::query()
            .where_(models::Membership::CONNECTION_ID.eq(id))
            .count(&*db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        models::User::query()
            .where_(models::User::CONNECTION_ID.eq(id))
            .count(&*db)
            .await
            .unwrap(),
        0
    );
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn the_directory_works_on_postgres() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL", "postgres", false).await else {
        return;
    };
    flow(&scratch).await;
    scratch.drop().await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn the_directory_works_on_mysql() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_MYSQL_URL", "mysql", true).await else {
        return;
    };
    flow(&scratch).await;
    scratch.drop().await;
}
