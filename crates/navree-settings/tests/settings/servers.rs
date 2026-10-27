//! PostgreSQL and MySQL: the store contract, resolution and a write in a
//! unit of work on real servers.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL`, or with `--features mysql` and set
//! `NAVREE_TEST_MYSQL_URL`, to the URL of a throwaway server whose user may
//! create databases. Each test creates its own database and drops it at the
//! end; without the feature or the variable the test returns at once.

#[cfg(any(feature = "postgres", feature = "mysql"))]
mod real {
    use navree_settings::{DatabaseSettingsStore, Scope, Source};
    use sea_orm::{ConnectionTrait, Database};
    use serde_json::json;
    use uuid::Uuid;

    use crate::store::contract;
    use crate::support::{PortalSettings, World, builder};

    /// A database of its own on the test server.
    struct Scratch {
        admin: String,
        name: String,
        url: String,
        drop_sql: String,
    }

    impl Scratch {
        async fn new(variable: &str, drop_sql: fn(&str) -> String) -> Option<Scratch> {
            let admin = std::env::var(variable).ok()?;
            let name = format!("navree_settings_{}", Uuid::now_v7().simple());
            let conn = Database::connect(&admin).await.unwrap();
            conn.execute_unprepared(&format!("CREATE DATABASE {name}"))
                .await
                .unwrap();
            conn.close().await.unwrap();
            let (base, rest) = admin.rsplit_once('/').unwrap();
            let options = rest.find('?').map_or("", |at| &rest[at..]);
            let url = format!("{base}/{name}{options}");
            Some(Scratch {
                drop_sql: drop_sql(&name),
                admin,
                name,
                url,
            })
        }

        async fn drop(self) {
            let conn = Database::connect(&self.admin).await.unwrap();
            conn.execute_unprepared(&self.drop_sql).await.unwrap();
            let _ = self.name;
        }
    }

    async fn exercise(scratch: &Scratch) {
        let world = World::with(builder().config("database.url", scratch.url.clone())).await;
        contract(&DatabaseSettingsStore::new(world.db().await)).await;

        let settings = world.settings().await;
        let ada = world.user("Ada").await;
        let holding = world.organization(ada, "Holding").await;
        let acme = world.workspace(ada, "Acme").await;
        world.attach(holding.id, acme.id).await;
        for (scope, patch) in [
            (Scope::global(), json!({"page_size": 50})),
            (Scope::organization(holding.id), json!({"show_leaderboard": true})),
            (Scope::workspace(acme.id), json!({"welcome_text": "Hi Acme"})),
        ] {
            let settings = settings.clone();
            world
                .work(|uow| async move {
                    settings.update::<PortalSettings>(&uow, scope, patch).await
                })
                .await
                .unwrap();
        }
        let portal = settings
            .resolve::<PortalSettings>(Scope::workspace(acme.id))
            .await
            .unwrap();
        assert_eq!(portal.page_size, 50);
        assert_eq!(portal.source("show_leaderboard"), Some(Source::Organization));
        assert_eq!(portal.welcome_text, "Hi Acme");
        assert_eq!(portal.overridden, ["welcome_text"]);
        world.app.shutdown().await.unwrap();
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn postgres() {
        let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL", |name| {
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")
        })
        .await
        else {
            return;
        };
        exercise(&scratch).await;
        scratch.drop().await;
    }

    #[cfg(feature = "mysql")]
    #[tokio::test]
    async fn mysql() {
        let Some(scratch) = Scratch::new("NAVREE_TEST_MYSQL_URL", |name| {
            format!("DROP DATABASE IF EXISTS {name}")
        })
        .await
        else {
            return;
        };
        exercise(&scratch).await;
        scratch.drop().await;
    }
}
