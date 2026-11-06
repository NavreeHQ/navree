//! The delivery tables on every database: SQLite always, PostgreSQL with
//! TimescaleDB (`--features test-postgres`, `NAVREE_TEST_POSTGRES_URL`) and
//! MySQL (`--features test-mysql`, `NAVREE_TEST_MYSQL_URL`).
//!
//! Each scenario runs in a database of its own (created on the server and
//! dropped afterwards): the migration, the delivery log and its events
//! (also recorded at the same time for one message), suppressions, tenant
//! transports, and the outbox (also released by several admins at once:
//! the message goes out once).
//!
//! ```text
//! cargo test -p navree-mail --features test-postgres,test-mysql --test mail servers
//! ```

use std::sync::atomic::{AtomicU32, Ordering};

use navree_db::record::{self, Filter};
use navree_db::{
    Backend, ChangeOp, Db, DbOptions, Executor, ExecutorExt, PageQuery, Pool, Record as _,
    Statement,
};
use navree_mail::entity::{DeliveryRow, EventRow, OutboxRow, SuppressionRow, TransportRow};
use navree_mail::{
    Deliveries, DeliveryFilter, DeliveryStatus, MailError, MailEventKind, MailScope, Mailable,
    Outbox, OutboxStatus, ProviderEvent, SuppressionReason, Suppressions, TemplateMail,
    TransportConfig, TransportOwner, TransportResolver, TransportTier,
};
use serde_json::json;
use uuid::Uuid;

use crate::harness::{GrantAuthorizer, MailApp, builder};

/// Where a scenario runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Server {
    Sqlite,
    Postgres,
    MySql,
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A database of its own on a server, dropped by [`Scratch::drop`].
struct Scratch {
    admin: Pool,
    name: String,
    url: String,
}

impl Scratch {
    async fn new(server: Server) -> Option<Scratch> {
        let (variable, compiled) = match server {
            Server::Sqlite => return None,
            Server::Postgres => ("NAVREE_TEST_POSTGRES_URL", cfg!(feature = "test-postgres")),
            Server::MySql => ("NAVREE_TEST_MYSQL_URL", cfg!(feature = "test-mysql")),
        };
        let admin_url = match std::env::var(variable) {
            Ok(url) if compiled && !url.is_empty() => url,
            _ => {
                assert!(
                    std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                    "{server:?}: {variable} is not set (or its feature is off) and NAVREE_TEST_REQUIRE_SERVICES=1"
                );
                eprintln!("skipped on {server:?}: set {variable} and turn its feature on");
                return None;
            }
        };
        let name = format!(
            "navree_mail_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let admin = Pool::connect(&DbOptions {
            url: admin_url.clone(),
            max_connections: 1,
            ..DbOptions::default()
        })
        .await
        .expect("the admin URL connects");
        admin
            .execute_script(&format!("CREATE DATABASE {name}"))
            .await
            .expect("the test user may create databases");
        let (base, query) = match admin_url.split_once('?') {
            Some((base, query)) => (base, format!("?{query}")),
            None => (admin_url.as_str(), String::new()),
        };
        let server_part = base.rsplit_once('/').map_or(base, |(server, _)| server);
        let url = format!("{server_part}/{name}{query}");
        Some(Scratch { admin, name, url })
    }

    async fn drop(self) {
        let force = if self.admin.backend() == Backend::Postgres {
            " WITH (FORCE)"
        } else {
            ""
        };
        if let Err(error) = self
            .admin
            .execute_script(&format!("DROP DATABASE IF EXISTS {}{force}", self.name))
            .await
        {
            eprintln!("could not drop {}: {error}", self.name);
        }
        self.admin.close().await;
    }
}

/// Runs `scenario` on a mail app on `server`.
pub async fn run<F, Fut>(server: Server, scenario: F)
where
    F: FnOnce(MailApp) -> Fut,
    Fut: Future<Output = ()>,
{
    let scratch = match server {
        Server::Sqlite => None,
        _ => match Scratch::new(server).await {
            Some(scratch) => Some(scratch),
            None => return,
        },
    };
    let fake = navree_mail::MailFake::new();
    let authorizer = std::sync::Arc::new(GrantAuthorizer::default());
    let mut app = builder(&fake, &authorizer).config("database.max_connections", 8);
    if let Some(scratch) = &scratch {
        app = app.config("database.url", scratch.url.as_str());
    }
    let app = app.build().await;
    let db = app.get::<Db>().await;
    let expected = match server {
        Server::Sqlite => Backend::Sqlite,
        Server::Postgres => Backend::Postgres,
        Server::MySql => Backend::MySql,
    };
    assert_eq!(Executor::backend(&*db), expected);
    let outcome =
        futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(scenario(MailApp {
            app,
            fake,
            authorizer,
        })))
        .await;
    if let Some(scratch) = scratch {
        scratch.drop().await;
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// One test per server for each scenario.
macro_rules! on_every_server {
    ($($scenario:ident),* $(,)?) => {
        $(
            mod $scenario {
                use super::{Server, run};

                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn sqlite() {
                    run(Server::Sqlite, super::$scenario).await;
                }

                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn postgres() {
                    run(Server::Postgres, super::$scenario).await;
                }

                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn mysql() {
                    run(Server::MySql, super::$scenario).await;
                }
            }
        )*
    };
}

on_every_server!(
    migration_creates_the_tables,
    delivery_log,
    events_for_one_message_at_the_same_time,
    suppressions,
    tenant_transports,
    outbox,
    released_by_several_admins_at_once,
);

fn welcome(to: &str) -> TemplateMail {
    TemplateMail::new(to, "Welcome", "mail/welcome").context(json!({ "name": "Anu" }))
}

/// The migration made the five tables, and each reads back empty.
async fn migration_creates_the_tables(mail: MailApp) {
    let db = mail.get::<Db>().await;
    assert_eq!(TransportRow::select().count(&*db).await.unwrap(), 0);
    assert_eq!(DeliveryRow::select().count(&*db).await.unwrap(), 0);
    assert_eq!(EventRow::select().count(&*db).await.unwrap(), 0);
    assert_eq!(SuppressionRow::select().count(&*db).await.unwrap(), 0);
    assert_eq!(OutboxRow::select().count(&*db).await.unwrap(), 0);
    let recorded: i64 = db
        .fetch_scalar(&Statement::portable(
            "SELECT COUNT(*) FROM navree_migrations WHERE module = ? AND name = ?",
            ["mail".into(), "m20261027_000001_create_mail_tables".into()],
        ))
        .await
        .unwrap();
    assert_eq!(recorded, 1, "the migration keeps its name");
}

/// Sending logs a delivery; filters, pages, provider events, first opens
/// and clicks, failures; the rows read back as written.
async fn delivery_log(mail: MailApp) {
    let workspace = Uuid::now_v7();
    let mailer = mail.mailer().await;
    let mut ids = Vec::new();
    for n in 0..5 {
        let scope = if n % 2 == 0 {
            MailScope::workspace(workspace)
        } else {
            MailScope::default()
        };
        let report = mailer
            .for_scope(&scope)
            .deliver(&welcome(&format!("Person {n} <P{n}@Example.com>")))
            .await
            .unwrap();
        assert_eq!(report.status, DeliveryStatus::Sent);
        ids.push(report.delivery_id.unwrap());
    }
    let deliveries = mail.get::<Deliveries>().await;
    let first = deliveries.find(ids[0]).await.unwrap();
    assert_eq!(first.status, DeliveryStatus::Sent);
    assert_eq!(first.workspace_id, Some(workspace));
    assert_eq!(first.transport, "fake");
    assert_eq!(first.tier, TransportTier::Config);
    assert_eq!(first.template.as_deref(), Some("mail/welcome"));
    assert_eq!(first.recipients, 1);
    let provider_id = first.provider_id.clone().unwrap();
    assert_eq!(
        deliveries
            .find_by_provider_id(&provider_id)
            .await
            .unwrap()
            .map(|d| d.id),
        Some(ids[0])
    );
    assert!(matches!(
        deliveries.find(Uuid::now_v7()).await,
        Err(MailError::NotFound(_))
    ));

    let in_workspace = deliveries
        .list(
            &DeliveryFilter {
                workspace_id: Some(workspace),
                ..DeliveryFilter::default()
            },
            PageQuery::new(1, 2),
        )
        .await
        .unwrap();
    assert_eq!(in_workspace.meta.total, 3);
    assert_eq!(in_workspace.meta.last_page, 2);
    assert_eq!(in_workspace.data[0].id, ids[4], "newest first");
    let to_one = deliveries
        .list(
            &DeliveryFilter {
                recipient: Some("p3@example.com".to_owned()),
                status: Some(DeliveryStatus::Sent),
                template: Some("mail/welcome".to_owned()),
                ..DeliveryFilter::default()
            },
            PageQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        to_one.data.iter().map(|d| d.id).collect::<Vec<_>>(),
        [ids[3]]
    );

    // Delivered, opened twice, clicked: the first open and click stick.
    let event = |kind| {
        let mut event = ProviderEvent::new("ses", provider_id.clone(), kind);
        event.recipient = Some("p0@example.com".to_owned());
        event
    };
    let delivered = deliveries
        .record(event(MailEventKind::Delivered))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivered.status, DeliveryStatus::Delivered);
    let opened = deliveries
        .record(event(MailEventKind::Opened))
        .await
        .unwrap()
        .unwrap();
    let opened_at = opened.opened_at.expect("the first open is stamped");
    let clicked = deliveries
        .record(event(MailEventKind::Clicked).url("https://example.com/a"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        clicked.opened_at,
        Some(opened_at),
        "a later open changes nothing"
    );
    assert!(clicked.clicked_at.is_some());
    let reread = deliveries.find(ids[0]).await.unwrap();
    assert_eq!(reread.status, DeliveryStatus::Delivered);
    assert_eq!(reread.opened_at, clicked.opened_at);
    assert_eq!(reread.clicked_at, clicked.clicked_at);
    let events = deliveries.events(ids[0]).await.unwrap();
    assert_eq!(
        events.iter().map(|e| e.kind).collect::<Vec<_>>(),
        [
            MailEventKind::Delivered,
            MailEventKind::Opened,
            MailEventKind::Clicked
        ]
    );
    assert_eq!(events[2].url.as_deref(), Some("https://example.com/a"));

    // A hard bounce suppresses the address; the next message to it is
    // logged as suppressed and not sent.
    let mut bounce = ProviderEvent::new(
        "ses",
        deliveries.find(ids[2]).await.unwrap().provider_id.unwrap(),
        MailEventKind::Bounced,
    )
    .permanent()
    .detail("550 5.1.1 user unknown");
    bounce.recipient = Some("p2@example.com".to_owned());
    let bounced = deliveries.record(bounce).await.unwrap().unwrap();
    assert_eq!(bounced.status, DeliveryStatus::Bounced);
    let report = mailer
        .for_scope(&MailScope::workspace(workspace))
        .deliver(&welcome("p2@example.com"))
        .await
        .unwrap();
    assert_eq!(report.status, DeliveryStatus::Suppressed);
    let suppressed = deliveries.find(report.delivery_id.unwrap()).await.unwrap();
    assert_eq!(suppressed.status, DeliveryStatus::Suppressed);
    assert_eq!(suppressed.transport, "none");

    // An event for an unknown message is still logged.
    assert!(
        deliveries
            .record(ProviderEvent::new(
                "ses",
                "unknown",
                MailEventKind::Delivered
            ))
            .await
            .unwrap()
            .is_none()
    );
    let db = mail.get::<Db>().await;
    assert_eq!(
        EventRow::select()
            .filter(Filter::all().is_null("delivery_id"))
            .count(&*db)
            .await
            .unwrap(),
        1
    );
}

/// Opens, clicks and a complaint about one message recorded at the same
/// time: every event is logged, the first open and click are stamped once,
/// and the complaint wins over the delivery report whatever the order.
async fn events_for_one_message_at_the_same_time(mail: MailApp) {
    let report = mail
        .mailer()
        .await
        .deliver(&welcome("anu@example.com"))
        .await
        .unwrap();
    let id = report.delivery_id.unwrap();
    let deliveries = mail.get::<Deliveries>().await;
    let mut tasks = Vec::new();
    for n in 0..12 {
        let deliveries = deliveries.clone();
        let kind = match n % 4 {
            0 => MailEventKind::Opened,
            1 => MailEventKind::Clicked,
            2 => MailEventKind::Delivered,
            _ => MailEventKind::Complained,
        };
        tasks.push(tokio::spawn(async move {
            let mut event = ProviderEvent::new("tracking", "unused", kind);
            event.provider_id = None;
            event.delivery_id = Some(id);
            if kind == MailEventKind::Clicked {
                event = event.url(format!("https://example.com/{n}"));
            }
            deliveries.record(event).await
        }));
    }
    for task in tasks {
        let delivery = task.await.unwrap().expect("every event is recorded");
        assert!(delivery.is_some());
    }
    let delivery = deliveries.find(id).await.unwrap();
    assert_eq!(delivery.status, DeliveryStatus::Complained);
    assert!(delivery.opened_at.is_some() && delivery.clicked_at.is_some());
    let events = deliveries.events(id).await.unwrap();
    assert_eq!(events.len(), 12);
    let first_open = events
        .iter()
        .find(|e| matches!(e.kind, MailEventKind::Opened | MailEventKind::Clicked))
        .unwrap();
    assert_eq!(delivery.opened_at, Some(first_open.occurred_at));
    let first_click = events
        .iter()
        .find(|e| e.kind == MailEventKind::Clicked)
        .unwrap();
    assert_eq!(delivery.clicked_at, Some(first_click.occurred_at));
}

/// Adding twice keeps one row, lists are per workspace, removal by id and
/// by address; writes record change events and roll back with their unit
/// of work.
async fn suppressions(mail: MailApp) {
    let suppressions = mail.get::<Suppressions>().await;
    let workspace = Uuid::now_v7();
    let uow = mail.uow().await;
    let added = suppressions
        .add(
            &uow,
            Some(workspace),
            "Anu <ANU@example.com>",
            SuppressionReason::Manual,
            Some("asked".to_owned()),
        )
        .await
        .unwrap();
    let again = suppressions
        .add(
            &uow,
            Some(workspace),
            "anu@example.com",
            SuppressionReason::Complaint,
            None,
        )
        .await
        .unwrap();
    assert_eq!(again.id, added.id, "an address is suppressed once per list");
    let everywhere = suppressions
        .add(
            &uow,
            None,
            "gauri@example.com",
            SuppressionReason::Unsubscribed,
            None,
        )
        .await
        .unwrap();
    let changes = uow.changes();
    assert_eq!(changes.len(), 2);
    assert!(
        changes
            .iter()
            .all(|c| c.op == ChangeOp::Created && c.entity == "mail_suppressions")
    );
    uow.commit().await.unwrap();

    assert!(
        suppressions
            .is_suppressed(Some(workspace), "anu@example.com")
            .await
            .unwrap()
    );
    assert!(
        !suppressions
            .is_suppressed(None, "anu@example.com")
            .await
            .unwrap()
    );
    assert!(
        suppressions
            .is_suppressed(Some(Uuid::now_v7()), "Gauri <gauri@example.com>")
            .await
            .unwrap(),
        "the app-wide list applies to every workspace"
    );
    let found = suppressions.find(added.id).await.unwrap();
    assert_eq!(found, added);
    assert_eq!(found.email, "anu@example.com");
    assert_eq!(found.reason, SuppressionReason::Manual);
    let listed = suppressions
        .list(Some(workspace), None, PageQuery::default())
        .await
        .unwrap();
    assert_eq!(listed.data, std::slice::from_ref(&added));
    let all = suppressions
        .list(None, None, PageQuery::default())
        .await
        .unwrap();
    assert_eq!(
        all.data.iter().map(|s| s.id).collect::<Vec<_>>(),
        [everywhere.id, added.id]
    );

    // A rolled-back removal leaves the row.
    let uow = mail.uow().await;
    suppressions.remove(&uow, added.id).await.unwrap();
    uow.rollback().await.unwrap();
    assert!(suppressions.find(added.id).await.is_ok());

    let uow = mail.uow().await;
    let removed = suppressions.remove(&uow, added.id).await.unwrap();
    assert_eq!(removed.id, added.id);
    assert!(
        suppressions
            .remove_email(&uow, None, "gauri@example.com")
            .await
            .unwrap()
    );
    assert!(
        !suppressions
            .remove_email(&uow, None, "gauri@example.com")
            .await
            .unwrap()
    );
    assert!(matches!(
        suppressions.remove(&uow, Uuid::now_v7()).await,
        Err(MailError::NotFound(_))
    ));
    assert_eq!(
        uow.changes()
            .iter()
            .filter(|c| c.op == ChangeOp::Deleted)
            .count(),
        2
    );
    uow.commit().await.unwrap();
    assert_eq!(
        suppressions
            .list(None, None, PageQuery::default())
            .await
            .unwrap()
            .meta
            .total,
        0
    );
}

/// Stored transports: replaced in place, encrypted, resolved by tier,
/// turned off and removed.
async fn tenant_transports(mail: MailApp) {
    let resolver = mail.get::<TransportResolver>().await;
    let (workspace, organization) = (Uuid::now_v7(), Uuid::now_v7());
    let set = |owner: TransportOwner, config: TransportConfig| {
        let resolver = resolver.clone();
        let mail = &mail;
        async move {
            let uow = mail.uow().await;
            let stored = resolver.set(&uow, owner, &config).await.unwrap();
            uow.commit().await.unwrap();
            stored
        }
    };
    let ses: TransportConfig =
        serde_json::from_value(json!("ses://AKID:topsecret@eu-west-1")).unwrap();
    let platform = set(TransportOwner::Platform, ses.clone()).await;
    let workspace_one = set(TransportOwner::workspace(workspace), TransportConfig::Log).await;
    let domain = set(
        TransportOwner::workspace_domain("Mail.Acme.test.", workspace),
        TransportConfig::Log,
    )
    .await;
    set(
        TransportOwner::organization(organization),
        TransportConfig::Log,
    )
    .await;
    assert_eq!(domain.domain.as_deref(), Some("mail.acme.test"));

    let db = mail.get::<Db>().await;
    let row = record::find::<TransportRow, _>(&*db, platform.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !row.config.contains("topsecret"),
        "the settings are encrypted"
    );
    assert_eq!(resolver.config(platform.id).await.unwrap(), ses);

    let scope = MailScope::workspace(workspace).with_organization(organization);
    let resolved = resolver
        .resolve(&scope, Some("mail.acme.test"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (resolved.id, resolved.tier),
        (domain.id, TransportTier::Domain)
    );
    let resolved = resolver
        .resolve(&scope, Some("other.test"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.id, workspace_one.id);
    let resolved = resolver
        .resolve(&MailScope::default(), None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.id, platform.id);

    // Set again: the same row, its new settings.
    let replaced = set(TransportOwner::workspace(workspace), TransportConfig::Log).await;
    assert_eq!(replaced.id, workspace_one.id);
    assert!(replaced.updated_at >= workspace_one.updated_at);

    // Off: skipped when resolving.
    let uow = mail.uow().await;
    let off = resolver.set_enabled(&uow, domain.id, false).await.unwrap();
    uow.commit().await.unwrap();
    assert!(!off.enabled);
    let resolved = resolver
        .resolve(&scope, Some("mail.acme.test"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.tier, TransportTier::Workspace);
    assert_eq!(
        resolver
            .list(Some(TransportTier::Domain))
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(resolver.list(None).await.unwrap().len(), 4);

    let uow = mail.uow().await;
    assert!(
        resolver
            .remove(&uow, &TransportOwner::workspace(workspace))
            .await
            .unwrap()
    );
    assert!(
        !resolver
            .remove(&uow, &TransportOwner::workspace(workspace))
            .await
            .unwrap()
    );
    assert!(matches!(
        resolver.set_enabled(&uow, Uuid::now_v7(), true).await,
        Err(MailError::NotFound(_))
    ));
    uow.commit().await.unwrap();
    assert!(
        resolver
            .get(&TransportOwner::workspace(workspace))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        resolver
            .get(&TransportOwner::Platform)
            .await
            .unwrap()
            .map(|t| t.id),
        Some(platform.id)
    );
}

/// Held mail: stored encrypted, listed, released once, discarded.
async fn outbox(mail: MailApp) {
    let workspace = Uuid::now_v7();
    let mailer = mail.mailer().await;
    let held = mailer
        .for_scope(&MailScope::workspace(workspace))
        .deliver(&welcome("all@acme.test").hold("Every employee"))
        .await
        .unwrap();
    assert_eq!(held.status, DeliveryStatus::Held);
    let other = mailer
        .deliver(&welcome("board@acme.test").hold("The board"))
        .await
        .unwrap();
    let outbox = mail.get::<Outbox>().await;
    let id = held.outbox_id.unwrap();
    let db = mail.get::<Db>().await;
    let row = record::find::<OutboxRow, _>(&*db, id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !row.message.contains("all@acme.test"),
        "the message is encrypted"
    );
    assert_eq!(row.recipients, json!(["all@acme.test"]));

    let found = outbox.find(id).await.unwrap();
    assert_eq!(found.recipients, ["all@acme.test"]);
    assert_eq!(found.workspace_id, Some(workspace));
    assert_eq!(
        outbox.message(id).await.unwrap().html.as_deref(),
        Some("<p>Welcome, Anu!</p>")
    );
    let listed = outbox
        .list(
            Some(OutboxStatus::Held),
            Some(workspace),
            PageQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(listed.data.iter().map(|h| h.id).collect::<Vec<_>>(), [id]);
    assert_eq!(
        outbox
            .list(None, None, PageQuery::new(1, 1))
            .await
            .unwrap()
            .meta
            .last_page,
        2
    );

    let admin = Uuid::now_v7();
    let released = outbox.release(id, Some(admin)).await.unwrap();
    assert_eq!(released.status, DeliveryStatus::Sent);
    let after = outbox.find(id).await.unwrap();
    assert_eq!(after.status, OutboxStatus::Released);
    assert_eq!(after.decided_by, Some(admin));
    assert!(after.decided_at.is_some());
    assert_eq!(after.delivery_id, released.delivery_id);
    assert!(matches!(
        outbox.release(id, None).await,
        Err(MailError::Conflict(_))
    ));

    let discarded = outbox
        .discard(other.outbox_id.unwrap(), Some(admin))
        .await
        .unwrap();
    assert_eq!(discarded.status, OutboxStatus::Discarded);
    assert!(matches!(
        outbox.release(discarded.id, None).await,
        Err(MailError::Conflict(_))
    ));
    assert_eq!(
        mail.fake.sent().len(),
        1,
        "only the released message went out"
    );
}

/// Eight admins release the same message (and one discards it) at the
/// same time: exactly one wins, the rest get a conflict, and the message
/// goes out at most once.
async fn released_by_several_admins_at_once(mail: MailApp) {
    let held = mail
        .mailer()
        .await
        .deliver(&welcome("all@acme.test").hold("Every employee"))
        .await
        .unwrap();
    let id = held.outbox_id.unwrap();
    let outbox = mail.get::<Outbox>().await;
    let mut tasks = Vec::new();
    for n in 0..9 {
        let outbox = outbox.clone();
        tasks.push(tokio::spawn(async move {
            if n == 4 {
                outbox
                    .discard(id, Some(Uuid::now_v7()))
                    .await
                    .map(|_| false)
            } else {
                outbox.release(id, Some(Uuid::now_v7())).await.map(|_| true)
            }
        }));
    }
    let mut won = Vec::new();
    for task in tasks {
        match task.await.unwrap() {
            Ok(released) => won.push(released),
            Err(MailError::Conflict(_)) => {}
            Err(other) => panic!("a racing admin got {other}"),
        }
    }
    assert_eq!(won.len(), 1, "exactly one admin decides");
    let after = outbox.find(id).await.unwrap();
    let sent = mail.fake.sent().len();
    if won[0] {
        assert_eq!(after.status, OutboxStatus::Released);
        assert_eq!(sent, 1);
        assert!(after.delivery_id.is_some());
    } else {
        assert_eq!(after.status, OutboxStatus::Discarded);
        assert_eq!(sent, 0);
    }
    let db = mail.get::<Db>().await;
    assert_eq!(
        DeliveryRow::select().count(&*db).await.unwrap(),
        u64::from(won[0]),
        "one delivery logged when released"
    );
}
