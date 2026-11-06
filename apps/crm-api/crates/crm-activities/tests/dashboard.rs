//! `GET /api/v1/dashboard`: the pipeline, deals won this
//! month and open deal totals are team-wide (amounts summed across
//! currencies); task counts are the signed-in user's (assigned to them, or
//! owned by them when unassigned); the ten newest activities.

mod support;

use chrono::Duration;
use crm_core::{ActivityKind, Stage, SubjectType};
use crm_testkit::{Role, TestCrm};
use serde_json::{Value, json};

use support::{NewActivity, NewDeal, activity, at, company, crm_at, deal};

/// Wednesday 2026-10-28, 10:00 UTC: this month began 2026-10-01 00:00 UTC.
fn now() -> chrono::DateTime<chrono::Utc> {
    at("2026-10-28T10:00:00Z")
}

/// The fixture: deals across every stage (one in EUR) and activities for
/// rep1, rep2 and the manager.
async fn seeded() -> TestCrm {
    let crm = crm_at(now()).await;
    let rep1 = crm.user_id(Role::Rep1);
    let rep2 = crm.user_id(Role::Rep2);
    let manager = crm.user_id(Role::Manager);
    let agra = company(&crm, rep1, "Agra").await;
    for (owner, new) in [
        (rep1, NewDeal::open("Lead A", Stage::Lead, 100_000)),
        (
            rep2,
            NewDeal::open("Lead B", Stage::Lead, 50_000).currency("EUR"),
        ),
        (
            rep2,
            NewDeal::open("Qualified C", Stage::Qualified, 200_000),
        ),
        (rep1, NewDeal::open("Proposal D", Stage::Proposal, 333)),
        (
            manager,
            NewDeal::open("Negotiation E", Stage::Negotiation, 400_000),
        ),
        (
            rep1,
            NewDeal::closed("Won F", Stage::Won, 1_000_000, at("2026-10-05T12:00:00Z")),
        ),
        (
            rep2,
            NewDeal::closed(
                "Won G last month",
                Stage::Won,
                250_000,
                at("2026-09-30T23:59:59Z"),
            ),
        ),
        (
            rep2,
            NewDeal::closed(
                "Won H at midnight",
                Stage::Won,
                70_000,
                at("2026-10-01T00:00:00Z"),
            ),
        ),
        (
            rep1,
            NewDeal::closed("Lost I", Stage::Lost, 90_000, at("2026-10-10T08:00:00Z")),
        ),
    ] {
        deal(&crm, owner, agra.id, new).await;
    }

    // Activities, created one minute apart (newest last).
    let base = at("2026-10-20T09:00:00Z");
    let task = |subject: &str, owner| {
        NewActivity::task(subject, agra.id, owner).on(SubjectType::Company, agra.id)
    };
    let activities = [
        task("T1 overdue, assigned to rep1", rep2)
            .assigned(rep1)
            .due(at("2026-10-27T23:00:00Z")),
        task("T2 today, assigned to rep1", rep2)
            .assigned(rep1)
            .due(at("2026-10-28T15:00:00Z")),
        task("T3 today, rep1's own", rep1).due(at("2026-10-28T09:00:00Z")),
        task("T4 upcoming, assigned to rep1", manager)
            .assigned(rep1)
            .due(at("2026-10-29T00:00:00Z")),
        task("T5 done", rep2)
            .assigned(rep1)
            .due(at("2026-10-20T00:00:00Z"))
            .completed(at("2026-10-21T00:00:00Z")),
        task("T6 rep1's, assigned to rep2", rep1)
            .assigned(rep2)
            .due(at("2026-10-27T00:00:00Z")),
        task("T7 rep2's own", rep2).due(at("2026-10-28T11:00:00Z")),
        task("M1 meeting", rep2)
            .kind(ActivityKind::Meeting)
            .assigned(rep1)
            .due(at("2026-10-27T10:00:00Z")),
        task("T8 no due date", rep2).assigned(rep1),
        task("N1 note", rep1).kind(ActivityKind::Note),
        task("N2 note", rep2).kind(ActivityKind::Note),
        task("N3 note", manager).kind(ActivityKind::Note),
    ];
    for (minute, new) in (0..).zip(activities) {
        activity(&crm, new.created(base + Duration::minutes(minute))).await;
    }
    crm
}

/// Amounts whose sum does not fit in 64 bits (rows from before the input
/// cap, or very many capped deals) saturate instead of failing the
/// dashboard for the whole team.
#[tokio::test]
async fn huge_amounts_do_not_break_the_dashboard() {
    let crm = crm_at(now()).await;
    let rep1 = crm.user_id(Role::Rep1);
    let agra = company(&crm, rep1, "Agra").await;
    for new in [
        NewDeal::open("Huge A", Stage::Lead, i64::MAX),
        NewDeal::open("Huge B", Stage::Lead, i64::MAX),
        NewDeal::open("Small", Stage::Lead, 100),
        NewDeal::open("Huge C", Stage::Qualified, i64::MAX),
        NewDeal::closed("Won A", Stage::Won, i64::MAX, at("2026-10-05T12:00:00Z")),
        NewDeal::closed("Won B", Stage::Won, i64::MAX, at("2026-10-06T12:00:00Z")),
    ] {
        deal(&crm, rep1, agra.id, new).await;
    }
    let body = dashboard(&crm, Role::Manager).await;
    assert_eq!(
        body["pipeline"][0],
        json!({ "stage": "lead", "count": 3, "amount_cents": i64::MAX })
    );
    assert_eq!(
        body["won_this_month"],
        json!({ "count": 2, "amount_cents": i64::MAX })
    );
    assert_eq!(body["open_deals"]["count"], 4);
    assert_eq!(body["open_deals"]["amount_cents"], json!(i64::MAX));
    crm.shutdown().await;
}

async fn dashboard(crm: &TestCrm, role: Role) -> Value {
    crm.login_as(role)
        .await
        .get("/api/v1/dashboard")
        .send()
        .await
        .assert_status(200)
        .json_value()
}

#[tokio::test]
async fn dashboard_numbers_match_seeded_fixture() {
    let crm = seeded().await;
    let body = dashboard(&crm, Role::Rep1).await;

    assert_eq!(
        body["pipeline"],
        json!([
            { "stage": "lead", "count": 2, "amount_cents": 150_000 },
            { "stage": "qualified", "count": 1, "amount_cents": 200_000 },
            { "stage": "proposal", "count": 1, "amount_cents": 333 },
            { "stage": "negotiation", "count": 1, "amount_cents": 400_000 },
            { "stage": "won", "count": 3, "amount_cents": 1_320_000 },
            { "stage": "lost", "count": 1, "amount_cents": 90_000 },
        ])
    );
    // F (10-05) and H (10-01 00:00) were won this month; G on 09-30 was not.
    assert_eq!(
        body["won_this_month"],
        json!({ "count": 2, "amount_cents": 1_070_000 })
    );
    // Weighted: 150000×10% + 200000×25% + 333×50% + 400000×75% = 365166.5,
    // rounded down to the cent.
    assert_eq!(
        body["open_deals"],
        json!({ "count": 5, "amount_cents": 750_333, "weighted_amount_cents": 365_166 })
    );
    // rep1: T1 overdue; T2 and T3 today; T4 upcoming. Not T5 (done), T6
    // (assigned to rep2), T7 (rep2's), M1 (a meeting) or T8 (no due date).
    assert_eq!(
        body["tasks"],
        json!({ "overdue": 1, "due_today": 2, "upcoming": 1 })
    );

    let recent = body["recent_activities"]
        .as_array()
        .expect("recent activities");
    let subjects: Vec<&str> = recent
        .iter()
        .map(|activity| activity["subject"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        subjects,
        [
            "N3 note",
            "N2 note",
            "N1 note",
            "T8 no due date",
            "M1 meeting",
            "T7 rep2's own",
            "T6 rep1's, assigned to rep2",
            "T5 done",
            "T4 upcoming, assigned to rep1",
            "T3 today, rep1's own",
        ]
    );
    assert_eq!(recent[0]["__type"], json!("Activity"));
    assert_eq!(recent[0]["subject_label"], json!("Agra"));
    assert_eq!(
        recent[0]["owner"],
        json!({ "id": crm.user_id(Role::Manager), "name": "Manpreet Gill" })
    );
    crm.shutdown().await;
}

#[tokio::test]
async fn task_counts_are_personal_and_the_rest_team_wide() {
    let crm = seeded().await;
    let rep1 = dashboard(&crm, Role::Rep1).await;
    let rep2 = dashboard(&crm, Role::Rep2).await;
    let manager = dashboard(&crm, Role::Manager).await;
    let admin = dashboard(&crm, Role::Admin).await;
    for other in [&rep2, &manager, &admin] {
        for key in [
            "pipeline",
            "won_this_month",
            "open_deals",
            "recent_activities",
        ] {
            assert_eq!(other[key], rep1[key], "{key}");
        }
    }
    // rep2: T6 (assigned to them) overdue; T7 (their own, unassigned) today.
    assert_eq!(
        rep2["tasks"],
        json!({ "overdue": 1, "due_today": 1, "upcoming": 0 })
    );
    assert_eq!(
        manager["tasks"],
        json!({ "overdue": 0, "due_today": 0, "upcoming": 0 })
    );
    crm.shutdown().await;
}

#[tokio::test]
async fn an_empty_crm_has_zeroes_in_every_stage() {
    let crm = crm_at(now()).await;
    let body = dashboard(&crm, Role::Manager).await;
    assert_eq!(
        body,
        json!({
            "pipeline": Stage::ALL
                .iter()
                .map(|stage| json!({ "stage": stage, "count": 0, "amount_cents": 0 }))
                .collect::<Vec<_>>(),
            "won_this_month": { "count": 0, "amount_cents": 0 },
            "open_deals": { "count": 0, "amount_cents": 0, "weighted_amount_cents": 0 },
            "tasks": { "overdue": 0, "due_today": 0, "upcoming": 0 },
            "recent_activities": [],
        })
    );
    crm.client()
        .get("/api/v1/dashboard")
        .send()
        .await
        .assert_error(401, None);
    crm.shutdown().await;
}
