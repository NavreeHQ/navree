//! End-to-end behaviour of `navree-plugin-notices`: portable migration,
//! permission checks on routes and navigation links, Helm panel link,
//! expiry CLI command and daily IST schedule, template and locale overrides,
//! and namespaced stylesheet delivery.

#![allow(missing_docs)]

use std::fs;
use std::process::Command;

use navree::access::AccessService;
use navree::crypto::generate_key;
use navree::datetime::{Span, Timestamp};
use navree::db::Db;
use navree::helm::HelmModule;
use navree::i18n::I18nModule;
use navree::orm::{Model as _, OrmModule, Record};
use navree::prelude::*;
use navree::schedule::{RunStatus, ScheduleModule, Scheduler};
use navree::testing::{TestApp, principal, temp_sqlite_url};
use navree::ui::{Shell, UiModule};
use navree::view::ViewModule;
use navree::workspace::{Members, NewWorkspace, WorkspaceRole, Workspaces};
use navree_plugin_notices::{BOARD_SHEET, NoticesModule, notice};
use serde_json::{Value, json};
use uuid::Uuid;

mod servers {
    #![allow(dead_code)]

    use navree::db::{DbOptions, Executor, Pool};
    use navree::testing::TestAppBuilder;
    use uuid::Uuid;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Server {
        Sqlite,
        Postgres,
        MySql,
    }

    impl Server {
        pub fn current() -> Server {
            let server = match std::env::var("NAVREE_TEST_DATABASE").as_deref() {
                Err(_) | Ok("" | "sqlite") => return Server::Sqlite,
                Ok("postgres") => Server::Postgres,
                Ok("mysql") => Server::MySql,
                Ok(other) => {
                    panic!("NAVREE_TEST_DATABASE: `{other}` is not sqlite, postgres or mysql")
                }
            };
            let compiled = match server {
                Server::Postgres => cfg!(feature = "test-postgres"),
                _ => cfg!(feature = "test-mysql"),
            };
            assert!(
                compiled,
                "NAVREE_TEST_DATABASE={server:?} needs the `{}` feature",
                server.feature()
            );
            server
        }

        fn feature(self) -> &'static str {
            match self {
                Server::Sqlite => "sqlite",
                Server::Postgres => "postgres",
                Server::MySql => "mysql",
            }
        }

        fn variable(self) -> &'static str {
            match self {
                Server::Postgres => "NAVREE_TEST_POSTGRES_URL",
                _ => "NAVREE_TEST_MYSQL_URL",
            }
        }
    }

    pub fn database(builder: TestAppBuilder) -> TestAppBuilder {
        let server = Server::current();
        if server == Server::Sqlite {
            return builder.temp_database();
        }
        let database = ServerDatabase::create(server);
        builder
            .config("database.url", database.url.clone())
            .config("database.migrate_on_boot", true)
            .config("database.max_connections", 4)
            .hold(database)
    }

    pub struct ServerDatabase {
        server: Server,
        admin: String,
        name: String,
        pub url: String,
    }

    impl ServerDatabase {
        pub fn create(server: Server) -> ServerDatabase {
            let admin = std::env::var(server.variable())
                .ok()
                .filter(|url| !url.is_empty())
                .unwrap_or_else(|| panic!("NAVREE_TEST_DATABASE needs {}", server.variable()));
            let name = format!("navree_notices_{}", Uuid::now_v7().simple());
            let base = admin.split('?').next().unwrap_or(&admin);
            let (base, _) = base.rsplit_once('/').expect("a database URL");
            let url = format!("{base}/{name}");
            run_admin(&admin, format!("CREATE DATABASE {name}"))
                .unwrap_or_else(|error| panic!("{error}"));
            ServerDatabase {
                server,
                admin,
                name,
                url,
            }
        }
    }

    impl Drop for ServerDatabase {
        fn drop(&mut self) {
            let force = if self.server == Server::Postgres {
                " WITH (FORCE)"
            } else {
                ""
            };
            let sql = format!("DROP DATABASE IF EXISTS {}{force}", self.name);
            if let Err(error) = run_admin(&self.admin, sql) {
                eprintln!("could not drop a test database: {error}");
            }
        }
    }

    fn run_admin(admin: &str, sql: String) -> std::result::Result<(), String> {
        let admin = admin.to_owned();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(async move {
                let pool = Pool::connect(&DbOptions {
                    url: admin,
                    max_connections: 1,
                    ..DbOptions::default()
                })
                .await
                .map_err(|error| format!("cannot connect to the test server: {error}"))?;
                let result = pool.execute_script(&sql).await;
                pool.close().await;
                result.map(drop).map_err(|error| format!("{sql}: {error}"))
            })
        })
        .join()
        .map_err(|_| "the admin thread panicked".to_owned())?
    }
}

/// App module providing the society shell with contributed sidebar links enabled.
struct SocietyShell;

impl Module for SocietyShell {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global();
        m.value(Shell::new("Model Town Society").contributed())
            .exports::<Shell>();
    }
}

/// Harpreet Sandhu owns "Ludhiana office"; Meera Sharma is a member and
/// Simran Kaur is a viewer; Kuldeep Gill belongs to no workspace.
struct People {
    harpreet: Uuid,
    meera: Uuid,
    simran: Uuid,
    kuldeep: Uuid,
    office: Uuid,
}

async fn people(app: &TestApp) -> People {
    let (harpreet, meera, simran, kuldeep) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let office = app
        .get::<Workspaces>()
        .await
        .create(&uow, harpreet, NewWorkspace::named("Ludhiana office"))
        .await
        .unwrap();
    let members = app.get::<Members>().await;
    members
        .add(&uow, office.id, meera, WorkspaceRole::Member)
        .await
        .unwrap();
    members
        .add(&uow, office.id, simran, WorkspaceRole::Viewer)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    People {
        harpreet,
        meera,
        simran,
        kuldeep,
        office: office.id,
    }
}

async fn society_app(roles: Value) -> TestApp {
    servers::database(TestApp::builder())
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.users_table", "")
        .config("workspace.resolvers", json!(["header"]))
        .config("workspace.roles", roles)
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(SocietyShell)
        .module(NoticesModule::from_config())
        .build()
        .await
}

const CLI_URL_ENV: &str = "NOTICES_BEHAVIOUR_DB_URL";
const CLI_ARGS_ENV: &str = "NOTICES_BEHAVIOUR_CLI_ARGS";

fn maybe_run_cli_child() -> bool {
    let Ok(url) = std::env::var(CLI_URL_ENV) else {
        return false;
    };
    let encoded = std::env::var(CLI_ARGS_ENV).expect("cli args");
    let args: Vec<String> = serde_json::from_str(&encoded).expect("decode cli args");
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let sources = ConfigSources::new()
            .set("database.url", url)
            .set("database.migrate_on_boot", true);
        runtime
            .block_on(
                Navree::app()
                    .module(ConfigModule::for_root(sources))
                    .module(DatabaseModule::from_config())
                    .module(OrmModule)
                    .module(AccessModule::for_root(AccessOptions::default()))
                    .module(NoticesModule::from_config())
                    .run_with_args(args),
            )
            .expect("cli command succeeded");
    })
    .join()
    .expect("cli child thread");
    true
}

fn run_cli_against_sqlite(test_name: &str, url: &str, args: &[&str]) -> String {
    let encoded = serde_json::to_string(&args).expect("encode cli args");
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(CLI_URL_ENV, url)
        .env(CLI_ARGS_ENV, encoded)
        .output()
        .expect("spawn cli child");
    assert!(
        output.status.success(),
        "cli {args:?} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}

#[tokio::test]
async fn its_table_is_created_by_its_migration_and_a_notice_round_trips() {
    let app = servers::database(TestApp::builder())
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(NoticesModule::from_config())
        .build()
        .await;

    let posted_at = Timestamp::utc(2026, 11, 15, 6, 0, 0).unwrap();
    let expires_at = posted_at + Span::days(30);
    let harpreet = Uuid::now_v7();

    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let mut saved = Record::new(notice::Model {
        id: 0,
        title: "Water supply off on Sunday".to_owned(),
        body: "Tank cleaning at Model Town, Ludhiana, from 6 am to 10 am IST.".to_owned(),
        posted_by: harpreet,
        expires_at,
        created_at: posted_at,
    });
    saved.save(&uow).await.expect("insert notice");
    uow.commit().await.expect("commit notice");
    assert!(saved.id > 0);

    let uow = db.uow(None, None).await;
    let loaded = notice::Model::find(&uow, saved.id)
        .await
        .expect("query notice")
        .expect("notice row exists");
    assert_eq!(loaded.title, "Water supply off on Sunday");
    assert_eq!(
        loaded.body,
        "Tank cleaning at Model Town, Ludhiana, from 6 am to 10 am IST."
    );
    assert_eq!(loaded.posted_by, harpreet);
    assert_eq!(loaded.expires_at, expires_at);
    drop(uow);

    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn posting_needs_notices_publish_and_reading_needs_notices_read() {
    let app = society_app(json!({
        "member": { "permissions": ["notices.read"] },
        "admin": { "permissions": ["notices.read", "notices.publish"] }
    }))
    .await;
    let them = people(&app).await;
    let office_header = them.office.to_string();

    let payload = json!({
        "title": "Water supply off on Sunday",
        "body": "Tank cleaning at Model Town, Ludhiana, from 6 am to 10 am IST."
    });

    // Harpreet Sandhu (owner, holds every workspace permission including notices.publish) posts.
    let created = app
        .acting_as(principal(them.harpreet))
        .post("/api/notices")
        .header("x-workspace", &office_header)
        .json(&payload)
        .send()
        .await
        .assert_status(201);
    let created_row: notice::Model = created.json();
    assert_eq!(created_row.title, "Water supply off on Sunday");
    assert_eq!(created_row.posted_by, them.harpreet);

    // Validation rejects an empty title with 400.
    app.acting_as(principal(them.harpreet))
        .post("/api/notices")
        .header("x-workspace", &office_header)
        .json(&json!({ "title": "", "body": "Empty title" }))
        .send()
        .await
        .assert_status(400);

    // Meera Sharma (member, holds only notices.read) lists the notice and is refused POST with 403.
    let listed: Vec<notice::Model> = app
        .acting_as(principal(them.meera))
        .get("/api/notices")
        .header("x-workspace", &office_header)
        .send()
        .await
        .assert_status(200)
        .json();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "Water supply off on Sunday");

    app.acting_as(principal(them.meera))
        .post("/api/notices")
        .header("x-workspace", &office_header)
        .json(&payload)
        .send()
        .await
        .assert_status(403);

    // Kuldeep Gill (in no workspace) is refused both reading and posting.
    app.acting_as(principal(them.kuldeep))
        .get("/api/notices")
        .header("x-workspace", &office_header)
        .send()
        .await
        .assert_status(403);
    app.acting_as(principal(them.kuldeep))
        .post("/api/notices")
        .header("x-workspace", &office_header)
        .json(&payload)
        .send()
        .await
        .assert_status(403);

    // Harpreet can also delete the notice.
    app.acting_as(principal(them.harpreet))
        .delete(&format!("/api/notices/{}", created_row.id))
        .header("x-workspace", &office_header)
        .send()
        .await
        .assert_status(204);

    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn describing_permissions_grants_nothing() {
    if maybe_run_cli_child() {
        return;
    }

    // Without granting `notices.read` to `member`, Meera gets 403 on `GET /notices`.
    let ungranted = society_app(json!({})).await;
    let them = people(&ungranted).await;
    ungranted
        .acting_as(principal(them.meera))
        .get("/notices")
        .header("accept", "text/html")
        .header("x-workspace", them.office.to_string())
        .send()
        .await
        .assert_status(403);
    ungranted.shutdown().await.unwrap();

    // Once the app grants `notices.read` to `member`, Meera gets 200 on `GET /notices`.
    let granted = society_app(json!({
        "member": { "permissions": ["notices.read"] }
    }))
    .await;
    let them = people(&granted).await;
    granted
        .acting_as(principal(them.meera))
        .get("/notices")
        .header("accept", "text/html")
        .header("x-workspace", them.office.to_string())
        .send()
        .await
        .assert_status(200);
    granted.shutdown().await.unwrap();

    // `access:explain` prints the permission description registered with `m.permissions`.
    let (sqlite_url, _guard) = temp_sqlite_url();
    let user = Uuid::now_v7();
    let explained = run_cli_against_sqlite(
        "describing_permissions_grants_nothing",
        &sqlite_url,
        &["access:explain", &user.to_string(), "read", "notices"],
    );
    assert!(
        explained.contains("Permission `notices.read`: Read the society's notices"),
        "{explained}"
    );
}

#[tokio::test]
async fn the_link_shows_to_members_who_hold_notices_read() {
    let app = society_app(json!({
        "member": { "permissions": ["notices.read"] }
    }))
    .await;
    let them = people(&app).await;

    let member_nav: Value = app
        .acting_as(principal(them.meera))
        .get("/_navree/ui/navigation")
        .header("x-workspace", them.office.to_string())
        .send()
        .await
        .assert_status(200)
        .json();
    assert_eq!(
        member_nav,
        json!({
            "items": [
                {
                    "label": "Notices",
                    "href": "/notices",
                    "group": "Society",
                    "order": 0,
                    "icon": null,
                }
            ]
        })
    );

    // Neither a guest nor Simran Kaur (viewer) nor Kuldeep Gill sees the link.
    let guest_nav: Value = app
        .client()
        .get("/_navree/ui/navigation")
        .send()
        .await
        .assert_status(200)
        .json();
    assert_eq!(guest_nav, json!({ "items": [] }));

    for person in [them.simran, them.kuldeep] {
        let nav: Value = app
            .acting_as(principal(person))
            .get("/_navree/ui/navigation")
            .header("x-workspace", them.office.to_string())
            .send()
            .await
            .assert_status(200)
            .json();
        assert_eq!(nav, json!({ "items": [] }));
    }

    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn helm_shows_its_link_to_who_may_see_it() {
    let harpreet = Uuid::now_v7();
    let meera = Uuid::now_v7();

    let app = servers::database(TestApp::builder())
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(AccessModule::for_feature::<notice::Model>())
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(HelmModule::from_config())
        .module(NoticesModule::from_config())
        .build()
        .await;

    let access = app.get::<AccessService>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    access.create_role(&uow, "staff", None).await.unwrap();
    access.grant(&uow, "staff", "helm.view").await.unwrap();
    access.create_role(&uow, "moderator", None).await.unwrap();
    access.grant(&uow, "moderator", "helm.view").await.unwrap();
    access.grant(&uow, "moderator", "notices.*").await.unwrap();
    access
        .assign_role(&uow, harpreet, "moderator")
        .await
        .unwrap();
    access.assign_role(&uow, meera, "staff").await.unwrap();
    uow.commit().await.unwrap();

    let admin_html = app
        .acting_as(principal(harpreet))
        .get("/admin")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(
        admin_html.contains(r#"href="/admin/notices""#),
        "Harpreet should see the Helm link:\n{admin_html}"
    );

    let viewer_html = app
        .acting_as(principal(meera))
        .get("/admin")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(
        !viewer_html.contains(r#"href="/admin/notices""#),
        "Meera without manage on notices.Notice should not see the Helm link:\n{viewer_html}"
    );

    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_expire_command_removes_expired_notices() {
    if maybe_run_cli_child() {
        return;
    }

    let (sqlite_url, _guard) = temp_sqlite_url();
    let app = TestApp::builder()
        .config("database.url", sqlite_url.clone())
        .config("database.migrate_on_boot", true)
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(NoticesModule::from_config())
        .build()
        .await;

    let now = Timestamp::now();
    let harpreet = Uuid::now_v7();
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;

    let mut old_notice = Record::new(notice::Model {
        id: 0,
        title: "Old generator servicing".to_owned(),
        body: "Completed last month in Model Town, Ludhiana.".to_owned(),
        posted_by: harpreet,
        expires_at: now - Span::days(5),
        created_at: now - Span::days(35),
    });
    old_notice.save(&uow).await.unwrap();

    let mut fresh_notice = Record::new(notice::Model {
        id: 0,
        title: "Water supply off on Sunday".to_owned(),
        body: "Tank cleaning at Model Town, Ludhiana, from 6 am to 10 am IST.".to_owned(),
        posted_by: harpreet,
        expires_at: now + Span::days(29),
        created_at: now - Span::days(1),
    });
    fresh_notice.save(&uow).await.unwrap();
    uow.commit().await.unwrap();
    app.shutdown().await.unwrap();

    let stdout = run_cli_against_sqlite(
        "the_expire_command_removes_expired_notices",
        &sqlite_url,
        &["notices:expire"],
    );
    assert!(
        stdout.lines().any(|line| line == "Removed 1 notices."),
        "{stdout}"
    );

    let verify = TestApp::builder()
        .config("database.url", sqlite_url)
        .config("database.migrate_on_boot", false)
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(NoticesModule::from_config())
        .build()
        .await;
    let db = verify.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let remaining = notice::Model::query().get(&uow).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].title, "Water supply off on Sunday");
    drop(uow);
    verify.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_expiry_is_scheduled_daily_at_two_thirty_ist() {
    let app = servers::database(TestApp::builder())
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(ScheduleModule::for_root())
        .module(NoticesModule::from_config())
        .build()
        .await;

    let scheduler = app.get::<Scheduler>().await;
    // 2026-11-15 20:00 UTC is 2026-11-16 01:30 IST; the next 02:30 IST is 2026-11-15 21:00 UTC.
    let before = Timestamp::utc(2026, 11, 15, 20, 0, 0).unwrap();
    let due_utc = Timestamp::utc(2026, 11, 15, 21, 0, 0).unwrap();

    let tasks = scheduler.list(before);
    let expire = tasks
        .iter()
        .find(|task| task.name == "notices.expire")
        .expect("notices.expire scheduled");
    assert_eq!(expire.expr, "30 2 * * *");
    assert_eq!(expire.zone, "Asia/Kolkata");
    assert_eq!(expire.next_run, Some(due_utc));

    // Not due one minute before 02:30 IST.
    let outcomes = scheduler.tick(due_utc - Span::minutes(1)).await.unwrap();
    assert!(outcomes.is_empty());

    // Fires cleanly at 02:30 IST (21:00 UTC).
    let outcomes = scheduler.tick(due_utc).await.unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].task, "notices.expire");
    assert_eq!(outcomes[0].status, RunStatus::Succeeded);

    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn its_page_renders_from_the_embedded_template_and_a_template_on_disk_wins() {
    let harpreet = Uuid::now_v7();
    let grant_all = || AccessOptions {
        abilities: |_, rules| {
            rules.can(MANAGE, ALL);
        },
        super_admin_role: None,
    };

    // Embedded template renders the notice and the "Notices" heading.
    let embedded_app = servers::database(TestApp::builder())
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(AccessModule::for_root(grant_all()))
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(SocietyShell)
        .module(NoticesModule::from_config())
        .build()
        .await;

    embedded_app
        .acting_as(principal(harpreet))
        .post("/api/notices")
        .json(&json!({
            "title": "Water supply off on Sunday",
            "body": "Tank cleaning at Model Town, Ludhiana, from 6 am to 10 am IST."
        }))
        .send()
        .await
        .assert_status(201);

    let html = embedded_app
        .acting_as(principal(harpreet))
        .get("/notices")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(html.contains("Notices"), "{html}");
    assert!(html.contains("Water supply off on Sunday"), "{html}");
    assert!(
        html.contains("Tank cleaning at Model Town, Ludhiana, from 6 am to 10 am IST."),
        "{html}"
    );
    embedded_app.shutdown().await.unwrap();

    // A file at `resources/views/notices/list.html` in the app's view directory wins.
    let dir = tempfile::tempdir().unwrap();
    let views_dir = dir.path().join("resources/views");
    fs::create_dir_all(views_dir.join("notices")).unwrap();
    fs::write(
        views_dir.join("notices/list.html"),
        "<div class=\"custom-board\">Model Town board ({{ notices | length }})</div>",
    )
    .unwrap();

    let override_app = servers::database(TestApp::builder())
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("view.dirs", json!([views_dir]))
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(AccessModule::for_root(grant_all()))
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(SocietyShell)
        .module(NoticesModule::from_config())
        .build()
        .await;

    let overridden = override_app
        .acting_as(principal(harpreet))
        .get("/notices")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(
        overridden.contains("<div class=\"custom-board\">Model Town board (0)</div>"),
        "{overridden}"
    );
    override_app.shutdown().await.unwrap();
}

#[tokio::test]
async fn its_messages_follow_the_language_of_the_page_and_the_apps_wins() {
    let meera = Uuid::now_v7();
    let grant_read = || AccessOptions {
        abilities: |_, rules| {
            rules.can("read", "notices");
        },
        super_admin_role: None,
    };

    let empty_locales = tempfile::tempdir().unwrap();
    let plugin_locales_app = servers::database(TestApp::builder())
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("i18n.locales", json!(["en", "pa-IN"]))
        .config("i18n.dir", json!(empty_locales.path()))
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(AccessModule::for_root(grant_read()))
        .module(I18nModule::from_config())
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(SocietyShell)
        .module(NoticesModule::from_config())
        .build()
        .await;

    let punjabi = plugin_locales_app
        .acting_as(principal(meera))
        .get("/notices")
        .header("accept", "text/html")
        .header("accept-language", "pa-IN")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(punjabi.contains("ਸੂਚਨਾਵਾਂ"), "{punjabi}");
    assert!(punjabi.contains("ਹਾਲੇ ਕੋਈ ਸੂਚਨਾ ਨਹੀਂ ਲਗਾਈ ਗਈ।"), "{punjabi}");
    plugin_locales_app.shutdown().await.unwrap();

    // An app message `notices-heading` in the app's `locales` folder wins over the plugin's.
    let app_locales = tempfile::tempdir().unwrap();
    fs::create_dir_all(app_locales.path().join("pa-IN")).unwrap();
    fs::write(
        app_locales.path().join("pa-IN/app.ftl"),
        "notices-heading = ਲੁਧਿਆਣਾ ਸੂਚਨਾ ਬੋਰਡ\n",
    )
    .unwrap();

    let override_locales_app = servers::database(TestApp::builder())
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("i18n.locales", json!(["en", "pa-IN"]))
        .config("i18n.dir", json!(app_locales.path()))
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(AccessModule::for_root(grant_read()))
        .module(I18nModule::from_config())
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(SocietyShell)
        .module(NoticesModule::from_config())
        .build()
        .await;

    let app_punjabi = override_locales_app
        .acting_as(principal(meera))
        .get("/notices")
        .header("accept", "text/html")
        .header("accept-language", "pa-IN")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(app_punjabi.contains("ਲੁਧਿਆਣਾ ਸੂਚਨਾ ਬੋਰਡ"), "{app_punjabi}");
    // The un-overridden message still comes from the plugin's `pa-IN` bundle.
    assert!(
        app_punjabi.contains("ਹਾਲੇ ਕੋਈ ਸੂਚਨਾ ਨਹੀਂ ਲਗਾਈ ਗਈ।"),
        "{app_punjabi}"
    );
    override_locales_app.shutdown().await.unwrap();
}

#[tokio::test]
async fn its_sheet_is_served_under_its_namespace() {
    let app = servers::database(TestApp::builder())
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(NoticesModule::from_config())
        .build()
        .await;

    let sheet_url = BOARD_SHEET.url();
    assert!(
        sheet_url.starts_with("/_navree/ui/app/notices.board-") && sheet_url.ends_with(".css"),
        "{sheet_url}"
    );
    let response = app.client().get(&sheet_url).send().await.assert_status(200);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/css"),
        "expected text/css, got `{content_type}`"
    );
    assert!(response.text().contains(".notice-board"));

    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn setting_notices_enabled_false_disables_routes_and_expiry() {
    let grant_all = AccessOptions {
        abilities: |_, rules| {
            rules.can(MANAGE, ALL);
        },
        super_admin_role: None,
    };

    let app = servers::database(TestApp::builder())
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("notices.enabled", false)
        .config("notices.keep_days", 7)
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(AccessModule::for_root(grant_all))
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(SocietyShell)
        .module(NoticesModule::from_config())
        .build()
        .await;

    let simran = Uuid::now_v7();
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let old = Timestamp::utc(2026, 10, 1, 6, 0, 0).unwrap();
    let mut row = Record::new(notice::Model {
        id: 0,
        title: "Lift maintenance in Block C".to_owned(),
        body: "Scheduled inspection at Model Town, Ludhiana.".to_owned(),
        posted_by: simran,
        expires_at: old + Span::days(1),
        created_at: old,
    });
    row.save(&uow).await.expect("insert expired notice");
    uow.commit().await.expect("commit notice");

    // Every page and API route returns 404 when `notices.enabled = false`.
    app.acting_as(principal(simran))
        .get("/notices")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(404);
    app.acting_as(principal(simran))
        .get("/api/notices")
        .send()
        .await
        .assert_status(404);
    app.acting_as(principal(simran))
        .post("/api/notices")
        .json(&json!({
            "title": "Water supply off on Sunday",
            "body": "Tank cleaning at Model Town, Ludhiana, from 6 am to 10 am IST."
        }))
        .send()
        .await
        .assert_status(404);
    app.acting_as(principal(simran))
        .delete(&format!("/api/notices/{}", row.id))
        .send()
        .await
        .assert_status(404);

    // `expire_notices` does nothing while `notices.enabled = false`, leaving existing rows intact.
    let removed = navree_plugin_notices::expire_notices(app.ctx())
        .await
        .expect("expire_notices succeeds when disabled");
    assert_eq!(removed, 0);

    let uow = db.uow(None, None).await;
    assert!(
        notice::Model::find(&uow, row.id)
            .await
            .expect("query notice")
            .is_some()
    );
    drop(uow);

    app.shutdown().await.unwrap();
}
