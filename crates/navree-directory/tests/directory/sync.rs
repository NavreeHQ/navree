//! Sync runs on the fake directory: stage, diff and apply, the sink, the
//! events, deltas, dry runs, the deactivation limit, failures and runs.

use std::time::Duration;

use navree_directory::{
    ChangeAction, ChangeStatus, ConnectionStatus, ConnectionUpdate, DirectoryError, DirectoryGroup,
    DirectorySyncFinished, DirectoryUser, DirectoryUserCreated, DirectoryUserDeactivated,
    DirectoryUserUpdated, NewConnection, Owner, Pulled, RunStatus, RunTrigger, SyncOptions,
};
use serde_json::json;

use crate::support::harness;

fn ada() -> DirectoryUser {
    DirectoryUser::new("u1", "ada@acme.test")
        .name("Ada", "Lovelace")
        .email("ada@acme.test")
        .title("Engineer")
}

fn grace() -> DirectoryUser {
    DirectoryUser::new("u2", "grace@acme.test")
        .name("Grace", "Hopper")
        .email("grace@acme.test")
}

fn alan() -> DirectoryUser {
    DirectoryUser::new("u3", "alan@acme.test").email("alan@acme.test")
}

#[tokio::test]
async fn a_first_sync_creates_users_and_groups_through_the_sink() {
    let h = harness(|b| b).await;
    h.fake()
        .user(ada())
        .user(grace().inactive())
        .group(DirectoryGroup::new("g1", "Engineering").members(["u1", "u2", "ghost"]));
    let id = h.connection("ldap").await;
    let directory = h.directory().await;

    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded, "{run:?}");
    assert_eq!(run.trigger, RunTrigger::Manual);
    assert_eq!(run.complete, Some(true));
    assert_eq!((run.done, run.total), (3, 3));
    let s = run.summary;
    assert_eq!((s.users_seen, s.users_created, s.groups_created), (2, 2, 1));
    assert_eq!((s.memberships_added, s.unknown_members), (2, 1));
    assert!(run.started_at.is_some() && run.finished_at.is_some());

    assert_eq!(
        h.log.calls(),
        vec![
            "user:create:u1",
            "user:create:u2 (inactive)",
            "group:create:g1:+local-u1,local-u2-",
        ]
    );
    let users = directory.users(id).await.unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].user.user_name, "ada@acme.test");
    assert_eq!(users[0].local_id.as_deref(), Some("local-u1"));
    assert!(users[1].deactivated_at.is_some());
    let groups = directory.groups(id).await.unwrap();
    assert_eq!(groups[0].group.members, vec!["u1", "u2"]);
    assert_eq!(groups[0].local_id.as_deref(), Some("team-g1"));
    let of_ada = directory.groups_of(users[0].id).await.unwrap();
    assert_eq!(of_ada[0].group.display_name, "Engineering");
    assert_eq!(
        directory.user(id, "u1").await.unwrap().unwrap().id,
        users[0].id
    );

    // The events go out after the commit.
    let created = h.inbox.of::<DirectoryUserCreated>();
    assert_eq!(created.len(), 2);
    assert_eq!(created[0].user.external_id, "u1");
    assert_eq!(created[0].local_id.as_deref(), Some("local-u1"));
    assert_eq!(created[0].owner, Owner::App);
    assert_eq!(created[0].run_id, Some(run.id));
    let finished = h.inbox.of::<DirectorySyncFinished>();
    assert_eq!(finished[0].status, RunStatus::Succeeded);
    assert_eq!(finished[0].summary.users_created, 2);

    let changes = directory.changes(run.id, 0, 10).await.unwrap();
    assert_eq!(changes.len(), 3);
    assert!(changes.iter().all(|c| c.status == ChangeStatus::Applied));
    assert_eq!(changes[2].members_added, 2);
    assert_eq!(changes[2].label, "Engineering");

    let connection = directory.connection(id).await.unwrap();
    assert_eq!(connection.last_run_id, Some(run.id));
    assert!(connection.last_synced_at.is_some());
    assert_eq!(connection.next_sync_at, Some(h.clock.now_plus_minutes(60)));
}

trait ClockExt {
    fn now_plus_minutes(&self, minutes: i64) -> chrono::DateTime<chrono::Utc>;
}

impl ClockExt for navree_contracts::FixedClock {
    fn now_plus_minutes(&self, minutes: i64) -> chrono::DateTime<chrono::Utc> {
        use navree_contracts::Clock;
        self.now() + chrono::TimeDelta::minutes(minutes)
    }
}

#[tokio::test]
async fn a_second_snapshot_updates_reactivates_and_deactivates() {
    let h = harness(|b| b).await;
    h.fake()
        .user(ada())
        .user(grace().inactive())
        .user(alan())
        .group(DirectoryGroup::new("g1", "Engineering").members(["u1", "u3"]))
        .group(DirectoryGroup::new("g2", "Legacy"));
    let id = h.connection("okta").await;
    let directory = h.directory().await;
    directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    h.log.clear();
    h.inbox.clear();

    h.fake()
        .user(ada().title("Principal engineer"))
        .user(grace())
        .remove_user("u3")
        .remove_group("g2")
        .group(DirectoryGroup::new("g1", "Engineering").members(["u1", "u2"]));
    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(
        h.log.calls(),
        vec![
            "user:update:u1",
            "user:reactivate:u2",
            "group:update:g1:+local-u2-u3",
            "group:delete:g2",
            "user:deactivate:u3",
        ]
    );
    let updated = h.inbox.of::<DirectoryUserUpdated>();
    assert_eq!(updated.len(), 2);
    assert_eq!(updated[0].changed, vec!["title"]);
    assert!(!updated[0].reactivated);
    assert!(updated[1].reactivated);
    let deactivated = h.inbox.of::<DirectoryUserDeactivated>();
    assert_eq!(deactivated[0].user.external_id, "u3");
    assert!(!deactivated[0].user.active);
    assert!(!deactivated[0].deleted);
    assert_eq!(deactivated[0].local_id.as_deref(), Some("local-u3"));

    let alan = directory.user(id, "u3").await.unwrap().unwrap();
    assert!(!alan.user.active);
    assert!(alan.deactivated_at.is_some());
    assert_eq!(directory.groups(id).await.unwrap().len(), 1);

    // Nothing changed: nothing is applied.
    h.log.clear();
    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.total, 0);
    // u3 is gone from the directory and stays deactivated.
    assert_eq!(run.summary.users_unchanged, 2);
    assert_eq!(run.summary.users_seen, 2);
    assert!(h.log.calls().is_empty());
}

#[tokio::test]
async fn the_cursor_is_handed_to_the_next_pull_and_deltas_change_only_what_they_name() {
    let h = harness(|b| b).await;
    h.fake()
        .user(ada())
        .user(grace())
        .group(DirectoryGroup::new("g1", "Engineering").members(["u1"]));
    let id = h.connection("microsoft").await;
    let directory = h.directory().await;
    directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    h.log.clear();

    h.fake().next_pull(
        Pulled::delta(vec![alan()], Vec::new())
            .removed_user("u2")
            .member_added("g1", "u3")
            .member_removed("g1", "u1")
            .cursor("delta-2"),
    );
    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.complete, Some(false));
    assert_eq!(
        h.log.calls(),
        vec![
            "user:create:u3",
            "group:update:g1:+local-u3-u1",
            "user:deactivate:u2",
        ]
    );
    // u1 is not in the delta and stays as it is.
    assert!(directory.user(id, "u1").await.unwrap().unwrap().user.active);

    directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    let pulls = h.fake().pulls();
    assert_eq!(pulls.len(), 3);
    assert_eq!(pulls[0].cursor, None);
    assert_eq!(pulls[1].cursor.as_deref(), Some("fake-1"));
    assert_eq!(pulls[2].cursor.as_deref(), Some("delta-2"));
    assert_eq!(pulls[2].source, "microsoft");

    // A full sync ignores the cursor.
    let options = SyncOptions {
        full: true,
        ..SyncOptions::default()
    };
    directory.sync_now(id, options).await.unwrap();
    assert_eq!(h.fake().pulls()[3].cursor, None);
}

#[tokio::test]
async fn a_dry_run_plans_without_applying() {
    let h = harness(|b| b).await;
    h.fake().user(ada()).user(grace());
    let id = h.connection("google").await;
    let directory = h.directory().await;

    let run = directory
        .sync_now(id, SyncOptions::dry_run())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded);
    assert!(run.dry_run);
    assert_eq!((run.total, run.done), (2, 0));
    assert_eq!(run.summary.users_created, 2);
    assert!(h.log.calls().is_empty());
    assert!(directory.users(id).await.unwrap().is_empty());
    let changes = directory.changes(run.id, 0, 10).await.unwrap();
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().all(|c| c.status == ChangeStatus::Planned));
    assert_eq!(changes[0].action, ChangeAction::Create);
    assert_eq!(changes[0].label, "ada@acme.test");
    // A dry run keeps no cursor and does not move the schedule.
    let connection = directory.connection(id).await.unwrap();
    assert!(connection.last_synced_at.is_none());
    directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(h.fake().pulls()[1].cursor, None);
    let finished = h.inbox.of::<DirectorySyncFinished>();
    assert!(finished[0].dry_run);
}

#[tokio::test]
async fn a_run_that_would_deactivate_too_many_users_is_held_until_forced() {
    let h = harness(|b| b).await;
    let people: Vec<DirectoryUser> = (0..40)
        .map(|n| DirectoryUser::new(format!("u{n}"), format!("p{n}@acme.test")))
        .collect();
    h.fake().users(people.clone());
    let id = h.connection("ldap").await;
    let directory = h.directory().await;
    directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    h.log.clear();

    // A filter gone wrong: 30 of 40 people disappear.
    h.fake().users(people[..10].to_vec());
    let held = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(held.status, RunStatus::Held);
    assert_eq!(held.summary.users_deactivated, 30);
    assert_eq!(
        held.error.as_deref(),
        Some(
            "the run would deactivate 30 of 40 active users (more than 20 percent); approve it to apply"
        )
    );
    assert!(h.log.calls().is_empty());
    let planned = directory.changes(held.id, 0, 100).await.unwrap();
    assert_eq!(planned.len(), 30);
    assert!(planned.iter().all(|c| c.status == ChangeStatus::Planned));
    assert_eq!(
        h.inbox.of::<DirectorySyncFinished>().last().unwrap().status,
        RunStatus::Held
    );

    let forced = SyncOptions {
        force: true,
        ..SyncOptions::default()
    };
    let run = directory.sync_now(id, forced).await.unwrap();
    assert_eq!(run.status, RunStatus::Succeeded);
    assert!(run.force);
    assert_eq!(h.log.calls().len(), 30);
    assert!(matches!(
        directory.approve(run.id).await,
        Err(DirectoryError::RunNotHeld)
    ));
}

#[tokio::test]
async fn failing_changes_are_recorded_and_retried_by_the_next_run() {
    let h = harness(|b| b.config("directory.sync.chunk_size", 2)).await;
    h.fake().user(ada()).user(grace()).user(alan());
    let id = h.connection("ldap").await;
    let directory = h.directory().await;
    h.log.refuse("u2");

    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Partial);
    assert_eq!(run.summary.failed, 1);
    assert_eq!(run.error.as_deref(), Some("1 changes failed"));
    assert_eq!(run.done, 3);
    let users = directory.users(id).await.unwrap();
    let names: Vec<&str> = users.iter().map(|u| u.user.external_id.as_str()).collect();
    assert_eq!(
        names,
        vec!["u1", "u3"],
        "the refused user's change rolled back"
    );
    let changes = directory.changes(run.id, 0, 10).await.unwrap();
    let failed: Vec<_> = changes
        .iter()
        .filter(|c| c.status == ChangeStatus::Failed)
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].external_id, "u2");
    assert!(failed[0].error.as_deref().unwrap().contains("refusing u2"));
    // A partial run keeps no cursor, so the next run sees everything again.
    assert!(
        directory
            .connection(id)
            .await
            .unwrap()
            .last_synced_at
            .is_none()
    );

    h.log.accept_all();
    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(run.summary.users_created, 1);
    assert_eq!(run.summary.users_unchanged, 2);
    assert_eq!(directory.users(id).await.unwrap().len(), 3);
}

#[tokio::test]
async fn a_source_that_fails_fails_the_run() {
    let h = harness(|b| b).await;
    let id = h.connection("ldap").await;
    let directory = h.directory().await;
    h.fake().fail_next_pull("the server refused the bind");
    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(
        run.error.as_deref(),
        Some("the fake directory could not be read: the server refused the bind")
    );
    let finished = h.inbox.of::<DirectorySyncFinished>();
    assert_eq!(finished[0].status, RunStatus::Failed);
}

#[tokio::test]
async fn only_active_pull_connections_sync() {
    let h = harness(|b| b).await;
    let directory = h.directory().await;
    let scim = h.scim(Owner::App).await;
    assert!(matches!(
        directory
            .sync_now(scim.connection.id, SyncOptions::default())
            .await,
        Err(DirectoryError::PushOnly)
    ));
    let id = h.connection("ldap").await;
    directory
        .update_connection(
            id,
            ConnectionUpdate {
                status: Some(ConnectionStatus::Paused),
                ..ConnectionUpdate::default()
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        directory.sync_now(id, SyncOptions::default()).await,
        Err(DirectoryError::Paused)
    ));
    assert!(matches!(
        directory
            .sync_now(uuid::Uuid::nil(), SyncOptions::default())
            .await,
        Err(DirectoryError::ConnectionNotFound)
    ));
}

#[tokio::test]
async fn a_queued_run_blocks_another_until_it_goes_stale() {
    let queue = navree_queue::QueueFake::new();
    let dispatcher = queue.dispatcher();
    let h = harness(move |b| b.override_value(dispatcher)).await;
    let id = h.connection("ldap").await;
    let directory = h.directory().await;

    let queued = directory.sync(id, SyncOptions::default()).await.unwrap();
    assert_eq!(queued.status, RunStatus::Queued);
    queue.assert_dispatched::<navree_directory::SyncDirectory>(|job| job.run_id == queued.id);
    assert!(matches!(
        directory.sync(id, SyncOptions::default()).await,
        Err(DirectoryError::SyncRunning)
    ));
    // Two hours later the worker is taken for crashed.
    h.clock.advance(Duration::from_secs(121 * 60));
    let next = directory.sync(id, SyncOptions::default()).await.unwrap();
    assert_ne!(next.id, queued.id);
    let stale = directory.run(queued.id).await.unwrap();
    assert_eq!(stale.status, RunStatus::Failed);
    assert_eq!(stale.error.as_deref(), Some("the run stopped responding"));
}

#[tokio::test]
async fn due_connections_are_queued_by_the_schedule() {
    let queue = navree_queue::QueueFake::new();
    let dispatcher = queue.dispatcher();
    let h = harness(move |b| b.override_value(dispatcher)).await;
    let directory = h.directory().await;
    let hourly = h.connection("ldap").await;
    let manual = directory
        .create_connection(
            Owner::App,
            NewConnection::new("On demand", "okta").interval(0),
        )
        .await
        .unwrap()
        .connection;
    assert!(manual.next_sync_at.is_none());
    h.scim(Owner::App).await;

    let runs = directory.sync_due().await.unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].connection_id, hourly);
    assert_eq!(runs[0].trigger, RunTrigger::Schedule);
    // Not due again within the hour.
    assert!(directory.sync_due().await.unwrap().is_empty());
    let connection = directory.connection(hourly).await.unwrap();
    assert!(connection.next_sync_at.unwrap() > h.clock.now_plus_minutes(59));
}

#[tokio::test]
async fn connections_keep_secrets_sealed_and_delete_their_rows() {
    let h = harness(|b| b).await;
    let directory = h.directory().await;
    let created = directory
        .create_connection(
            Owner::App,
            NewConnection::new("Acme AD", "ldap")
                .settings(json!({ "url": "ldaps://dc1.acme.test", "base_dn": "DC=acme,DC=test" }))
                .secret("hunter2"),
        )
        .await
        .unwrap();
    let id = created.connection.id;
    assert!(created.connection.has_secret);
    assert!(created.scim_token.is_none());
    use navree_orm::Model as _;
    let db = h.app.get::<navree_db::Db>().await;
    let row = navree_directory::models::Connection::query()
        .where_(navree_directory::models::Connection::ID.eq(id))
        .first(&*db)
        .await
        .unwrap()
        .unwrap();
    let sealed = row.secret.clone().unwrap();
    assert!(!sealed.contains("hunter2"));
    let shown = serde_json::to_string(&created.connection).unwrap();
    assert!(!shown.contains("hunter2") && !shown.contains(&sealed));

    let updated = directory
        .update_connection(
            id,
            ConnectionUpdate {
                name: Some("Acme Active Directory".into()),
                secret: Some(String::new()),
                ..ConnectionUpdate::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "Acme Active Directory");
    assert!(!updated.has_secret);

    h.fake()
        .user(ada())
        .group(DirectoryGroup::new("g1", "Eng").members(["u1"]));
    directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    directory.delete_connection(id).await.unwrap();
    assert!(matches!(
        directory.connection(id).await,
        Err(DirectoryError::ConnectionNotFound)
    ));
    assert_eq!(
        navree_directory::models::User::query()
            .count(&*db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        navree_directory::models::Membership::query()
            .count(&*db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        navree_directory::models::Run::query()
            .count(&*db)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn bad_input_is_refused() {
    let h = harness(|b| b).await;
    let directory = h.directory().await;
    assert!(matches!(
        directory
            .create_connection(Owner::App, NewConnection::new(" ", "ldap"))
            .await,
        Err(DirectoryError::InvalidSettings(_))
    ));
    assert!(matches!(
        directory
            .create_connection(Owner::App, NewConnection::new("x", "Bad Name"))
            .await,
        Err(DirectoryError::UnknownSource(_))
    ));
    assert!(matches!(
        directory
            .create_connection(
                Owner::App,
                NewConnection::new("x", "ldap").settings(json!(["not", "an", "object"]))
            )
            .await,
        Err(DirectoryError::InvalidSettings(_))
    ));
    let id = h.connection("ldap").await;
    assert!(matches!(
        directory.rotate_scim_token(id).await,
        Err(DirectoryError::NotScim)
    ));
}

#[tokio::test]
async fn old_runs_are_pruned() {
    let h = harness(|b| b).await;
    let id = h.connection("ldap").await;
    let directory = h.directory().await;
    h.fake().user(ada());
    let old = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    h.clock.advance(Duration::from_secs(91 * 24 * 3600));
    let recent = directory
        .sync_now(id, SyncOptions::dry_run())
        .await
        .unwrap();
    assert_eq!(directory.prune_runs().await.unwrap(), 1);
    assert!(matches!(
        directory.run(old.id).await,
        Err(DirectoryError::RunNotFound)
    ));
    assert!(directory.changes(old.id, 0, 10).await.unwrap().is_empty());
    assert_eq!(directory.runs(id, 10).await.unwrap()[0].id, recent.id);
    h.app.run_command("directory:prune", &[]).await.unwrap();
}
