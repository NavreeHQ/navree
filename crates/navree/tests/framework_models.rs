//! The framework's own tables as navree-orm models, on every database: the
//! audit trail, stored notifications and the auth tables are written by
//! their modules (through the ORM where the modules use it) and read back
//! as models.
//!
//! The SQLite test always runs. The PostgreSQL (TimescaleDB) and MySQL
//! ones run against real servers: build with `--features postgres,mysql`
//! and set `NAVREE_TEST_POSTGRES_URL` and `NAVREE_TEST_MYSQL_URL` to URLs
//! whose users may create databases (each test works in a database of its
//! own):
//!
//! ```text
//! NAVREE_TEST_POSTGRES_URL=postgres://postgres:secret@127.0.0.1:55432/postgres \
//! NAVREE_TEST_MYSQL_URL=mysql://root:secret@127.0.0.1:53306/test \
//! cargo test -p navree --features postgres,mysql --test framework_models
//! ```

use navree::audit::{AuditFilter, AuditLog, AuditRecord};
use navree::auth::{AuthService, ClientMeta, User, refresh_token};
use navree::contracts::{AuditEntry, AuditKind};
use navree::db::{Db, PageQuery};
use navree::notify::{Channel, Notifiable, Notification, Notifier, NotifyOptions, notifications};
use navree::orm::Model as _;
use navree::prelude::*;
use navree::testing::{TestApp, TestAppBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// A strong enough password.
const PASSWORD: &str = "correct horse battery";

/// An app with the modules whose tables are models, migrated at boot (the
/// database comes from the caller).
fn app() -> TestAppBuilder {
    TestApp::builder()
        .config("database.migrate_on_boot", true)
        .config("app.key", navree::crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("audit.routes", false)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuditModule::from_config())
        .module(AuthModule::from_config())
        .module(NotifyModule::for_root(NotifyOptions {
            routes: false,
            ..NotifyOptions::default()
        }))
}

/// A user receiving notifications.
struct Recipient(Uuid);

impl Notifiable for Recipient {
    fn notifiable_type(&self) -> &'static str {
        "user"
    }

    fn notifiable_id(&self) -> String {
        self.0.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        None
    }
}

/// A notification stored in the database.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct DealWon {
    title: String,
}

impl Notification for DealWon {
    const TYPE: &'static str = "deal_won";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database]
    }

    fn to_database(&self, _notifiable: &dyn Notifiable) -> Option<Value> {
        Some(json!({ "title": self.title }))
    }
}

/// Writes through each module and reads the rows back as models.
async fn models_round_trip(app: &TestApp) {
    let db = app.get::<Db>().await;

    // The audit trail: written by the log, listed through the ORM.
    let log = app.get::<AuditLog>().await;
    let actor = Uuid::now_v7();
    log.record(
        AuditEntry::new(AuditKind::Auth, "deal.exported")
            .actor(actor)
            .metadata(json!({ "rows": 42 })),
    )
    .await
    .unwrap();
    log.flush().await;
    let entry = AuditRecord::query()
        .where_(AuditRecord::ACTOR_ID.eq(actor))
        .first(&db)
        .await
        .unwrap()
        .expect("the audit entry");
    assert_eq!(entry.action, "deal.exported");
    assert_eq!(entry.metadata, json!({ "rows": 42 }));
    let filter = AuditFilter {
        action: Some("exported".to_owned()),
        ..AuditFilter::default()
    };
    let page = log.list(&filter, PageQuery::default()).await.unwrap();
    assert_eq!(page.meta.total, 1);
    assert_eq!(page.data[0].id, entry.id);

    // Stored notifications: written by the database channel.
    let notifier = app.get::<Notifier>().await;
    let recipient = Recipient(Uuid::now_v7());
    let id = notifier
        .send_now(
            &recipient,
            DealWon {
                title: "Acme".to_owned(),
            },
        )
        .await
        .unwrap();
    let stored = notifications::Model::find(&db, id)
        .await
        .unwrap()
        .expect("the stored notification");
    assert_eq!(stored.notification_type, "deal_won");
    assert_eq!(stored.data, json!({ "title": "Acme" }));
    assert_eq!(stored.notifiable_id, recipient.0.to_string());

    // The auth tables: a user, two sessions, one of them ended.
    let auth = app.get::<AuthService>().await;
    let ada = auth
        .create_user("Ada Lovelace", "ada@navree.test", PASSWORD)
        .await
        .unwrap();
    let meta = ClientMeta::default();
    let (_, first) = auth
        .login("ada@navree.test", PASSWORD, &meta)
        .await
        .unwrap();
    auth.login("ada@navree.test", PASSWORD, &meta)
        .await
        .unwrap();
    auth.logout(Some(&first.refresh_token), ada.id, false)
        .await
        .unwrap();
    let found = User::query()
        .where_(User::EMAIL.eq("ada@navree.test"))
        .first(&db)
        .await
        .unwrap()
        .expect("ada");
    assert_eq!((found.id, found.name.as_str()), (ada.id, "Ada Lovelace"));
    assert_eq!(found.created_at, ada.created_at);
    let live = refresh_token::Model::query()
        .where_(refresh_token::Model::USER_ID.eq(ada.id))
        .where_null(refresh_token::Model::REVOKED_AT)
        .count(&db)
        .await
        .unwrap();
    assert_eq!(live, 1);
    assert_eq!(auth.sessions(ada.id, None).await.unwrap().len(), 1);
    assert_eq!(
        auth.find_by_email("ADA@navree.test")
            .await
            .unwrap()
            .map(|user| user.id),
        Some(ada.id)
    );
}

#[tokio::test]
async fn framework_tables_are_models_on_sqlite() {
    let app = app().temp_database().build().await;
    models_round_trip(&app).await;
    app.shutdown().await.unwrap();
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
mod scratch {
    use std::sync::atomic::{AtomicU32, Ordering};

    use sea_orm::{ConnectionTrait, Database};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A database created for one test; [`drop`](Scratch::drop) removes it.
    pub struct Scratch {
        admin: String,
        name: String,
        pub url: String,
        mysql: bool,
    }

    impl Scratch {
        /// A fresh database on the server of `variable`, or `None` (with a
        /// notice) when it is unset.
        pub async fn new(variable: &str, mysql: bool) -> Option<Scratch> {
            let Ok(admin) = std::env::var(variable) else {
                eprintln!("{variable} is not set: skipping this real-server test");
                return None;
            };
            let name = format!(
                "navree_framework_models_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            );
            let conn = Database::connect(&admin).await.expect("connects");
            conn.execute_unprepared(&format!("CREATE DATABASE {name}"))
                .await
                .expect("the test user may create databases");
            conn.close().await.expect("closes");
            let (base, query) = match admin.split_once('?') {
                Some((base, query)) => (base.to_owned(), format!("?{query}")),
                None => (admin.clone(), String::new()),
            };
            let (server, _) = base.rsplit_once('/').expect("a URL with a database");
            let url = format!("{server}/{name}{query}");
            Some(Scratch {
                admin,
                name,
                url,
                mysql,
            })
        }

        /// Drops the database.
        pub async fn drop(self) {
            let conn = Database::connect(&self.admin).await.expect("connects");
            let sql = if self.mysql {
                format!("DROP DATABASE IF EXISTS {}", self.name)
            } else {
                format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name)
            };
            conn.execute_unprepared(&sql).await.expect("drops");
            conn.close().await.expect("closes");
        }
    }
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn framework_tables_are_models_on_postgres() {
    let Some(scratch) = scratch::Scratch::new("NAVREE_TEST_POSTGRES_URL", false).await else {
        return;
    };
    let app = app()
        .config("database.url", scratch.url.clone())
        .build()
        .await;
    models_round_trip(&app).await;
    app.shutdown().await.unwrap();
    scratch.drop().await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn framework_tables_are_models_on_mysql() {
    let Some(scratch) = scratch::Scratch::new("NAVREE_TEST_MYSQL_URL", true).await else {
        return;
    };
    let app = app()
        .config("database.url", scratch.url.clone())
        .build()
        .await;
    models_round_trip(&app).await;
    app.shutdown().await.unwrap();
    scratch.drop().await;
}
