//! Pruning: record by record (`Prunable`) and a chunk per statement
//! (`MassPrunable`), in code and through `model:prune`.

use std::sync::{Arc, Mutex};

use chrono::{Duration, TimeZone, Utc};
use navree_events::{EventsModule, Listener, ModuleBuilderEventsExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_orm::prune::{
    MassPrunable, ModelPruningFinished, ModelPruningStarting, ModelsPruned, Prunable,
};
use navree_orm::{ExecContext, Model, ModuleBuilderOrmExt, Observer, Record, Result};

use super::models::{LogEntry, Metric, create_tables, prune_cutoff, pruned_with_prefix};
use crate::models::Company;
use crate::support::{app, count_rows};

fn old() -> chrono::DateTime<Utc> {
    prune_cutoff() - Duration::days(30)
}

fn recent() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap()
}

/// What the pruning events said, in order.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Vec<String>>>);

impl Heard {
    fn push(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// Listens to every pruning event.
struct PruningListener(Heard);

impl Injectable for PruningListener {
    fn construct(r: Resolver) -> BoxFuture<'static, std::result::Result<Self, KernelError>> {
        Box::pin(async move { Ok(PruningListener(Heard::clone(&*r.get::<Heard>().await?))) })
    }
}

impl Listener<ModelsPruned> for PruningListener {
    fn handle<'a>(
        &'a self,
        event: &'a ModelsPruned,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, std::result::Result<(), BoxError>> {
        self.0.push(format!("pruned {} {}", event.model, event.count));
        Box::pin(async { Ok(()) })
    }
}

impl Listener<ModelPruningStarting> for PruningListener {
    fn handle<'a>(
        &'a self,
        event: &'a ModelPruningStarting,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, std::result::Result<(), BoxError>> {
        self.0.push(format!("starting {}", event.models.join(",")));
        Box::pin(async { Ok(()) })
    }
}

impl Listener<ModelPruningFinished> for PruningListener {
    fn handle<'a>(
        &'a self,
        event: &'a ModelPruningFinished,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, std::result::Result<(), BoxError>> {
        self.0.push(format!("finished {}", event.models.join(",")));
        Box::pin(async { Ok(()) })
    }
}

/// Hears log entries being removed for good.
struct LogObserver(Heard);

impl Observer<LogEntry> for LogObserver {
    async fn force_deleted(&self, entry: &Record<LogEntry>, _ctx: &ExecContext) -> Result<()> {
        self.0.push(format!("force_deleted {}", entry.message));
        Ok(())
    }
}

/// Registers the models, a pruning listener and a log observer.
struct Pruning(Heard);

impl Module for Pruning {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(EventsModule)
            .value(self.0.clone())
            .listen::<ModelsPruned, PruningListener>()
            .on::<ModelPruningStarting, PruningListener>()
            .on::<ModelPruningFinished, PruningListener>()
            .model::<LogEntry>()
            .model::<Metric>()
            .model::<Company>()
            .observe::<LogEntry>(LogObserver(self.0.clone()));
    }
}

async fn log(db: &navree_db::Db, message: &str, at: chrono::DateTime<Utc>) -> Record<LogEntry> {
    LogEntry::create(
        db,
        LogEntry {
            message: message.into(),
            created_at: at,
            ..LogEntry::default()
        },
    )
    .await
    .unwrap()
}

async fn metric(db: &navree_db::Db, name: &str, at: chrono::DateTime<Utc>) {
    Metric::create(
        db,
        Metric {
            name: name.into(),
            recorded_at: at,
            ..Metric::default()
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn prunable_models_prune_old_records_one_by_one_trashed_ones_included() {
    let heard = Heard::default();
    let (_app, db, _clock) = app(vec![Box::new(Pruning(heard.clone()))]).await;
    create_tables(&db).await;
    for n in 1..=5 {
        let mut entry = log(&db, &format!("p1 old {n}"), old()).await;
        if n == 3 {
            entry.delete(&*db).await.unwrap();
        }
    }
    log(&db, "p1 new", recent()).await;
    heard.take();

    let pruned = LogEntry::prune_all(&*db, 2).await.unwrap();
    assert_eq!(pruned, 5);
    assert_eq!(count_rows(&db, "log_entries").await, 1, "the trashed one too");
    assert_eq!(
        pruned_with_prefix("p1"),
        ["p1 old 1", "p1 old 2", "p1 old 3", "p1 old 4", "p1 old 5"],
        "pruning ran for each, in key order"
    );
    assert_eq!(
        heard.take(),
        [
            "force_deleted p1 old 1",
            "force_deleted p1 old 2",
            "pruned LogEntry 2",
            "force_deleted p1 old 3",
            "force_deleted p1 old 4",
            "pruned LogEntry 4",
            "force_deleted p1 old 5",
            "pruned LogEntry 5",
        ],
        "model events per record, a running total per chunk"
    );
    assert_eq!(LogEntry::prune_all(&*db, 2).await.unwrap(), 0);
}

#[tokio::test]
async fn a_failing_record_is_reported_and_the_run_goes_on() {
    let (_app, db, _clock) = app(vec![Box::new(Pruning(Heard::default()))]).await;
    create_tables(&db).await;
    log(&db, "p2 first", old()).await;
    log(&db, "p2 explode", old()).await;
    log(&db, "p2 last", old()).await;

    assert_eq!(LogEntry::prune_all(&*db, 10).await.unwrap(), 2);
    assert_eq!(pruned_with_prefix("p2"), ["p2 first", "p2 last"]);
    let left = LogEntry::query().with_trashed().get(&*db).await.unwrap();
    assert_eq!(left.map(|e| e.message.clone()), ["p2 explode"]);
}

#[tokio::test]
async fn mass_prunable_models_delete_a_chunk_per_statement_without_events() {
    let heard = Heard::default();
    let (_app, db, _clock) = app(vec![Box::new(Pruning(heard.clone()))]).await;
    create_tables(&db).await;
    for n in 1..=5 {
        metric(&db, &format!("cpu {n}"), old()).await;
    }
    metric(&db, "cpu now", recent()).await;

    assert_eq!(Metric::prune_all(&*db, 2).await.unwrap(), 5);
    assert_eq!(count_rows(&db, "metrics").await, 1);
    assert_eq!(
        heard.take(),
        ["pruned Metric 2", "pruned Metric 4", "pruned Metric 5"]
    );
}

#[tokio::test]
async fn the_prune_command_prunes_every_registered_prunable_model() {
    let heard = Heard::default();
    let (app, db, _clock) = app(vec![Box::new(Pruning(heard.clone()))]).await;
    create_tables(&db).await;
    log(&db, "p3 old", old()).await;
    log(&db, "p3 new", recent()).await;
    metric(&db, "disk", old()).await;
    heard.take();

    // Pretending counts and changes nothing.
    app.run_command("model:prune", &["--pretend"]).await.unwrap();
    assert_eq!(count_rows(&db, "log_entries").await, 2);
    assert_eq!(count_rows(&db, "metrics").await, 1);
    assert!(heard.take().is_empty());

    app.run_command("model:prune", &["--model", "Metric"])
        .await
        .unwrap();
    assert_eq!(count_rows(&db, "metrics").await, 0);
    assert_eq!(count_rows(&db, "log_entries").await, 2);
    assert_eq!(
        heard.take(),
        ["starting Metric", "pruned Metric 1", "finished Metric"]
    );

    app.run_command("model:prune", &["--except=Metric", "--chunk", "5"])
        .await
        .unwrap();
    assert_eq!(count_rows(&db, "log_entries").await, 1);
    assert_eq!(
        heard.take(),
        [
            "starting LogEntry",
            "force_deleted p3 old",
            "pruned LogEntry 1",
            "finished LogEntry"
        ]
    );
    assert_eq!(pruned_with_prefix("p3"), ["p3 old"]);
}

#[tokio::test]
async fn the_prune_command_refuses_what_it_cannot_do() {
    let (app, db, _clock) = app(vec![Box::new(Pruning(Heard::default()))]).await;
    create_tables(&db).await;
    let error = |args: &'static [&'static str]| {
        let app = &app;
        async move {
            app.run_command("model:prune", args)
                .await
                .unwrap_err()
                .to_string()
        }
    };
    assert_eq!(
        error(&["--model", "Ghost"]).await,
        "model:prune: no prunable model is named `Ghost` (prunable: LogEntry, Metric; register models with m.model::<M>() and mark them #[model(prunable)] or #[model(mass_prunable)])"
    );
    assert_eq!(
        error(&["--model", "Company"]).await,
        "model:prune: no prunable model is named `Company` (prunable: LogEntry, Metric; register models with m.model::<M>() and mark them #[model(prunable)] or #[model(mass_prunable)])"
    );
    assert_eq!(
        error(&["--model", "LogEntry", "--except", "Metric"]).await,
        "model:prune: --model and --except cannot be combined"
    );
    assert_eq!(
        error(&["--chunk", "0"]).await,
        "model:prune: --chunk must be a whole number of at least 1, not `0`"
    );
    assert_eq!(
        error(&["--force"]).await,
        "model:prune: unknown option `--force` (expected --model, --except, --chunk or --pretend)"
    );
}

#[test]
fn prunable_models_say_so_in_their_metadata() {
    use navree_orm::prune::PruneKind;

    let pruning = LogEntry::meta().pruning().expect("prunable");
    assert_eq!(pruning.kind(), PruneKind::EachRecord);
    assert_eq!(pruning.chunk_size(), None);
    let pruning = Metric::meta().pruning().expect("mass prunable");
    assert_eq!(pruning.kind(), PruneKind::MassDelete);
    assert_eq!(pruning.chunk_size(), Some(2));
    assert!(Company::meta().pruning().is_none());
}
