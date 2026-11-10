//! What staff do is written down whether or not it works: an entry doesn't roll
//! back with a request or a queued action that fails.

use super::*;

/// One entry more than before, after each of these requests that fail.
#[tokio::test]
async fn a_request_that_fails_leaves_its_staff_entry() {
    let (world, staff) = staff_world().await;
    let gulati = world.gulati.to_string();
    let note = world.gulati_note;
    let cases = [
        (
            "a record that isn't there",
            404,
            staff.get(&format!("{NOTES}/{}", Uuid::now_v7())),
        ),
        ("a bad sort", 400, staff.get(&format!("{NOTES}?sort=nope"))),
        (
            "an invalid change",
            400,
            staff
                .patch(&format!("{NOTES}/{note}"))
                .json(&json!({ "title": "" })),
        ),
        (
            "an invalid form",
            400,
            staff
                .post(&format!("/admin/resources/notes/{note}"))
                .header("accept", "text/html")
                .form(&[("title", "")]),
        ),
    ];
    let mut expected = staff_entries(&world.app).await.len();
    for (what, status, request) in cases {
        request
            .header("X-Workspace", gulati.clone())
            .send()
            .await
            .assert_status(status);
        expected += 1;
        assert_eq!(staff_entries(&world.app).await.len(), expected, "{what}");
    }
}

#[tokio::test]
async fn a_queued_action_that_fails_leaves_its_staff_entry() {
    use navree_contracts::{Job, JobContext};

    let fake = QueueFake::new();
    let builder = crate::servers::database(TestApp::builder())
        .config("audit.routes", false)
        .override_value::<dyn navree_contracts::Dispatcher>(fake.dispatcher())
        .module(navree_audit::AuditModule::from_config())
        .module(StaffNotesModule);
    let world = world(with_members(builder).build().await).await;
    let staff_id = account(&world.app, "staff").await;
    give_role(
        &world.app,
        staff_id,
        "support",
        &["helm.view", "notes.*", "support.enter"],
    )
    .await;
    let staff = world.app.acting_as(navree_testing::principal(staff_id));
    staff
        .post(&format!("{NOTES}/actions/whisper"))
        .header("X-Workspace", world.gulati.to_string())
        .json(&json!({ "ids": [world.gulati_note] }))
        .send()
        .await
        .assert_status(200);
    let before = staff_entries(&world.app).await.len();
    // The record goes before the job runs, so the run fails.
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    enter(&uow, world.gulati).await;
    Note::query()
        .where_key(world.gulati_note)
        .delete(&uow)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let job = fake.dispatched::<HelmActionJob>().pop().unwrap();
    job.handle(JobContext {
        app: world.app.ctx().clone(),
        job_id: "1".into(),
        attempt: 1,
    })
    .await
    .expect_err("the record has gone");
    assert_eq!(staff_entries(&world.app).await.len(), before + 1);
}
