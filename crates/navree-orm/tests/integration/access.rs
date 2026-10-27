//! Access control on models: abilities as query filters (counts and pages
//! included), record checks, `Authorized<Model, A>` and `Accessible<Model,
//! A>` in routes with a policy attached to the model, and the property that
//! a record check and a list filter never disagree.

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use http::Method;
use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, Accessible, Authorized, READ, Read, Rules,
    SubjectRecords, UPDATE, Update, field,
};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::access::{AbilityExt, AccessibleQuery, ModelColumns, record_json};
use navree_orm::{Model, Record};
use navree_testing::principal;
use proptest::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::models::Ticket;
use crate::support::{app, sqlite};

/// Two of Alice's tickets (one closed), one of Bob's, one without owner.
async fn tickets(db: &navree_db::Db, alice: Uuid, bob: Uuid) -> Vec<Record<Ticket>> {
    let mut made = Vec::new();
    for (owner, status, priority) in [
        (Some(alice), "open", 1),
        (Some(alice), "closed", 3),
        (Some(bob), "open", 5),
        (None, "open", 2),
    ] {
        made.push(
            Ticket::factory()
                .set(Ticket::OWNER_ID, owner)
                .set(Ticket::STATUS, status)
                .set(Ticket::PRIORITY, priority)
                .create_one(db)
                .await
                .unwrap(),
        );
    }
    made
}

/// Alice's abilities under the ticket policy.
fn abilities_of(user: Uuid) -> navree_access::Ability {
    let ctx = AbilityCtx::for_user(user, Vec::new(), Vec::new());
    let mut rules = Rules::new();
    <Ticket as SubjectRecords>::policy().expect("a policy")(&ctx, &mut rules);
    rules.try_build().unwrap()
}

#[tokio::test]
async fn queries_return_what_the_rules_allow() {
    let test = sqlite().await;
    let (alice, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let made = tickets(&test.db, alice, bob).await;
    let ability = abilities_of(alice);

    assert_eq!(Ticket::query().accessible(&ability, Read).count(&test.db).await.unwrap(), 4);
    let updatable = Ticket::query()
        .accessible(&ability, Update)
        .get(&test.db)
        .await
        .unwrap();
    assert_eq!(updatable.model_keys(), [made[0].id], "her own open ticket only");
    assert_eq!(
        Ticket::query()
            .accessible_for(&ability, "update")
            .count(&test.db)
            .await
            .unwrap(),
        1
    );
    // Pages and aggregates respect the filter.
    let page = Ticket::query()
        .accessible(&ability, Update)
        .paginate(&test.db, (1, 10))
        .await
        .unwrap();
    assert_eq!(page.meta.total, 1);
    assert_eq!(
        Ticket::query()
            .accessible(&ability, Update)
            .sum::<i64>(&test.db, Ticket::PRIORITY)
            .await
            .unwrap(),
        1
    );
    // A guest may do nothing.
    let guest = Rules::new().try_build().unwrap();
    assert_eq!(Ticket::query().accessible(&guest, Read).count(&test.db).await.unwrap(), 0);
}

#[tokio::test]
async fn records_are_checked_like_lists_are_filtered() {
    let test = sqlite().await;
    let (alice, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let made = tickets(&test.db, alice, bob).await;
    let ability = abilities_of(alice);
    assert!(ability.allows(Update, &made[0]));
    assert!(!ability.allows(Update, &made[1]), "closed");
    assert!(!ability.allows(Update, &made[2]), "Bob's");
    assert!(ability.allows_action("read", &made[2]));
    assert!(ability.authorize(Update, &made[0]).is_ok());
    let denied = ability.authorize(Update, &made[2]).unwrap_err();
    assert_eq!(denied.status, http::StatusCode::FORBIDDEN);

    // The record as conditions see it: hyphenated UUIDs, RFC 3339 with
    // microseconds.
    let json = record_json(&made[0]);
    assert_eq!(json["owner_id"], json!(alice.to_string()));
    let created = json["created_at"].as_str().unwrap();
    let fraction = created
        .rsplit('.')
        .next()
        .and_then(|rest| rest.strip_suffix('Z'))
        .unwrap();
    assert_eq!(fraction.len(), 6, "microseconds, UTC: {created}");
    assert_eq!(json["closed_at"], Value::Null);
    let _ = ModelColumns::<Ticket>::new();
}

/// One rule over a ticket field, for the property test.
#[derive(Clone, Debug)]
enum Condition {
    StatusEq(&'static str),
    StatusIn(Vec<&'static str>),
    PriorityGt(i64),
    PriorityLte(i64),
    OwnerEq(usize),
    OwnerNe(usize),
    ClosedBefore(i64),
    Unknown,
}

fn condition() -> impl Strategy<Value = Condition> {
    prop_oneof![
        prop::sample::select(vec!["open", "closed", "waiting"]).prop_map(Condition::StatusEq),
        prop::sample::subsequence(vec!["open", "closed", "waiting"], 0..3).prop_map(Condition::StatusIn),
        (0i64..6).prop_map(Condition::PriorityGt),
        (0i64..6).prop_map(Condition::PriorityLte),
        (0usize..3).prop_map(Condition::OwnerEq),
        (0usize..3).prop_map(Condition::OwnerNe),
        (-3i64..3).prop_map(Condition::ClosedBefore),
        Just(Condition::Unknown),
    ]
}

/// A rule: allow or deny, with zero to two conditions.
fn rule() -> impl Strategy<Value = (bool, Vec<Condition>)> {
    (any::<bool>(), prop::collection::vec(condition(), 0..3))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn a_record_check_and_a_list_filter_never_disagree(
        rules in prop::collection::vec(rule(), 1..5),
        statuses in prop::collection::vec(prop::sample::select(vec!["open", "closed", "waiting"]), 6),
        priorities in prop::collection::vec(1i64..6, 6),
        owners in prop::collection::vec(0usize..4, 6),
        closed in prop::collection::vec(prop::option::of(-3i64..3), 6),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let test = sqlite().await;
            let users = [Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7()];
            let base = Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
            let mut built = Rules::new();
            for (allow, conditions) in &rules {
                let mut all = navree_access::Conditions::new();
                // One condition per field and operator (rules refuse more).
                let mut used = Vec::new();
                for condition in conditions {
                    let kind = std::mem::discriminant(condition);
                    if used.contains(&kind) {
                        continue;
                    }
                    used.push(kind);
                    all = all.and(match condition {
                        Condition::StatusEq(status) => field("status").eq(*status),
                        Condition::StatusIn(statuses) => field("status").is_in(statuses.clone()),
                        Condition::PriorityGt(n) => field("priority").gt(*n),
                        Condition::PriorityLte(n) => field("priority").lte(*n),
                        Condition::OwnerEq(i) => field("owner_id").eq(users[*i]),
                        Condition::OwnerNe(i) => field("owner_id").ne(users[*i]),
                        Condition::ClosedBefore(days) => field("closed_at").lt(base + Duration::days(*days)),
                        Condition::Unknown => field("not_a_column").eq("x"),
                    });
                }
                let builder = if *allow { built.can(READ, "Ticket") } else { built.cannot(READ, "Ticket") };
                if !conditions.is_empty() {
                    builder.when(all);
                }
            }
            let ability = built.try_build().unwrap();
            let mut records = Vec::new();
            for index in 0..6 {
                let owner = users.get(owners[index]).copied();
                records.push(
                    Ticket::factory()
                        .set(Ticket::STATUS, statuses[index])
                        .set(Ticket::PRIORITY, priorities[index] as i32)
                        .set(Ticket::OWNER_ID, owner)
                        .set(Ticket::CLOSED_AT, closed[index].map(|days| base + Duration::days(days)))
                        .create_one(&test.db)
                        .await
                        .unwrap(),
                );
            }
            let listed = Ticket::query()
                .accessible(&ability, Read)
                .pluck(&test.db, Ticket::ID)
                .await
                .unwrap();
            for record in &records {
                let checked = ability.allows(Read, record);
                let filtered = listed.contains(&record.id);
                prop_assert_eq!(checked, filtered, "ticket {} with rules {:?}", record_json(record), rules);
            }
            Ok(())
        })?;
    }
}

struct Routes;

impl Injectable for Routes {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Routes) })
    }
}

impl Controller for Routes {
    fn prefix(&self) -> &'static str {
        "/tickets"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", |tickets: Accessible<Ticket, Read>, uow: navree_db::Uow| async move {
                let count = tickets.query().count(&uow).await?;
                Ok::<_, AppError>(Json(json!({ "count": count })))
            })
            .authenticated(),
            RouteDef::on(
                Method::PATCH,
                "/{ticket}",
                |ticket: Authorized<Ticket, Update>, uow: navree_db::Uow| async move {
                    let mut ticket: Record<Ticket> = ticket.into_inner();
                    ticket.update(&uow, |t| t.title = "Edited".to_owned()).await?;
                    Ok::<_, AppError>(Json(json!({ "title": ticket.title })))
                },
            )
            .can(UPDATE, "Ticket", Some("ticket")),
        ]
    }
}

struct TicketsModule;

impl Module for TicketsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<Ticket>())
            .controller::<Routes>();
    }
}

#[tokio::test]
async fn routes_load_and_authorize_model_records() {
    let (app, db, _clock) = app(vec![
        Box::new(AccessModule::for_root(AccessOptions::default())),
        Box::new(TicketsModule),
    ])
    .await;
    let (alice, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let made = tickets(&db, alice, bob).await;

    // The policy attached to the model joined Alice's rules.
    app.acting_as(principal(alice))
        .get("/api/tickets")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("count", json!(4));
    app.acting_as(principal(alice))
        .patch(&format!("/api/tickets/{}", made[0].id))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Edited"));
    app.acting_as(principal(alice))
        .patch(&format!("/api/tickets/{}", made[2].id))
        .send()
        .await
        .assert_error(403, None);
    app.acting_as(principal(alice))
        .patch(&format!("/api/tickets/{}", made[1].id))
        .send()
        .await
        .assert_error(403, None);
    app.acting_as(principal(alice))
        .patch(&format!("/api/tickets/{}", Uuid::now_v7()))
        .send()
        .await
        .assert_error(404, None);
    app.acting_as(principal(alice))
        .patch("/api/tickets/not-a-uuid")
        .send()
        .await
        .assert_error(404, None);
    assert_eq!(<Ticket as Model>::table_name(), "tickets");
}
