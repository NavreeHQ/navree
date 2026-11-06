//! Observers (declared on the model or registered by a module), closure
//! listeners, custom events and `replicate_on`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::hooks::{Flow, ModelEvent};
use navree_orm::{ExecContext, Model, ModuleBuilderOrmExt, Observer, OrmError, Record, Result};

use super::models::{AuditRow, Ticket, create_tables, ticket_log};
use crate::models::Company;
use crate::support::{app, count_rows, sqlite};

/// Lines in order.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    fn push(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// Hears every event of companies.
struct CompanyObserver(Log);

impl CompanyObserver {
    fn note(&self, event: &str, company: &Record<Company>) {
        self.0.push(format!("{event} {}", company.name));
    }
}

impl Observer<Company> for CompanyObserver {
    async fn retrieved(&self, company: &mut Record<Company>, _ctx: &ExecContext) -> Result<()> {
        self.note("retrieved", company);
        Ok(())
    }

    async fn saving(&self, company: &mut Record<Company>, _ctx: &ExecContext) -> Result<Flow> {
        self.note("saving", company);
        if company.name == "Blocked" {
            return Ok(Flow::Cancel);
        }
        Ok(Flow::Continue)
    }

    async fn creating(&self, company: &mut Record<Company>, _ctx: &ExecContext) -> Result<Flow> {
        self.note("creating", company);
        if company.country.is_none() {
            company.country = Some("US".into());
        }
        Ok(Flow::Continue)
    }

    async fn created(&self, company: &Record<Company>, _ctx: &ExecContext) -> Result<()> {
        self.note("created", company);
        Ok(())
    }

    async fn updating(&self, company: &mut Record<Company>, _ctx: &ExecContext) -> Result<Flow> {
        self.note("updating", company);
        Ok(Flow::Continue)
    }

    async fn updated(&self, company: &Record<Company>, _ctx: &ExecContext) -> Result<()> {
        let changed: Vec<&str> = company.get_changes().iter().map(|(c, _)| *c).collect();
        let before = company
            .get_original()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        self.0.push(format!(
            "updated {} (changed {}; was {before}; name changed: {})",
            company.name,
            changed.join(", "),
            company.was_changed_on([Company::NAME]),
        ));
        Ok(())
    }

    async fn saved(&self, company: &Record<Company>, _ctx: &ExecContext) -> Result<()> {
        self.note("saved", company);
        Ok(())
    }

    async fn deleting(&self, company: &mut Record<Company>, _ctx: &ExecContext) -> Result<Flow> {
        self.note("deleting", company);
        Ok(if company.employees > 0 {
            Flow::Cancel
        } else {
            Flow::Continue
        })
    }

    async fn deleted(&self, company: &Record<Company>, _ctx: &ExecContext) -> Result<()> {
        self.note("deleted", company);
        Ok(())
    }

    async fn replicating(&self, copy: &mut Record<Company>, _ctx: &ExecContext) -> Result<()> {
        copy.name = format!("{} (copy)", copy.name);
        self.note("replicating", copy);
        Ok(())
    }
}

struct Observed(Log);

impl Module for Observed {
    fn register(&self, m: &mut ModuleBuilder) {
        m.observe::<Company>(CompanyObserver(self.0.clone()));
    }
}

fn company(name: &str) -> Company {
    Company {
        name: name.into(),
        ..Company::default()
    }
}

#[tokio::test]
async fn observers_hear_every_event_in_order() {
    let log = Log::default();
    let (_app, db, clock) = app(vec![Box::new(Observed(log.clone()))]).await;

    let mut agra = Company::create(&*db, company("Agra")).await.unwrap();
    assert_eq!(
        agra.country.as_deref(),
        Some("US"),
        "creating changed the record"
    );
    assert_eq!(
        log.take(),
        ["saving Agra", "creating Agra", "created Agra", "saved Agra"]
    );

    let mut found = Company::find_or_fail(&*db, agra.id).await.unwrap();
    assert_eq!(log.take(), ["retrieved Agra"]);
    clock.advance(Duration::from_secs(60));
    found.name = "Agra Corp".into();
    found.save(&*db).await.unwrap();
    assert_eq!(
        log.take(),
        [
            "saving Agra Corp",
            "updating Agra Corp",
            "updated Agra Corp (changed name, updated_at; was Agra; name changed: true)",
            "saved Agra Corp",
        ]
    );

    assert!(found.delete(&*db).await.unwrap());
    assert_eq!(log.take(), ["deleting Agra Corp", "deleted Agra Corp"]);

    // `-ing` methods cancel.
    let mut blocked = Record::new(company("Blocked"));
    assert!(!blocked.save(&*db).await.unwrap());
    agra.employees = 3;
    agra.save(&*db).await.unwrap();
    log.take();
    assert!(
        !agra.delete(&*db).await.unwrap(),
        "companies with people stay"
    );
    assert_eq!(
        count_rows(&db, "companies").await,
        0,
        "agra was deleted as found"
    );
}

#[tokio::test]
async fn observers_hear_what_changed_after_the_commit() {
    let log = Log::default();
    let (_app, db, clock) = app(vec![Box::new(Observed(log.clone()))]).await;
    let mut agra = Company::create(&*db, company("Agra")).await.unwrap();
    log.take();

    clock.advance(Duration::from_secs(60));
    let uow = db.uow(None, None).await;
    agra.name = "Agra Inc".into();
    agra.save(&uow).await.unwrap();
    agra.employees = 12;
    agra.save(&uow).await.unwrap();
    assert_eq!(
        log.take(),
        [
            "saving Agra Inc",
            "updating Agra Inc",
            "saving Agra Inc",
            "updating Agra Inc"
        ]
    );
    uow.commit().await.unwrap();
    assert_eq!(
        log.take(),
        [
            "updated Agra Inc (changed name, updated_at; was Agra; name changed: true)",
            "saved Agra Inc",
            "updated Agra Inc (changed employees; was Agra Inc; name changed: false)",
            "saved Agra Inc",
        ],
        "each listener call saw the changes and the original of its own save"
    );
}

#[tokio::test]
async fn observers_declared_on_the_model_need_no_app() {
    let t = sqlite().await;
    create_tables(&t.db).await;

    let ticket = Ticket::create(
        &t.db,
        Ticket {
            subject: "printer jam 41".into(),
            ..Ticket::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(ticket.status, "open", "the observer filled the status");
    assert_eq!(
        ticket_log("printer jam 41"),
        [
            "saving printer jam 41",
            "creating printer jam 41",
            "created printer jam 41",
            "saved printer jam 41",
        ]
    );

    let mut empty = Record::new(Ticket::default());
    assert!(
        !empty.save(&t.db).await.unwrap(),
        "an empty subject is refused"
    );
    assert_eq!(count_rows(&t.db, "tickets").await, 1);
}

/// Writes an audit row for every company created, in the same
/// transaction.
struct WritesAudit;

impl Observer<Company> for WritesAudit {
    const IN_TRANSACTION: bool = true;

    async fn created(&self, company: &Record<Company>, ctx: &ExecContext) -> Result<()> {
        let uow = ctx.uow().expect("runs in the unit of work");
        assert!(!uow.is_finished().await, "inside the transaction");
        AuditRow::create(
            uow,
            AuditRow {
                entry: format!("created {}", company.name),
                ..AuditRow::default()
            },
        )
        .await?;
        Ok(())
    }
}

struct Audited;

impl Module for Audited {
    fn register(&self, m: &mut ModuleBuilder) {
        m.observe::<Company>(WritesAudit);
    }
}

#[tokio::test]
async fn in_transaction_observers_write_with_the_record_or_not_at_all() {
    let (_app, db, _clock) = app(vec![Box::new(Audited)]).await;
    create_tables(&db).await;

    let uow = db.uow(None, None).await;
    Company::create(&uow, company("Kept")).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(count_rows(&db, "audit_rows").await, 1);

    let uow = db.uow(None, None).await;
    Company::create(&uow, company("Dropped")).await.unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(
        count_rows(&db, "audit_rows").await,
        1,
        "rolled back together"
    );
    assert_eq!(count_rows(&db, "companies").await, 1);
}

/// An observer built by the container, with a dependency.
struct InjectedObserver {
    log: Arc<Log>,
}

impl Injectable for InjectedObserver {
    fn construct(r: Resolver) -> BoxFuture<'static, std::result::Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(InjectedObserver {
                log: r.get::<Log>().await?,
            })
        })
    }
}

impl Observer<Company> for InjectedObserver {
    async fn created(&self, company: &Record<Company>, _ctx: &ExecContext) -> Result<()> {
        self.log.push(format!("injected {}", company.name));
        Ok(())
    }
}

struct Injected(Log);

impl Module for Injected {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(self.0.clone());
        m.observer::<Company, InjectedObserver>();
    }
}

#[tokio::test]
async fn module_observers_can_be_built_by_the_container() {
    let log = Log::default();
    let (_app, db, _clock) = app(vec![Box::new(Injected(log.clone()))]).await;
    Company::create(&*db, company("Built")).await.unwrap();
    assert_eq!(log.take(), ["injected Built"]);
}

struct Closures(Log);

impl Module for Closures {
    fn register(&self, m: &mut ModuleBuilder) {
        m.on_model::<Company, _>(ModelEvent::Saving, |company, _ctx| {
            Box::pin(async move {
                Ok(if company.name.is_empty() {
                    Flow::Cancel
                } else {
                    company.name = company.name.trim().to_owned();
                    Flow::Continue
                })
            })
        });
        let log = self.0.clone();
        m.on_model::<Company, _>(ModelEvent::Created, move |company, ctx| {
            let log = log.clone();
            Box::pin(async move {
                let committed = match ctx.uow() {
                    Some(uow) => uow.is_finished().await,
                    None => false,
                };
                log.push(format!("created {} committed={committed}", company.name));
                Ok(())
            })
        });
    }
}

#[tokio::test]
async fn closure_listeners_steer_and_hear_one_event_each() {
    let log = Log::default();
    let (_app, db, _clock) = app(vec![Box::new(Closures(log.clone()))]).await;

    let trimmed = Company::create(&*db, company("  Spaced  ")).await.unwrap();
    assert_eq!(trimmed.name, "Spaced");
    assert_eq!(log.take(), ["created Spaced committed=false"]);

    let mut nameless = Record::new(company(""));
    assert!(!nameless.save(&*db).await.unwrap());

    let uow = db.uow(None, None).await;
    Company::create(&uow, company("Later")).await.unwrap();
    assert!(log.take().is_empty());
    uow.commit().await.unwrap();
    assert_eq!(log.take(), ["created Later committed=true"]);
}

#[tokio::test]
async fn custom_events_reach_observers_and_closures_at_once() {
    let t = sqlite().await;
    create_tables(&t.db).await;
    let mut ticket = Ticket::create(
        &t.db,
        Ticket {
            subject: "custom 7".into(),
            ..Ticket::default()
        },
    )
    .await
    .unwrap();
    ticket_log("custom 7");

    assert_eq!(
        ticket.fire(&t.db, "escalated").await.unwrap(),
        Flow::Continue
    );
    assert_eq!(ticket_log("custom 7"), ["escalated custom 7"]);

    ticket.status = "frozen".into();
    assert_eq!(
        ticket.fire(&t.db, "escalated").await.unwrap(),
        Flow::Cancel,
        "observers may cancel a custom event"
    );

    let error = ticket.fire(&t.db, "created").await.unwrap_err();
    assert!(matches!(error, OrmError::InvalidQuery(_)), "{error:?}");
    assert_eq!(
        error.to_string(),
        "invalid query: `created` is a built-in model event: fire events of your own (`archived`, `escalated`, ...)"
    );
}

struct CustomClosure(Log);

impl Module for CustomClosure {
    fn register(&self, m: &mut ModuleBuilder) {
        let log = self.0.clone();
        m.on_model::<Company, _>(ModelEvent::Custom("archived"), move |company, _ctx| {
            let log = log.clone();
            Box::pin(async move {
                log.push(format!("archived {}", company.name));
                Ok(())
            })
        });
    }
}

#[tokio::test]
async fn closure_listeners_hear_custom_events_by_name() {
    let log = Log::default();
    let (_app, db, _clock) = app(vec![Box::new(CustomClosure(log.clone()))]).await;
    let mut agra = Company::create(&*db, company("Agra")).await.unwrap();
    assert_eq!(agra.fire(&*db, "archived").await.unwrap(), Flow::Continue);
    assert_eq!(agra.fire(&*db, "published").await.unwrap(), Flow::Continue);
    assert_eq!(log.take(), ["archived Agra"]);
}

#[tokio::test]
async fn replicate_on_lets_listeners_change_the_copy() {
    let log = Log::default();
    let (_app, db, _clock) = app(vec![Box::new(Observed(log.clone()))]).await;
    let agra = Company::create(&*db, company("Agra")).await.unwrap();
    log.take();

    let copy = agra.replicate_on(&*db).await.unwrap();
    assert_eq!(copy.name, "Agra (copy)");
    assert!(!copy.exists());
    assert_eq!(copy.id, 0, "no key");
    assert_eq!(log.take(), ["replicating Agra (copy)"]);

    let quiet = agra.replicate_quietly();
    assert_eq!(quiet.name, "Agra");
    assert!(log.take().is_empty());
}
