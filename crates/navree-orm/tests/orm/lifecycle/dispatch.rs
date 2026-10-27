//! When hooks run: behaviours inside the transaction, listeners' "-ed"
//! events after the unit of work commits, nothing for listeners inside
//! `without_events`.

use std::sync::{Arc, Mutex};

use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder};
use navree_orm::hooks::{Flow, HookKind, ModelEvent, ModelHook};
use navree_orm::{ExecContext, Model, OrmError, Record, Result, without_events};

use crate::models::Company;
use crate::support::{app, count_rows};

/// What a hook saw, in order.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<String>>>);

impl Seen {
    fn push(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// Notes every event, and whether its unit of work had committed.
struct Recorder {
    seen: Seen,
    kind: HookKind,
}

async fn committed(ctx: &ExecContext) -> &'static str {
    match ctx.uow() {
        Some(uow) if uow.is_finished().await => " (committed)",
        _ => "",
    }
}

impl ModelHook<Company> for Recorder {
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<Company>,
    ) -> BoxFuture<'a, Result<Flow>> {
        Box::pin(async move {
            self.seen
                .push(format!("{event} {}{}", record.name, committed(ctx).await));
            Ok(Flow::Continue)
        })
    }

    fn after<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<Company>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.seen
                .push(format!("{event} {}{}", record.name, committed(ctx).await));
            Ok(())
        })
    }

    fn kind(&self) -> HookKind {
        self.kind
    }
}

/// Registers a [`Recorder`] of `kind`.
struct Recording(Seen, HookKind);

impl Module for Recording {
    fn register(&self, m: &mut ModuleBuilder) {
        register_recorder(m, self.0.clone(), self.1);
    }
}

fn register_recorder(m: &mut ModuleBuilder, seen: Seen, kind: HookKind) {
    m.multi(Factory::<dyn ModelHook<Company>>::new(move |_| {
        let seen = seen.clone();
        async move { Ok(Arc::new(Recorder { seen, kind }) as Arc<dyn ModelHook<Company>>) }
    }));
}

/// Registers a listener and a behaviour (one module: a module type
/// registers once per app).
struct Both {
    listener: Seen,
    behaviour: Seen,
}

impl Module for Both {
    fn register(&self, m: &mut ModuleBuilder) {
        register_recorder(m, self.listener.clone(), HookKind::Listener);
        register_recorder(m, self.behaviour.clone(), HookKind::Behaviour);
    }
}

fn acme() -> Company {
    Company {
        name: "Acme".into(),
        ..Company::default()
    }
}

#[tokio::test]
async fn listeners_see_ed_events_after_the_unit_of_work_commits() {
    let seen = Seen::default();
    let (_app, db, _clock) = app(vec![Box::new(Recording(seen.clone(), HookKind::Listener))]).await;

    let uow = db.uow(None, None).await;
    let mut company = Company::create(&uow, acme()).await.unwrap();
    assert_eq!(seen.take(), ["saving Acme", "creating Acme"]);
    company.name = "Acme Corp".into();
    company.save(&uow).await.unwrap();
    assert_eq!(seen.take(), ["saving Acme Corp", "updating Acme Corp"]);
    uow.commit().await.unwrap();
    assert_eq!(
        seen.take(),
        [
            "created Acme (committed)",
            "saved Acme (committed)",
            "updated Acme Corp (committed)",
            "saved Acme Corp (committed)",
        ],
        "each event saw the record as it was when it happened"
    );

    // Outside a unit of work, at once.
    Company::create(&*db, acme()).await.unwrap();
    assert_eq!(
        seen.take(),
        ["saving Acme", "creating Acme", "created Acme", "saved Acme"]
    );
}

#[tokio::test]
async fn a_rolled_back_unit_of_work_tells_listeners_nothing_happened() {
    let seen = Seen::default();
    let (_app, db, _clock) = app(vec![Box::new(Recording(seen.clone(), HookKind::Listener))]).await;
    let uow = db.uow(None, None).await;
    Company::create(&uow, acme()).await.unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(seen.take(), ["saving Acme", "creating Acme"]);
    assert_eq!(count_rows(&db, "companies").await, 0);
}

#[tokio::test]
async fn behaviours_and_transactional_listeners_run_inside_the_transaction() {
    for kind in [HookKind::Behaviour, HookKind::TransactionalListener] {
        let seen = Seen::default();
        let (_app, db, _clock) = app(vec![Box::new(Recording(seen.clone(), kind))]).await;
        let uow = db.uow(None, None).await;
        Company::create(&uow, acme()).await.unwrap();
        assert_eq!(
            seen.take(),
            ["saving Acme", "creating Acme", "created Acme", "saved Acme"],
            "{kind:?}"
        );
        uow.commit().await.unwrap();
        assert!(seen.take().is_empty(), "{kind:?}");
    }
}

#[tokio::test]
async fn hooks_run_inside_the_unit_of_work_of_their_operation() {
    /// Checks that the unit of work the record is written in is the
    /// task's current one while the hook runs.
    struct Current(Seen);

    impl ModelHook<Company> for Current {
        fn after<'a>(
            &'a self,
            event: ModelEvent,
            ctx: &'a ExecContext,
            _record: &'a mut Record<Company>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let current = navree_db::Uow::current();
                let same = matches!((ctx.uow(), &current), (Some(a), Some(b)) if a.same_as(b));
                self.0.push(format!("{event} current={same}"));
                Ok(())
            })
        }
    }

    struct Registering(Seen);

    impl Module for Registering {
        fn register(&self, m: &mut ModuleBuilder) {
            let seen = self.0.clone();
            m.multi(Factory::<dyn ModelHook<Company>>::new(move |_| {
                let seen = seen.clone();
                async move { Ok(Arc::new(Current(seen)) as Arc<dyn ModelHook<Company>>) }
            }));
        }
    }

    let seen = Seen::default();
    let (_app, db, _clock) = app(vec![Box::new(Registering(seen.clone()))]).await;
    let uow = db.uow(None, None).await;
    // Not inside `uow.scope`: the ORM makes the unit of work current for
    // its hooks, so jobs and events they dispatch join it.
    Company::create(&uow, acme()).await.unwrap();
    assert_eq!(seen.take(), ["created current=true", "saved current=true"]);
    uow.commit().await.unwrap();
}

#[tokio::test]
async fn without_events_silences_listeners_but_keeps_behaviours_and_the_audit_trail() {
    let listener = Seen::default();
    let behaviour = Seen::default();
    let (_app, db, _clock) = app(vec![Box::new(Both {
        listener: listener.clone(),
        behaviour: behaviour.clone(),
    })])
    .await;

    let uow = db.uow(None, None).await;
    let created = without_events(Company::create(&uow, acme())).await.unwrap();
    assert!(listener.take().is_empty());
    assert_eq!(
        behaviour.take(),
        ["saving Acme", "creating Acme", "created Acme", "saved Acme"]
    );
    assert_eq!(uow.changes().len(), 1, "the change is still recorded");
    uow.commit().await.unwrap();
    assert!(listener.take().is_empty(), "nothing was deferred either");
    assert!(created.exists());

    // Nested and finished: events come back afterwards.
    without_events(async {
        without_events(async {}).await;
        Company::create(&*db, acme()).await.unwrap();
    })
    .await;
    assert!(listener.take().is_empty());
    Company::create(&*db, acme()).await.unwrap();
    assert_eq!(listener.take().len(), 4);
}

/// Fails on `event` with `message`.
struct Failing {
    event: ModelEvent,
    kind: HookKind,
}

impl ModelHook<Company> for Failing {
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        _record: &'a mut Record<Company>,
    ) -> BoxFuture<'a, Result<Flow>> {
        Box::pin(async move {
            if event == self.event {
                return Err(OrmError::Hook {
                    model: "Company".into(),
                    message: format!("{event} refused"),
                });
            }
            Ok(Flow::Continue)
        })
    }

    fn after<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        _record: &'a mut Record<Company>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if event == self.event {
                return Err(OrmError::Hook {
                    model: "Company".into(),
                    message: format!("{event} refused"),
                });
            }
            Ok(())
        })
    }

    fn kind(&self) -> HookKind {
        self.kind
    }
}

struct FailingOn(ModelEvent, HookKind);

impl Module for FailingOn {
    fn register(&self, m: &mut ModuleBuilder) {
        let (event, kind) = (self.0, self.1);
        m.multi(Factory::<dyn ModelHook<Company>>::new(move |_| async move {
            Ok(Arc::new(Failing { event, kind }) as Arc<dyn ModelHook<Company>>)
        }));
    }
}

#[tokio::test]
async fn an_ing_listener_error_aborts_the_write_and_is_returned() {
    let (_app, db, _clock) = app(vec![Box::new(FailingOn(
        ModelEvent::Creating,
        HookKind::Listener,
    ))])
    .await;
    let error = Company::create(&*db, acme()).await.unwrap_err();
    assert_eq!(error.to_string(), "a Company hook failed: creating refused");
    assert_eq!(count_rows(&db, "companies").await, 0);
}

#[tokio::test]
async fn an_after_commit_listener_error_never_undoes_the_commit() {
    let (_app, db, _clock) = app(vec![Box::new(FailingOn(
        ModelEvent::Created,
        HookKind::Listener,
    ))])
    .await;
    let uow = db.uow(None, None).await;
    let created = Company::create(&uow, acme()).await.unwrap();
    uow.commit().await.expect("the commit succeeds");
    assert_eq!(count_rows(&db, "companies").await, 1);
    assert!(created.exists());

    // Outside a unit of work the "-ed" listener runs at once: its error is
    // the operation's (the row is already written).
    let error = Company::create(&*db, acme()).await.unwrap_err();
    assert_eq!(error.to_string(), "a Company hook failed: created refused");
}

#[tokio::test]
async fn a_panicking_after_commit_listener_is_contained() {
    struct Panics;

    impl ModelHook<Company> for Panics {
        fn after<'a>(
            &'a self,
            event: ModelEvent,
            _ctx: &'a ExecContext,
            _record: &'a mut Record<Company>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                assert_ne!(event, ModelEvent::Created, "a listener bug");
                Ok(())
            })
        }

        fn kind(&self) -> HookKind {
            HookKind::Listener
        }
    }

    struct Registering;

    impl Module for Registering {
        fn register(&self, m: &mut ModuleBuilder) {
            m.multi(Factory::<dyn ModelHook<Company>>::new(|_| async {
                Ok(Arc::new(Panics) as Arc<dyn ModelHook<Company>>)
            }));
        }
    }

    let (_app, db, _clock) = app(vec![Box::new(Registering)]).await;
    let uow = db.uow(None, None).await;
    Company::create(&uow, acme()).await.unwrap();
    uow.commit()
        .await
        .expect("a listener's panic does not reach the committer");
    assert_eq!(count_rows(&db, "companies").await, 1);
}

#[test]
fn every_event_knows_when_listeners_hear_it() {
    let after_commit: Vec<&str> = ModelEvent::ALL
        .into_iter()
        .filter(|event| event.runs_after_commit())
        .map(ModelEvent::name)
        .collect();
    assert_eq!(
        after_commit,
        [
            "created",
            "updated",
            "saved",
            "deleted",
            "trashed",
            "force_deleted",
            "restored"
        ]
    );
    assert_eq!(ModelEvent::ALL.len(), 15);
    let archived = ModelEvent::Custom("archived");
    assert_eq!(archived.name(), "archived");
    assert!(archived.is_cancellable(), "custom events run at once");
    assert!(!archived.runs_after_commit());
    assert_eq!(
        ModelEvent::from_name("force_deleting"),
        Some(ModelEvent::ForceDeleting)
    );
    assert_eq!(ModelEvent::from_name("archived"), None);
    assert_eq!(HookKind::default(), HookKind::Behaviour);
}
