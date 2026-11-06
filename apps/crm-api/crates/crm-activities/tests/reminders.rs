//! The daily schedule: `crm:task-reminders` at 08:00 UTC notifies
//! everyone with open tasks due today (database, mail and their private
//! channel; data `{title, message, url}`), and `audit:prune`
//! runs daily.

mod support;

use crm_core::{ActivityKind, SubjectType};
use crm_testkit::{Role, TestCrm};
use navree::mail::Mailer;
use navree::schedule::{RunStatus, Scheduler, SkipReason};
use serde_json::{Value, json};

use support::{NewActivity, activity, at, company};

/// Monday 2026-10-26, a minute before the reminders are due.
fn before_eight() -> chrono::DateTime<chrono::Utc> {
    at("2026-10-26T07:59:00Z")
}

/// A CRM whose clock starts at [`before_eight`], with `app.url` set (the
/// links in reminder mails), plus `config` overrides.
async fn crm_with(config: &[(&str, Value)]) -> TestCrm {
    let mut builder = TestCrm::builder()
        .module(crm_activities::ActivitiesModule)
        .module(crm_activities::DashboardModule)
        .config("app.url", "http://localhost:3000")
        .now(before_eight());
    for (key, value) in config {
        builder = builder.config(key, value.clone());
    }
    builder.build().await
}

/// Tasks for every case: rep1 has two due today (and one tomorrow), rep2
/// owns one unassigned task due at midnight; the manager's are completed
/// or overdue; the admin only has a meeting today.
async fn seed_tasks(crm: &TestCrm) {
    let rep1 = crm.user_id(Role::Rep1);
    let rep2 = crm.user_id(Role::Rep2);
    let manager = crm.user_id(Role::Manager);
    let admin = crm.user_id(Role::Admin);
    let agra = company(crm, rep2, "Agra").await;
    let task = |subject: &str, owner| {
        NewActivity::task(subject, agra.id, owner).on(SubjectType::Company, agra.id)
    };
    for new in [
        task("Send the contract", rep2)
            .assigned(rep1)
            .due(at("2026-10-26T23:59:59Z")),
        task("Call Agra back", rep2)
            .assigned(rep1)
            .due(at("2026-10-26T09:00:00Z")),
        task("Prepare tomorrow's demo", rep1)
            .assigned(rep1)
            .due(at("2026-10-27T00:00:00Z")),
        task("Renew the NDA", rep2).due(at("2026-10-26T00:00:00Z")),
        task("Done already", manager)
            .assigned(manager)
            .due(at("2026-10-26T12:00:00Z"))
            .completed(at("2026-10-26T07:00:00Z")),
        task("Overdue since Sunday", manager)
            .assigned(manager)
            .due(at("2026-10-25T12:00:00Z")),
        task("Board meeting", admin)
            .kind(ActivityKind::Meeting)
            .assigned(admin)
            .due(at("2026-10-26T15:00:00Z")),
    ] {
        activity(crm, new).await;
    }
}

/// The notifications `role` sees in `GET /api/notifications`.
async fn notifications(crm: &TestCrm, role: Role) -> Vec<Value> {
    let page = crm
        .login_as(role)
        .await
        .get("/api/notifications")
        .send()
        .await
        .assert_status(200)
        .json_value();
    page["data"].as_array().cloned().expect("a page")
}

fn status_of(outcomes: &[navree::schedule::RunOutcome], task: &str) -> Option<RunStatus> {
    outcomes
        .iter()
        .find(|outcome| outcome.task == task)
        .map(|outcome| outcome.status.clone())
}

#[tokio::test]
async fn reminder_tick_notifies_assignees_of_tasks_due_today() {
    let crm = crm_with(&[]).await;
    seed_tasks(&crm).await;
    let scheduler = crm.get::<Scheduler>().await;

    // 07:59: not due yet.
    let outcomes = scheduler.tick(before_eight()).await.expect("tick");
    assert_eq!(status_of(&outcomes, "crm:task-reminders"), None);

    // 08:00 UTC.
    crm.clock().set(at("2026-10-26T08:00:00Z"));
    let outcomes = scheduler
        .tick(at("2026-10-26T08:00:30Z"))
        .await
        .expect("tick");
    assert_eq!(
        status_of(&outcomes, "crm:task-reminders"),
        Some(RunStatus::Succeeded),
        "{outcomes:?}"
    );

    // rep1: both tasks due today, earliest first; not tomorrow's.
    let rep1 = notifications(&crm, Role::Rep1).await;
    assert_eq!(rep1.len(), 1, "{rep1:#?}");
    assert_eq!(rep1[0]["type"], json!("tasks_due_today"));
    assert_eq!(
        rep1[0]["data"],
        json!({
            "title": "Tasks due today",
            "message": "You have 2 tasks due today: Call Agra back and Send the contract.",
            "url": "/tasks?tab=today",
        })
    );
    // rep2: their own unassigned task.
    let rep2 = notifications(&crm, Role::Rep2).await;
    assert_eq!(rep2.len(), 1, "{rep2:#?}");
    assert_eq!(
        rep2[0]["data"],
        json!({
            "title": "Tasks due today",
            "message": "You have 1 task due today: Renew the NDA.",
            "url": "/tasks?tab=today",
        })
    );
    // Completed and overdue tasks, and meetings, are not reminded of.
    assert!(notifications(&crm, Role::Manager).await.is_empty());
    assert!(notifications(&crm, Role::Admin).await.is_empty());

    // Each got a mail linking to their tasks.
    let mailer = crm.get::<Mailer>().await;
    let sent = mailer.array_transport().expect("array transport").sent();
    let mut recipients: Vec<String> = sent.iter().flat_map(|mail| mail.to.clone()).collect();
    recipients.sort();
    assert_eq!(recipients, ["rep1@navree.test", "rep2@navree.test"]);
    let to_rep1 = sent
        .iter()
        .find(|mail| mail.to.contains(&"rep1@navree.test".to_owned()))
        .expect("rep1's mail");
    assert_eq!(to_rep1.subject, "2 tasks due today");
    let text = to_rep1.text.clone().unwrap_or_default();
    for expected in [
        "Priya Verma",
        "Call Agra back",
        "Send the contract",
        "http://localhost:3000/tasks?tab=today",
    ] {
        assert!(text.contains(expected), "{expected:?} missing from {text}");
    }
    assert!(!text.contains("tomorrow's demo"), "{text}");
    let html = to_rep1.html.clone().unwrap_or_default();
    assert!(html.contains("Call Agra back"), "{html}");

    // The tick runs once, however often it is claimed; the next minute is
    // not due.
    let again = scheduler
        .tick(at("2026-10-26T08:00:50Z"))
        .await
        .expect("tick");
    assert_eq!(
        status_of(&again, "crm:task-reminders"),
        Some(RunStatus::Skipped(SkipReason::AlreadyClaimed))
    );
    let later = scheduler
        .tick(at("2026-10-26T08:01:00Z"))
        .await
        .expect("tick");
    assert_eq!(status_of(&later, "crm:task-reminders"), None);
    assert_eq!(notifications(&crm, Role::Rep1).await.len(), 1);
    crm.shutdown().await;
}

#[tokio::test]
async fn reminders_and_audit_pruning_are_scheduled_daily() {
    let crm = crm_with(&[]).await;
    let scheduler = crm.get::<Scheduler>().await;
    let tasks = scheduler.list(before_eight());
    let find = |name: &str| {
        tasks
            .iter()
            .find(|task| task.name == name)
            .unwrap_or_else(|| panic!("{name} is not scheduled: {tasks:#?}"))
    };
    let reminders = find("crm:task-reminders");
    assert_eq!(reminders.expr, "0 8 * * *");
    assert_eq!(reminders.next_run, Some(at("2026-10-26T08:00:00Z")));
    assert!(reminders.without_overlapping);
    let prune = find("audit:prune");
    assert_eq!(prune.expr, "0 3 * * *");
    assert!(prune.without_overlapping);

    // audit:prune runs at 03:00 UTC.
    let outcomes = scheduler
        .tick(at("2026-10-27T03:00:00Z"))
        .await
        .expect("tick");
    assert_eq!(
        status_of(&outcomes, "audit:prune"),
        Some(RunStatus::Succeeded),
        "{outcomes:?}"
    );
    assert_eq!(status_of(&outcomes, "crm:task-reminders"), None);
    crm.shutdown().await;
}

#[tokio::test]
async fn the_reminder_command_runs_on_demand_and_with_a_queue() {
    // Notifications queued for a worker (`notify.queue`).
    let crm = crm_with(&[("notify.queue", json!(true))]).await;
    seed_tasks(&crm).await;
    crm.clock().set(at("2026-10-26T10:00:00Z"));
    crm.app()
        .run_command("crm:task-reminders", &[])
        .await
        .expect("the command runs");
    let stats = crm.run_queue().await;
    assert_eq!(stats.failed, 0, "{stats:?}");
    assert!(stats.succeeded > 0, "{stats:?}");
    assert_eq!(notifications(&crm, Role::Rep1).await.len(), 1);
    assert_eq!(notifications(&crm, Role::Rep2).await.len(), 1);
    let sent = crm
        .get::<Mailer>()
        .await
        .array_transport()
        .expect("array transport")
        .sent();
    assert_eq!(sent.len(), 2);
    crm.shutdown().await;
}

#[tokio::test]
async fn reminder_messages_list_at_most_three_tasks() {
    let crm = crm_with(&[]).await;
    let rep1 = crm.user_id(Role::Rep1);
    let agra = company(&crm, rep1, "Agra").await;
    for (hour, subject) in [
        (9, "One"),
        (10, "Two"),
        (11, "Three"),
        (12, "Four"),
        (13, "Five"),
    ] {
        activity(
            &crm,
            NewActivity::task(subject, agra.id, rep1)
                .on(SubjectType::Company, agra.id)
                .due(at(&format!("2026-10-26T{hour:02}:00:00Z"))),
        )
        .await;
    }
    crm.clock().set(at("2026-10-26T08:00:00Z"));
    crm.app()
        .run_command("crm:task-reminders", &[])
        .await
        .expect("the command runs");
    let rep1_notifications = notifications(&crm, Role::Rep1).await;
    assert_eq!(
        rep1_notifications[0]["data"]["message"],
        json!("You have 5 tasks due today: One, Two, Three and 2 more.")
    );
    crm.shutdown().await;
}
