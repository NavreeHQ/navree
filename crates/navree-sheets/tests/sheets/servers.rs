//! PostgreSQL and MySQL: the import and export flow on a real server, so
//! the staging SQL (multi-row inserts, `IN` lists, `LIMIT ? OFFSET ?`,
//! `GROUP BY`, `IS NULL` lookups of saved mappings) runs on each.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL`, or `--features mysql` and
//! `NAVREE_TEST_MYSQL_URL`, to a URL of a throwaway server whose user may
//! create databases. Each test creates its own database and drops it at
//! the end.

use encoding_rs::WINDOWS_1256;
use navree_db::Db;
use sea_orm::{ConnectionTrait, Database, DbBackend};
use serde_json::json;
use uuid::Uuid;

use crate::support::{
    self, ADMIN, contact, csv_lines, id_of, principal, run_queue, seed, stored_contacts, upload,
};

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
        let name = format!("navree_sheets_{}", Uuid::now_v7().simple());
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

/// Upload, stage, preview, map, apply with updates, the error file, and an
/// export, on `scratch`.
async fn flow(scratch: &Scratch) {
    let app = support::base()
        .config("database.url", scratch.url.as_str())
        .config("database.migrate_on_boot", true)
        .config("sheets.imports.chunk_size", 2)
        .build()
        .await;
    let expected = if scratch.mysql {
        DbBackend::MySql
    } else {
        DbBackend::Postgres
    };
    assert_eq!(app.get::<Db>().await.backend(), expected);
    seed(&app, &[("ada@example.com", "Ada", Some(30), false)]).await;
    let admin = app.acting_as(principal(ADMIN));

    let text = "البريد الإلكتروني,الاسم,Age\n\
        ADA@example.com,آدا لوفليس,36\n\
        mohammed@example.com,محمد عبد الله,40\n\
        not-an-email,فاطمة الزهراء,31\n\
        mohammed@example.com,محمد,41\n\
        noura@example.com,نورة القحطاني,28\n";
    let (bytes, _, _) = WINDOWS_1256.encode(text);
    let uploaded = upload(&admin, "عملاء.csv", &bytes, &[])
        .await
        .assert_status(202)
        .json_value();
    let id = id_of(&uploaded);
    run_queue(&app).await;
    let record = admin
        .get(&format!("/api/imports/{id}"))
        .send()
        .await
        .json_value();
    assert_eq!(record["status"], "staged", "{record}");
    assert_eq!(record["encoding"], "windows-1256");
    assert_eq!(record["counts"]["valid"], 2);
    assert_eq!(record["counts"]["duplicate"], 1);
    assert_eq!(record["counts"]["invalid"], 1);
    assert_eq!(record["counts"]["repeated"], 1);
    let page = admin
        .get(&format!("/api/imports/{id}/rows?per_page=2&page=2"))
        .send()
        .await
        .json_value();
    assert_eq!(
        page["meta"],
        json!({ "current_page": 2, "per_page": 2, "total": 5, "last_page": 3 })
    );
    assert_eq!(page["data"][0]["row"], 4);

    admin
        .post(&format!("/api/imports/{id}/apply"))
        .json(&json!({ "duplicates": "update" }))
        .send()
        .await
        .assert_status(202);
    run_queue(&app).await;
    let record = admin
        .get(&format!("/api/imports/{id}"))
        .send()
        .await
        .json_value();
    assert_eq!(record["status"], "completed", "{record}");
    assert_eq!(record["counts"]["created"], 2);
    assert_eq!(record["counts"]["updated"], 1);
    let contacts = stored_contacts(&app).await;
    assert_eq!(
        contacts[0],
        contact("ada@example.com", "آدا لوفليس", Some(36), false)
    );
    assert_eq!(contacts.len(), 3);
    let errors = admin
        .get(&format!("/api/imports/{id}/errors"))
        .send()
        .await
        .assert_status(200);
    let lines = csv_lines(errors.bytes());
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[2].ends_with("Repeats row 3"), "{lines:?}");

    // A chosen, saved mapping is found again for the next file.
    let uploaded = upload(&admin, "x.csv", b"Mail,Who\nzed@example.com,Zed\n", &[])
        .await
        .json_value();
    run_queue(&app).await;
    let next = id_of(&uploaded);
    admin
        .put(&format!("/api/imports/{next}/mapping"))
        .json(&json!({ "mapping": { "email": "Mail", "name": "Who" }, "save": true }))
        .send()
        .await
        .assert_status(202);
    run_queue(&app).await;
    let again = upload(&admin, "y.csv", b"WHO,mail\nyan@example.com,Yan\n", &[])
        .await
        .json_value();
    run_queue(&app).await;
    let record = admin
        .get(&format!("/api/imports/{}", id_of(&again)))
        .send()
        .await
        .json_value();
    assert_eq!(record["status"], "staged", "{record}");

    // An export pages through the server.
    let export = admin
        .get("/api/exports/contacts?vip=false")
        .send()
        .await
        .assert_status(200);
    assert_eq!(csv_lines(export.bytes()).len(), 4);
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn imports_and_exports_run_on_postgres() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL", "postgres", false).await else {
        return;
    };
    flow(&scratch).await;
    scratch.drop().await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn imports_and_exports_run_on_mysql() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_MYSQL_URL", "mysql", true).await else {
        return;
    };
    flow(&scratch).await;
    scratch.drop().await;
}
