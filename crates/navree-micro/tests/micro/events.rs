//! Event handlers with `MicroFake`: delivery, the actor, retries, dead
//! letters, the inbox, wildcards, broadcast, ordering, forwarding and the
//! outbox.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_db::{DatabaseModule, Db, Executor, ExecutorExt, ModuleBuilderDbExt, Statement, Uow};
use navree_events::{EventBus, EventsModule, Listener, ModuleBuilderEventsExt};
use navree_http::{AppError, Result};
use navree_kernel::{AppContext, BoxError, BoxFuture, Module, ModuleBuilder};
use navree_macros::{injectable, message_handlers};
use navree_micro::{
    DeadLetters, MessageContext, MicroFake, MicroModule, MicroOptions, ModuleBuilderMicroExt,
    Payload, ServiceClient, headers,
};
use navree_testing::TestApp;
use navree_validation::Valid;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::databases::Scratch;
use crate::on_every_server;
use crate::support::{Counter, DealRenamed, NewNote, insert_note, notes};

/// What the handlers saw, in order.
#[derive(Debug, Default)]
pub struct Seen(Mutex<Vec<String>>);

impl Seen {
    fn push(&self, value: impl Into<String>) {
        self.0.lock().unwrap().push(value.into());
    }

    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// Whether `jobs.flaky` succeeds.
#[derive(Debug, Default)]
pub struct Switch(AtomicBool);

/// The alerts service's event handlers.
#[injectable]
pub struct Alerts {
    counter: Arc<Counter>,
    seen: Arc<Seen>,
    switch: Arc<Switch>,
    client: Arc<ServiceClient>,
}

#[message_handlers]
impl Alerts {
    #[event_pattern("note.added")]
    async fn added(
        &self,
        uow: Uow,
        msg: MessageContext,
        Payload(note): Payload<NewNote>,
    ) -> Result<()> {
        insert_note(&uow, &note.body).await?;
        self.seen.push(format!(
            "added {} by {}",
            note.body,
            msg.actor().map(|a| a.to_string()).unwrap_or_default()
        ));
        Ok(())
    }

    #[event_pattern("jobs.flaky", max_attempts = 3)]
    async fn flaky(&self, msg: MessageContext) -> Result<()> {
        self.counter.bump();
        self.seen.push(format!("attempt {}", msg.attempt()));
        if self.switch.0.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(AppError::internal("smtp refused the message"))
        }
    }

    #[event_pattern("note.strict")]
    async fn strict(&self, Valid(Payload(note)): Valid<Payload<NewNote>>) -> Result<()> {
        self.seen.push(note.body);
        Ok(())
    }

    #[event_pattern("audit.*")]
    async fn any_audit(&self, msg: MessageContext, Payload(_event): Payload<Value>) -> Result<()> {
        self.seen.push(msg.pattern());
        Ok(())
    }

    #[event_pattern("cache.cleared", broadcast)]
    async fn cache_cleared(&self) -> Result<()> {
        self.seen.push("cache cleared");
        Ok(())
    }

    #[event_pattern("deal.updated")]
    async fn deal_updated(&self, Payload(n): Payload<u64>) -> Result<()> {
        // Later events finish faster unless they wait for their key.
        tokio::time::sleep(Duration::from_millis(20u64.saturating_sub(n * 2))).await;
        self.seen.push(format!("updated {n}"));
        Ok(())
    }

    #[event_pattern("deal.ordered", ordered)]
    async fn deal_ordered(&self, Payload(n): Payload<u64>) -> Result<()> {
        if n == 1 && self.counter.bump() < 2 {
            // Fails once: the events behind it wait.
            return Err(AppError::internal("not yet"));
        }
        self.seen.push(format!("ordered {n}"));
        Ok(())
    }

    #[message_pattern("alerts.fail_loudly")]
    #[public]
    async fn fail_loudly(&self) -> Result<()> {
        self.client
            .emit("security.alert", &json!({}))
            .immediately()
            .await?;
        self.client.emit("security.quiet", &json!({})).await?;
        Err(AppError::conflict("rolled back"))
    }

    /// Writes a note and emits an event in the same unit of work.
    #[message_pattern("alerts.announce")]
    #[public]
    async fn announce(&self, uow: Uow, Payload(note): Payload<NewNote>) -> Result<()> {
        insert_note(&uow, &note.body).await?;
        self.client
            .emit("alerts.announced", &NewNote { body: note.body })
            .await?;
        Ok(())
    }
}

/// The alerts feature.
pub struct AlertsModule;

impl Module for AlertsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(Counter::default())
            .value(Seen::default())
            .value(Switch::default())
            .migrations(
                navree_db::MigrationSet::new("deals")
                    .migration(crate::support::CreateTables::migration()),
            )
            .message_handlers::<Alerts>();
    }
}

async fn alerts_app(micro: &MicroFake, scratch: &Scratch) -> TestApp {
    alerts_app_with(micro, scratch, |options| options).await
}

/// The alerts service on the scratch database, its units of work on
/// navree-db's own transaction.
async fn alerts_app_with(
    micro: &MicroFake,
    scratch: &Scratch,
    options: impl FnOnce(MicroOptions) -> MicroOptions,
) -> TestApp {
    TestApp::builder()
        .config("database.url", scratch.url.as_str())
        .config("database.migrate_on_boot", true)
        .config("database.native_units_of_work", true)
        .module(DatabaseModule)
        .module(MicroModule::for_root(options(MicroOptions {
            service: "alerts".into(),
            events: navree_micro::EventsOptions {
                // `DeadLetters::retry` runs in this process: no waiting.
                backoff: navree_contracts::Backoff::Fixed { secs: 0 },
                ..navree_micro::EventsOptions::default()
            },
            ..MicroOptions::from(micro.transport())
        })))
        .module(AlertsModule)
        .build()
        .await
}

async fn events_reach_their_handler_with_the_actor(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app(&micro, &scratch).await;
    let rep = Uuid::now_v7();
    micro
        .client()
        .emit("note.added", &NewNote { body: "won".into() })
        .header(headers::ACTOR, rep.to_string())
        .await
        .unwrap();
    // The handler's unit of work has the actor, and it committed.
    assert_eq!(
        notes(&app).await,
        [("won".to_owned(), Some(rep.to_string()))]
    );
    assert_eq!(
        app.get::<Seen>().await.all(),
        [format!("added won by {rep}")]
    );
    assert!(micro.dead_letters().is_empty());
    app.shutdown().await.unwrap();
}

async fn a_redelivered_event_is_skipped_by_the_inbox(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app(&micro, &scratch).await;
    micro
        .client()
        .emit(
            "note.added",
            &NewNote {
                body: "once".into(),
            },
        )
        .await
        .unwrap();
    micro
        .client()
        .emit(
            "note.added",
            &NewNote {
                body: "once".into(),
            },
        )
        .redelivered()
        .await
        .unwrap();
    assert_eq!(notes(&app).await.len(), 1, "handled once");
    // A new message with the same payload is a new event.
    micro
        .client()
        .emit(
            "note.added",
            &NewNote {
                body: "once".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(notes(&app).await.len(), 2);
    app.shutdown().await.unwrap();
}

async fn failed_events_are_retried_then_dead_lettered_and_retried_by_hand(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app(&micro, &scratch).await;
    micro.client().emit("jobs.flaky", &json!({})).await.unwrap();
    let seen = app.get::<Seen>().await;
    assert_eq!(
        seen.all(),
        ["attempt 1", "attempt 2", "attempt 3"],
        "max_attempts = 3"
    );
    let letters = micro.dead_letters();
    assert_eq!(letters.len(), 1);
    assert_eq!(letters[0].pattern, "jobs.flaky");
    assert_eq!(letters[0].attempts, 3);
    assert_eq!(letters[0].error, "smtp refused the message");

    // Stored in the database, for micro:failed, micro:retry and micro:flush.
    let dead = app.get::<DeadLetters>().await;
    let stored = dead.list().await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].message_id, letters[0].message_id);
    app.run_command("micro:failed", &[]).await.unwrap();

    // Retrying while it still fails puts it back.
    assert!(!dead.retry(&stored[0].id).await.unwrap());
    let again = dead.list().await.unwrap();
    assert_eq!(again.len(), 1);
    assert_ne!(again[0].id, stored[0].id);

    // Fixed: the retry handles it, with its attempts starting over.
    app.get::<Switch>().await.0.store(true, Ordering::SeqCst);
    app.run_command("micro:retry", &["all"]).await.unwrap();
    assert!(dead.list().await.unwrap().is_empty());
    assert_eq!(seen.all().last().map(String::as_str), Some("attempt 1"));

    // micro:flush deletes what is left.
    app.get::<Switch>().await.0.store(false, Ordering::SeqCst);
    micro.client().emit("jobs.flaky", &json!({})).await.unwrap();
    assert_eq!(dead.list().await.unwrap().len(), 1);
    app.run_command("micro:flush", &[]).await.unwrap();
    assert!(dead.list().await.unwrap().is_empty());
    app.shutdown().await.unwrap();
}

async fn events_that_cannot_succeed_are_dead_lettered_at_once(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app(&micro, &scratch).await;
    micro
        .client()
        .emit(
            "note.strict",
            &NewNote {
                body: String::new(),
            },
        )
        .await
        .unwrap();
    micro
        .client()
        .emit("note.strict", &json!({ "nope": 1 }))
        .await
        .unwrap();
    let letters = micro.dead_letters();
    assert_eq!(letters.len(), 2);
    assert!(
        letters.iter().all(|letter| letter.attempts == 1),
        "{letters:?}"
    );
    assert!(app.get::<Seen>().await.all().is_empty());
    app.shutdown().await.unwrap();
}

async fn dead_letters_can_go_to_the_log_instead(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app_with(&micro, &scratch, |mut options| {
        options.events.dead_letter = Some(navree_micro::DeadLetterMode::Log);
        options
    })
    .await;
    micro.client().emit("jobs.flaky", &json!({})).await.unwrap();
    assert_eq!(
        micro.dead_letters().len(),
        1,
        "the fake sees every dead letter"
    );
    assert!(
        app.get::<DeadLetters>()
            .await
            .list()
            .await
            .unwrap()
            .is_empty()
    );
    app.shutdown().await.unwrap();
}

async fn wildcard_handlers_see_the_actual_pattern(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app(&micro, &scratch).await;
    for pattern in ["audit.login", "audit.logout", "audit.deep.nested"] {
        micro.client().emit(pattern, &json!({})).await.unwrap();
    }
    assert_eq!(
        app.get::<Seen>().await.all(),
        ["audit.login", "audit.logout"]
    );
    app.shutdown().await.unwrap();
}

async fn instances_share_events_and_broadcast_reaches_each(scratch: Scratch) {
    let micro = MicroFake::new();
    // Two instances of the service on one database.
    let one = alerts_app(&micro, &scratch).await;
    let two = alerts_app(&micro, &scratch).await;

    micro
        .client()
        .emit("cache.cleared", &json!({}))
        .await
        .unwrap();
    assert_eq!(one.get::<Seen>().await.all(), ["cache cleared"]);
    assert_eq!(two.get::<Seen>().await.all(), ["cache cleared"]);

    for n in 0..4 {
        micro
            .client()
            .emit(
                "note.added",
                &NewNote {
                    body: format!("n{n}"),
                },
            )
            .await
            .unwrap();
    }
    // The database is shared: its notes are the sum of both instances'.
    assert_eq!(
        notes(&one).await.len(),
        4,
        "each event handled by one instance"
    );
    assert_eq!(notes(&two).await.len(), 4);
    one.shutdown().await.unwrap();
    two.shutdown().await.unwrap();
}

async fn events_with_one_key_are_handled_in_order(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app(&micro, &scratch).await;
    let client = micro.client();
    let mut sends = Vec::new();
    for n in 0..8u64 {
        sends.push(client.emit("deal.updated", &n).key("deal-1"));
    }
    let mut waits = Vec::new();
    for send in sends {
        waits.push(tokio::spawn(send.into_future()));
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    for wait in waits {
        wait.await.unwrap().unwrap();
    }
    let expected: Vec<String> = (0..8).map(|n| format!("updated {n}")).collect();
    assert_eq!(app.get::<Seen>().await.all(), expected);
    app.shutdown().await.unwrap();
}

async fn ordered_handlers_hold_the_events_behind_a_failing_one(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app(&micro, &scratch).await;
    let client = micro.client();
    let mut waits = Vec::new();
    for n in 0..4u64 {
        waits.push(tokio::spawn(client.emit("deal.ordered", &n).into_future()));
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    for wait in waits {
        wait.await.unwrap().unwrap();
    }
    assert_eq!(
        app.get::<Seen>().await.all(),
        ["ordered 0", "ordered 1", "ordered 2", "ordered 3"]
    );
    app.shutdown().await.unwrap();
}

/// The `micro` migration set keeps its name, so databases that ran it
/// before it moved to the schema builder see nothing pending, and its tables
/// and indexes exist.
async fn the_micro_migration_keeps_its_name(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app(&micro, &scratch).await;
    let status = navree_db::Migrator::status(app.ctx()).await.unwrap();
    let ours: Vec<_> = status.iter().filter(|m| m.module == "micro").collect();
    assert_eq!(ours.len(), 1, "{status:?}");
    assert_eq!(ours[0].name, "m20261026_000001_create_micro_tables");
    assert!(ours[0].applied);
    let db = app.get::<Db>().await;
    let schema = navree_db::navree_schema::Schema::new(&*db);
    for (table, index) in [
        ("micro_outbox", ["created_at"].as_slice()),
        ("micro_inbox", ["created_at"].as_slice()),
        ("micro_dead_letters", ["service", "failed_at"].as_slice()),
    ] {
        assert!(schema.has_table(table).await.unwrap(), "{table}");
        assert!(
            schema.has_index(table, index, None).await.unwrap(),
            "{table} {index:?}"
        );
    }
    app.shutdown().await.unwrap();
}

/// The transactional outbox: the event is written with the note in one
/// transaction, published after the commit and removed from the outbox.
async fn events_emitted_in_a_unit_of_work_are_published_after_it_commits(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app(&micro, &scratch).await;
    micro
        .client()
        .request(
            "alerts.announce",
            &NewNote {
                body: "hello".into(),
            },
        )
        .await
        .assert_status(200);
    micro.assert_emitted("alerts.announced", |note: &NewNote| note.body == "hello");
    assert_eq!(notes(&app).await.len(), 1);
    let db = app.get::<Db>().await;
    let waiting: i64 = db
        .fetch_scalar(&Statement::text("SELECT COUNT(*) FROM micro_outbox"))
        .await
        .unwrap();
    assert_eq!(waiting, 0, "published rows leave the outbox");
    // The handled request left its inbox row (`dedupe` is off: no reply).
    app.shutdown().await.unwrap();
}

async fn immediately_publishes_even_when_the_unit_of_work_rolls_back(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app(&micro, &scratch).await;
    micro
        .client()
        .request("alerts.fail_loudly", &())
        .await
        .assert_error(409, "rolled back");
    micro.assert_emitted_times("security.alert", 1);
    micro.assert_emitted_times("security.quiet", 0);
    app.shutdown().await.unwrap();
}

async fn the_relay_publishes_what_a_crashed_process_left_in_the_outbox(scratch: Scratch) {
    let micro = MicroFake::new();
    let app = alerts_app_with(&micro, &scratch, |mut options| {
        options.outbox_poll_ms = 20;
        options
    })
    .await;
    let db = app.get::<Db>().await;
    let headers = json!({
        "navree-pattern": "deal.renamed",
        "navree-message-id": "0199c1d2-6f7e-7a31-b5d4-2f0c8e9a1b23",
        "content-type": "application/json",
    })
    .to_string();
    let old = chrono::Utc::now().timestamp_millis() - 60_000;
    db.execute(&Statement::portable(
        "INSERT INTO micro_outbox (id, pattern, headers, payload, seq, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        [
            "0199c1d2-6f7e-7a31-b5d4-2f0c8e9a1b23".into(),
            "deal.renamed".into(),
            headers.into(),
            br#"{"deal_id":"0199c1d2-6f7e-7a31-b5d4-2f0c8e9a1b24","title":"Left behind"}"#
                .to_vec()
                .into(),
            1i64.into(),
            old.into(),
        ],
    ))
    .await
    .unwrap();
    for _ in 0..300 {
        if !micro.emitted_envelopes("deal.renamed").is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    micro.assert_emitted("deal.renamed", |event: &DealRenamed| {
        event.title == "Left behind"
    });
    let left: i64 = db
        .fetch_scalar(&Statement::text("SELECT COUNT(*) FROM micro_outbox"))
        .await
        .unwrap();
    assert_eq!(left, 0, "published rows are deleted");
    app.shutdown().await.unwrap();
}

/// Records every `DealRenamed` on the local bus.
#[injectable]
pub struct LocalRenames {
    seen: Arc<Seen>,
}

impl Listener<DealRenamed> for LocalRenames {
    fn handle<'a>(
        &'a self,
        event: &'a DealRenamed,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, std::result::Result<(), BoxError>> {
        self.seen.push(format!("local {}", event.title));
        Box::pin(async { Ok(()) })
    }
}

/// Forwards and receives `DealRenamed`.
struct BridgeModule {
    keyed: bool,
}

impl Module for BridgeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(Seen::default())
            .listen::<DealRenamed, LocalRenames>();
        if self.keyed {
            m.forward_event_keyed::<DealRenamed, _, _>(|event| event.deal_id);
        } else {
            m.forward_event::<DealRenamed>()
                .receive_event::<DealRenamed>();
        }
    }
}

#[tokio::test]
async fn in_process_events_are_forwarded_and_received() {
    let micro = MicroFake::new();
    let app = TestApp::builder()
        .module(EventsModule::for_root())
        .module(MicroModule::for_root(micro.transport()))
        .module(BridgeModule { keyed: false })
        .build()
        .await;
    let bus = app.get::<EventBus>().await;
    let deal_id = Uuid::now_v7();
    bus.emit(DealRenamed {
        deal_id,
        title: "Forwarded".into(),
    })
    .await
    .unwrap();
    micro.assert_emitted("deal.renamed", |event: &DealRenamed| {
        event.deal_id == deal_id
    });
    assert_eq!(micro.emitted_envelopes("deal.renamed")[0].key(), None);

    // receive_event: an event published anywhere reaches the local bus.
    micro
        .client()
        .emit(
            "deal.renamed",
            &DealRenamed {
                deal_id,
                title: "From elsewhere".into(),
            },
        )
        .await
        .unwrap();
    let seen = app.get::<Seen>().await.all();
    assert!(
        seen.contains(&"local From elsewhere".to_owned()),
        "{seen:?}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn forwarded_events_can_carry_a_key() {
    let micro = MicroFake::new();
    let app = TestApp::builder()
        .module(EventsModule::for_root())
        .module(MicroModule::for_root(micro.transport()))
        .module(BridgeModule { keyed: true })
        .build()
        .await;
    let deal_id = Uuid::now_v7();
    app.get::<EventBus>()
        .await
        .emit(DealRenamed {
            deal_id,
            title: "Keyed".into(),
        })
        .await
        .unwrap();
    let sent = micro.emitted_envelopes("deal.renamed");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].key(), Some(deal_id.to_string().as_str()));
    app.shutdown().await.unwrap();
}

on_every_server!(
    "navree_micro";
    events_reach_their_handler_with_the_actor,
    a_redelivered_event_is_skipped_by_the_inbox,
    failed_events_are_retried_then_dead_lettered_and_retried_by_hand,
    events_that_cannot_succeed_are_dead_lettered_at_once,
    dead_letters_can_go_to_the_log_instead,
    wildcard_handlers_see_the_actual_pattern,
    instances_share_events_and_broadcast_reaches_each,
    events_with_one_key_are_handled_in_order,
    ordered_handlers_hold_the_events_behind_a_failing_one,
    immediately_publishes_even_when_the_unit_of_work_rolls_back,
    the_relay_publishes_what_a_crashed_process_left_in_the_outbox,
    events_emitted_in_a_unit_of_work_are_published_after_it_commits,
    the_micro_migration_keeps_its_name,
);
