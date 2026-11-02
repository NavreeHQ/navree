//! A resource across workspaces under schema isolation: a record goes in the
//! schema of the workspace it names, or nowhere, and never in the shared tables.

use navree_db::Executor as _;

use super::*;

struct SchemaEveryModule;

impl Module for SchemaEveryModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("notes")
                .after(["auth"])
                .migration(SchemaMigration::new(
                    "m20261101_000001_create_notes",
                    CreateNotes,
                )),
        )
        .workspace_migrations(
            MigrationSet::new("workspace_notes").migration(SchemaMigration::new(
                "m20261101_000002_create_notes",
                CreateNotes,
            )),
        )
        .resource::<NoteResource>()
        .resource::<EveryNote>();
    }
}

#[tokio::test]
async fn under_schema_isolation_a_create_across_workspaces_never_lands_in_the_shared_table() {
    if !postgres_only() {
        return;
    }
    // A role row-level security doesn't bind: nothing else would stop it.
    let database = RlsDatabase::create(true);
    let app = TestApp::builder()
        .config("database.url", database.url.clone())
        .config("database.migrate_on_boot", true)
        .config("database.max_connections", 4)
        .config("workspace.isolation", "schema")
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.routes", false)
        .hold(database)
        .module(navree_db::DatabaseModule)
        .module(navree_access::AccessModule::for_root(options()))
        .module(UsersModule)
        .module(WorkspaceModule::from_config())
        .module(SchemaEveryModule)
        .module(navree_helm::HelmModule::from_config())
        .build()
        .await;
    let owner = account(&app, "owner").await;
    let acme = workspace(&app, owner, "Acme").await.id;
    let globex = workspace(&app, owner, "Globex").await.id;
    let member_id = account(&app, "member").await;
    join(&app, acme, member_id).await;
    let outsider_id = account(&app, "outsider").await;
    give_role(&app, member_id, "editor", &["helm.view", "notes.*"]).await;
    give_role(&app, outsider_id, "editor", &["helm.view", "notes.*"]).await;
    let member = app.acting_as(navree_testing::principal(member_id));
    let outsider = app.acting_as(navree_testing::principal(outsider_id));
    let db = app.get::<Db>().await;
    let in_public = |title: &'static str| {
        let db = db.clone();
        async move {
            let row = db
                .fetch_optional(&Statement::text(format!(
                    "SELECT count(*) AS n FROM public.notes WHERE title = '{title}'"
                )))
                .await
                .unwrap()
                .unwrap();
            row.get::<i64>("n").unwrap()
        }
    };

    // Someone who isn't in the workspace gets a clear refusal, the same for one
    // that doesn't exist.
    for target in [globex, Uuid::now_v7()] {
        outsider
            .post(EVERY)
            .json(&json!({ "title": "From outside", "workspace_id": target }))
            .send()
            .await
            .assert_error_code(403, "helm_workspace_refused");
    }
    assert_eq!(in_public("From outside").await, 0);

    // A member's goes in the workspace's schema, and is found there.
    let created = member
        .post(EVERY)
        .json(&json!({ "title": "From a member", "workspace_id": acme }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id = created["data"]["id"].as_str().unwrap().to_owned();
    assert_eq!(in_public("From a member").await, 0);
    in_workspace(&member, acme, &format!("{NOTES}/{id}"))
        .send()
        .await
        .assert_status(200);
}
