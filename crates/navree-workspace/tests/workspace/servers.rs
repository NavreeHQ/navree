//! Shared libraries on PostgreSQL (with TimescaleDB) and MySQL: the
//! library columns and their `CHECK` constraint, the visibility scope's
//! organization subquery, the guard, clones and seeding on real servers.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL`, or with `--features mysql` and set
//! `NAVREE_TEST_MYSQL_URL`, to the URL of a throwaway server whose user may
//! create databases. Each test creates its own database and drops it at the
//! end; without the feature or the variable the test returns at once.

#[cfg(any(feature = "postgres", feature = "mysql"))]
mod real {
    use navree_orm::prelude::*;
    use navree_workspace::library::{
        Library, LibraryOwner, LibraryRepo, LibraryScope, LibraryViewer, LibraryVisibility,
    };
    use navree_db::{DbOptions, Executor, Pool};
    use serde_json::json;
    use uuid::Uuid;

    use crate::library::{Template, TemplateBlock, fixture_with, names, visible};
    use crate::support::org_builder;

    /// A database of its own on the test server.
    struct Scratch {
        admin: String,
        url: String,
        drop_sql: String,
    }

    impl Scratch {
        async fn new(variable: &str, drop_sql: fn(&str) -> String) -> Option<Scratch> {
            let admin = std::env::var(variable).ok()?;
            let name = format!("navree_library_{}", Uuid::now_v7().simple());
            run(&admin, &format!("CREATE DATABASE {name}")).await;
            let (base, rest) = admin.rsplit_once('/').unwrap();
            let options = rest.find('?').map_or("", |at| &rest[at..]);
            let url = format!("{base}/{name}{options}");
            Some(Scratch {
                drop_sql: drop_sql(&name),
                admin,
                url,
            })
        }

        async fn drop(self) {
            run(&self.admin, &self.drop_sql).await;
        }
    }

    /// Runs `sql` on the server of `admin`.
    async fn run(admin: &str, sql: &str) {
        let pool = Pool::connect(&DbOptions {
            url: admin.to_owned(),
            max_connections: 1,
            ..DbOptions::default()
        })
        .await
        .unwrap();
        pool.execute_script(sql).await.unwrap();
        pool.close().await;
    }

    fn raw(
        id: Uuid,
        scope: LibraryScope,
        organization: Option<Uuid>,
        workspace: Option<Uuid>,
    ) -> Template {
        Template {
            id,
            scope,
            organization_id: organization,
            workspace_id: workspace,
            slug: "raw".into(),
            name: "Raw".into(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            ..Template::default()
        }
    }

    async fn exercise(scratch: &Scratch) {
        let lib = fixture_with(org_builder().config("database.url", scratch.url.clone())).await;
        let db = lib.world.db().await;

        // Each viewer's library.
        assert_eq!(
            visible(&db, &lib.acme).await,
            ["Acme only", "Group handbook", "Welcome"]
        );
        assert_eq!(visible(&db, &lib.globex).await, ["Globex only", "Welcome"]);
        assert_eq!(
            visible(&db, LibraryViewer::organization(lib.other.id)).await,
            ["Other handbook", "Welcome"]
        );
        // The scope reads the current workspace's organization in SQL.
        let initech =
            navree_orm::workspace::scope(lib.initech.id, Template::query().get(db.as_ref()))
                .await
                .unwrap();
        assert_eq!(names(&initech), ["Group handbook", "Welcome"]);
        assert_eq!(
            names(&Template::query().get(db.as_ref()).await.unwrap()),
            ["Welcome"]
        );

        // The CHECK constraint, under the hooks (a plain insert).
        let (org, ws) = (lib.group.id, lib.acme.id);
        for (scope, organization, workspace) in [
            (LibraryScope::Global, None, Some(ws)),
            (LibraryScope::Organization, None, None),
            (LibraryScope::Workspace, Some(org), Some(ws)),
        ] {
            let refused = Template::insert(
                db.as_ref(),
                [raw(Uuid::now_v7(), scope, organization, workspace)],
            )
            .await;
            assert!(refused.is_err(), "{scope} {organization:?} {workspace:?}");
        }
        Template::insert(
            db.as_ref(),
            [raw(
                Uuid::now_v7(),
                LibraryScope::Organization,
                Some(org),
                None,
            )],
        )
        .await
        .unwrap();

        // The guard over HTTP, and a clone with its blocks and event.
        let mut events = lib.world.events(&["library.item_cloned"]).await;
        let client = lib.world.as_user(lib.ada);
        client
            .patch(&format!("/api/templates/{}", lib.handbook))
            .header("x-workspace", "acme")
            .json(&json!({ "name": "Mine" }))
            .send()
            .await
            .assert_status(403)
            .assert_json_path("code", json!("library_read_only"));
        let copy = client
            .post(&format!("/api/templates/{}/clone", lib.welcome))
            .header("x-workspace", "acme")
            .send()
            .await
            .assert_status(200)
            .json::<serde_json::Value>();
        let copy_id: Uuid = serde_json::from_value(copy["id"].clone()).unwrap();
        assert_eq!(copy["cloned_from_id"], json!(lib.welcome));
        assert_eq!(copy["workspace_id"], json!(lib.acme.id));
        assert_eq!(
            TemplateBlock::query()
                .where_(TemplateBlock::TEMPLATE_ID.eq(copy_id))
                .count(db.as_ref())
                .await
                .unwrap(),
            2
        );
        assert_eq!(events.names(), ["library.item_cloned"]);
        let copy = LibraryRepo::<Template>::owned_by(LibraryOwner::Workspace(lib.acme.id))
            .find_or_fail(db.as_ref(), copy_id)
            .await
            .unwrap();
        assert_eq!(copy.cloned_from().unwrap(), Some(lib.welcome));

        // Seeding global items twice.
        let items = |name: &str| {
            [("welcome", name), ("reset", "Reset")].map(|(slug, name)| Template {
                slug: slug.into(),
                name: name.into(),
                ..Template::default()
            })
        };
        let first = LibraryRepo::<Template>::seed_global(db.as_ref(), "slug", items("Welcome"))
            .await
            .unwrap();
        assert_eq!((first.created, first.updated, first.unchanged), (1, 0, 1));
        let again = LibraryRepo::<Template>::seed_global(db.as_ref(), "slug", items("Hello"))
            .await
            .unwrap();
        assert_eq!((again.created, again.updated, again.unchanged), (0, 1, 1));
        assert_eq!(
            Template::query()
                .without_global_scope::<LibraryVisibility>()
                .where_(Template::SLUG.eq("welcome"))
                .count(db.as_ref())
                .await
                .unwrap(),
            2,
            "the global welcome and Acme's clone"
        );
        lib.world.app.shutdown().await.unwrap();
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn libraries_on_postgres() {
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
    async fn libraries_on_mysql() {
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
