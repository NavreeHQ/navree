//! The table can't reach past the scope it's given: a workspace-scoped model through
//! the ORM, and row-level security on PostgreSQL with a role that isn't a superuser.
//! Reads, the bulk resolve and the export all start from the scoped query.

use navree::db::Executor as _;
use navree::db::{Db, DbOptions};
use navree::orm::Model;
use navree::ui::table::{NoFilter, Total};
use navree::ui::{BulkRequest, TableQuery, TableRow};
use uuid::Uuid;

#[derive(Model, Clone, Debug, Default, serde::Serialize, TableRow)]
#[model(table = "dt_private", workspace)]
#[table(sort = "title", per = 10, per_pages(10))]
pub struct Private {
    #[col(skip)]
    pub id: i64,
    #[col(skip)]
    pub workspace_id: Uuid,
    #[col(sortable, search, filter = text)]
    pub title: String,
}

fn own_database(url: &str, name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("a URL with a database");
    format!("{base}/{name}")
}

fn as_role(url: &str, role: &str) -> String {
    let rest = url.split_once("://").expect("a scheme").1;
    let host = rest.split_once('@').expect("a user").1;
    format!("postgres://{role}:dt_app_pw@{host}")
}

fn rows(table: &navree::ui::TablePage<Private>) -> Vec<String> {
    table.rows.iter().map(|row| row.title.clone()).collect()
}

#[tokio::test]
async fn workspaces_and_row_level_security_hold_on_every_path() {
    let Ok(url) = std::env::var("NAVREE_TEST_POSTGRES_URL") else {
        assert!(
            std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
            "NAVREE_TEST_POSTGRES_URL is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
        );
        eprintln!("skipped: NAVREE_TEST_POSTGRES_URL isn't set");
        return;
    };
    let admin = Db::connect(&DbOptions {
        url: url.clone(),
        ..DbOptions::default()
    })
    .await
    .unwrap();
    let _ = admin
        .pool()
        .execute_script("DROP DATABASE IF EXISTS ui_dt_scoping WITH (FORCE)")
        .await;
    admin
        .pool()
        .execute_script("CREATE DATABASE ui_dt_scoping")
        .await
        .unwrap();
    let own = own_database(&url, "ui_dt_scoping");
    let owner = Db::connect(&DbOptions {
        url: own.clone(),
        ..DbOptions::default()
    })
    .await
    .unwrap();
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    for statement in [
        "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'dt_app') THEN CREATE ROLE dt_app LOGIN PASSWORD 'dt_app_pw' NOSUPERUSER NOBYPASSRLS; END IF; END $$".to_owned(),
        "CREATE TABLE dt_private (id BIGINT PRIMARY KEY, workspace_id UUID NOT NULL, title TEXT NOT NULL)".to_owned(),
        format!("INSERT INTO dt_private SELECT n, CASE WHEN n % 2 = 0 THEN '{a}'::uuid ELSE '{b}'::uuid END, 'Row ' || n FROM generate_series(1, 40) n"),
        "ALTER TABLE dt_private ENABLE ROW LEVEL SECURITY".to_owned(),
        "ALTER TABLE dt_private FORCE ROW LEVEL SECURITY".to_owned(),
        "CREATE POLICY dt_private_scope ON dt_private USING (workspace_id = NULLIF(current_setting('navree.workspace', true), '')::uuid)".to_owned(),
        "GRANT SELECT, UPDATE, DELETE ON dt_private TO dt_app".to_owned(),
    ] {
        owner.pool().execute_script(&statement).await.unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
    let db = Db::connect(&DbOptions {
        url: as_role(&own, "dt_app"),
        ..DbOptions::default()
    })
    .await
    .unwrap();

    // With no workspace entered and row-level security on: nothing, not everything.
    let query = TableQuery::<Private, NoFilter>::parse("/p", "per=10");
    let page = query
        .load(Private::query().without_workspace_scope(), &db)
        .await
        .unwrap();
    assert!(
        page.rows.is_empty(),
        "no setting, no rows: {:?}",
        rows(&page)
    );

    // Workspace A, by the ORM's scope and by the policy.
    let uow = db.uow(None, None).await;
    uow.set_local("navree.workspace", &a.to_string())
        .await
        .unwrap();
    let page = query
        .load(Private::query().for_workspace(a), &uow)
        .await
        .unwrap();
    assert_eq!(page.total, Total::Exact(20));
    assert!(page.rows.iter().all(|row| row.workspace_id == a));
    // The policy holds even if the ORM's scope is opted out of.
    let page = query
        .load(Private::query().without_workspace_scope(), &uow)
        .await
        .unwrap();
    assert_eq!(page.total, Total::Exact(20), "row-level security alone");
    assert!(page.rows.iter().all(|row| row.workspace_id == a));

    // A filter or search can't step out: B's row 1 is invisible by name.
    let searched = TableQuery::<Private, NoFilter>::parse("/p", "q=Row+1&per=10");
    let page = searched
        .load(Private::query().for_workspace(a), &uow)
        .await
        .unwrap();
    assert!(
        page.rows.iter().all(|row| row.workspace_id == a),
        "{:?}",
        rows(&page)
    );
    assert!(!rows(&page).contains(&"Row 1".to_owned()));

    // Bulk: keys of another workspace don't match, and "all" counts only this one.
    let request =
        BulkRequest::<Private, NoFilter>::parse(b"action=x&ids=1&ids=2&ids=3&ids=4").unwrap();
    let target = request
        .resolve(Private::query().for_workspace(a), &uow)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(target.count(&uow).await.unwrap(), 2, "rows 2 and 4 only");
    let request =
        BulkRequest::<Private, NoFilter>::parse(b"action=x&_mode=all&_expected=19&ex=2&_q=")
            .unwrap();
    let target = request
        .resolve(Private::query().for_workspace(a), &uow)
        .await
        .unwrap()
        .expect("what was shown");
    let changed = target.delete(&uow).await.unwrap();
    assert_eq!(changed, 19);
    let refused = BulkRequest::<Private, NoFilter>::parse(b"action=x&_mode=all&_expected=40&_q=")
        .unwrap()
        .resolve(Private::query().for_workspace(a), &uow)
        .await
        .unwrap();
    assert!(
        refused.is_err(),
        "the whole table's count, from one workspace, is refused"
    );

    // Export: the same scope.
    let response = query
        .export_csv(
            Private::query().for_workspace(a),
            &uow,
            "x.csv",
            navree::orm::Record::into_inner,
        )
        .await
        .unwrap();
    assert_eq!(
        response.headers()["content-type"],
        "text/csv; charset=utf-8"
    );
    uow.rollback().await.unwrap();

    // The owner here is a superuser, which row-level security doesn't bind.
    let all = Private::query()
        .without_workspace_scope()
        .count(&owner)
        .await
        .unwrap();
    assert_eq!(
        all, 40,
        "the rollback put A's rows back, and B's were never touched"
    );

    db.close().await.unwrap();
    owner.close().await.unwrap();
    admin
        .pool()
        .execute_script("DROP DATABASE ui_dt_scoping WITH (FORCE)")
        .await
        .unwrap();
    admin.close().await.unwrap();
}
