//! Members limited to partitions, a small pool, and a row a role can only read.

use navree_http::ModuleBuilderHttpExt;
use navree_workspace::{NewPartition, PartitionSelector, Partitions};

use super::*;

#[tokio::test]
async fn a_member_limited_to_partitions_isnt_let_in_to_see_every_partition() {
    let app = with_members(crate::servers::database(TestApp::builder()))
        .config("workspace.partitions.enabled", true)
        .build()
        .await;
    let world = world(app).await;
    let partitions = world.app.get::<Partitions>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let partition = uow
        .scope(partitions.create(&uow, world.acme, NewPartition::named("East")))
        .await
        .unwrap();
    uow.scope(partitions.grant(
        &uow,
        world.acme,
        world.member_id,
        PartitionSelector::only([partition.id]),
    ))
    .await
    .unwrap();
    uow.commit().await.unwrap();
    in_workspace(&world.member, world.acme, NOTES)
        .send()
        .await
        .assert_error_code(403, "helm_workspace_partitions");
    let response = world
        .member
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", world.acme.to_string().as_str())])
        .send()
        .await
        .assert_status(303);
    assert!(response.set_cookie(WORKSPACE_COOKIE).is_none());
    // Given every partition again, they are back.
    let uow = db.uow(None, None).await;
    uow.scope(partitions.grant(&uow, world.acme, world.member_id, PartitionSelector::All))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    in_workspace(&world.member, world.acme, NOTES)
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn a_pool_of_one_connection_doesnt_stop_a_form_that_names_its_workspace() {
    // The request's transaction holds the one connection when the form's
    // workspace is asked about.
    if Server::current() == Server::Sqlite {
        return;
    }
    let (url, guard) = crate::servers::scratch();
    let app = with_members(
        TestApp::builder()
            .config("database.url", url)
            .config("database.migrate_on_boot", true)
            .config("database.max_connections", 1)
            .hold(guard),
    )
    .build()
    .await;
    let world = world(app).await;
    world
        .member
        .set_cookie(WORKSPACE_COOKIE, &world.acme.to_string());
    join(&world.app, world.globex, world.member_id).await;
    let started = std::time::Instant::now();
    world
        .member
        .post(EVERY)
        .json(&json!({ "title": "Small pool", "workspace_id": world.globex }))
        .send()
        .await
        .assert_status(201);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

/// A policy shows every row of the table to a request with a test setting, as
/// an organization's read policy does for the rows of its workspaces.
struct ShowLayer;

impl Module for ShowLayer {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(navree_http::GlobalLayer::new(
            navree_http::LayerSlot::Context,
            "test-show",
            |_ctx, router| {
                Ok(router.layer(navree_http::middleware::from_fn(
                    |request: navree_http::extract::Request,
                     next: navree_http::middleware::Next| async move {
                        if let Some(uow) = request.extensions().get::<navree_db::Uow>().cloned() {
                            uow.set_local("navree.shown", "shown").await.unwrap();
                        }
                        next.run(request).await
                    },
                )))
            },
        ));
    }
}

#[tokio::test]
async fn a_row_a_bound_role_can_only_read_isnt_written_through_a_resource_across_workspaces() {
    if !postgres_only() {
        return;
    }
    let database = RlsDatabase::create(false);
    let builder = TestApp::builder()
        .config("database.url", database.url.clone())
        .config("database.migrate_on_boot", true)
        .config("database.max_connections", 4)
        .hold(database)
        .module(ShowLayer);
    let world = world(with_members(builder).build().await).await;
    {
        use navree_db::Executor as _;
        let db = world.app.get::<Db>().await;
        db.execute(&Statement::text(
            "CREATE POLICY shown_rows ON notes FOR SELECT \
             USING (current_setting('navree.shown', true) = 'shown')",
        ))
        .await
        .unwrap();
    }
    // The outsider sees the row. A write locks it first, which a policy for
    // reads alone doesn't allow, so to a write it isn't there.
    world
        .outsider
        .get(&format!("{EVERY}/{}", world.globex_note))
        .send()
        .await
        .assert_status(200);
    world
        .outsider
        .patch(&format!("{EVERY}/{}", world.globex_note))
        .json(&json!({ "title": "Read, then written" }))
        .send()
        .await
        .assert_error(404, None);
    world
        .outsider
        .delete(&format!("{EVERY}/{}", world.globex_note))
        .send()
        .await
        .assert_error(404, None);
    assert_eq!(
        note_in(&world.app, world.globex, world.globex_note)
            .await
            .unwrap()
            .title,
        "Globex secret"
    );
}
