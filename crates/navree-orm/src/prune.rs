//! Pruning: deleting records that are no longer needed.
//!
//! - [`Prunable`] models delete their prunable records one by one, in
//!   chunks by key: each record's [`pruning`](Prunable::pruning) hook runs,
//!   then the record is deleted (removed for good when the model
//!   soft-deletes: trashed records are pruned too) with its model events. A
//!   record that fails is logged and reported, and the run goes on.
//! - [`MassPrunable`] models delete a chunk per statement, without loading
//!   records or firing model events (soft-deleted rows removed for good).
//!
//! Declare which on the model (`#[model(prunable)]` or
//! `#[model(mass_prunable)]`, so the registry knows) and register the model
//! with the app (`m.model::<Deal>()`): `navree model:prune` prunes every
//! registered prunable model. Schedule it daily with navree-schedule, in
//! any module: `m.schedule(|s| { s.command("model:prune",
//! &[]).daily_at("03:00").without_overlapping(); });`.
//!
//! Every run emits [`ModelsPruned`] after each chunk (through navree-events
//! when the app has `EventsModule`); `model:prune` also emits
//! [`ModelPruningStarting`] and [`ModelPruningFinished`].
//!
//! ```no_run
//! use chrono::{DateTime, Duration, Utc};
//! use navree_db::Db;
//! use navree_orm::prelude::*;
//! use navree_orm::prune::Prunable;
//!
//! #[derive(Model, Clone, Debug, Default, serde::Serialize)]
//! #[model(timestamps, soft_deletes, prunable)]
//! struct Note {
//!     id: i64,
//!     body: String,
//!     created_at: DateTime<Utc>,
//!     updated_at: DateTime<Utc>,
//!     deleted_at: Option<DateTime<Utc>>,
//! }
//!
//! impl Prunable for Note {
//!     /// Trashed notes are gone after 30 days.
//!     fn prunable(query: Builder<Self>) -> Builder<Self> {
//!         query.only_trashed().where_(Note::DELETED_AT.lt(Utc::now() - Duration::days(30)))
//!     }
//! }
//!
//! # async fn demo(db: Db) -> navree_orm::Result<()> {
//! let pruned = Note::prune_all(&db, 500).await?;
//! # Ok(()) }
//! ```

use std::fmt;
use std::future::Future;

use navree_db::Db;
use navree_events::{Event, EventBus};
use navree_kernel::{AppContext, BoxError, BoxFuture};
use serde::{Deserialize, Serialize};

use crate::error::{OrmError, Result};
use crate::exec::{Access, ExecContext, Executor};
use crate::ir::{Direction, Operator};
use crate::model::{Model, PrimaryKey};
use crate::query::{Builder, key_expr};
use crate::record::Record;
use crate::scope::Trashed;
use crate::value::Field;

/// A model whose old records are deleted one by one, with their events.
///
/// Mark the model `#[model(prunable)]` so `model:prune` finds it (the
/// derive checks this trait is implemented).
#[diagnostic::on_unimplemented(
    message = "`{Self}` is marked prunable but does not implement `Prunable`",
    note = "add `impl navree_orm::prune::Prunable for {Self} {{ fn prunable(query: Builder<Self>) -> Builder<Self> {{ .. }} }}`"
)]
pub trait Prunable: Model {
    /// How many records a chunk loads, when the model knows best (it wins
    /// over `model:prune --chunk`).
    const CHUNK_SIZE: Option<u64> = None;

    /// The records to prune: `query` narrowed (soft-deleted records are
    /// included unless it says otherwise).
    fn prunable(query: Builder<Self>) -> Builder<Self>;

    /// Runs before a record is deleted (remove its files, archive it, ...).
    /// An error skips the record (it is logged and reported).
    fn pruning(&self, ctx: &ExecContext) -> impl Future<Output = Result<()>> + Send {
        let _ = ctx;
        async { Ok(()) }
    }

    /// Prunes every prunable record, `chunk_size` loaded at a time; returns
    /// how many were deleted.
    fn prune_all<E: Executor + ?Sized>(
        exec: &E,
        chunk_size: u64,
    ) -> impl Future<Output = Result<u64>> + Send {
        prune_each::<Self, E>(exec, chunk_size)
    }
}

/// A model whose old records are deleted a chunk per statement, without
/// model events.
///
/// Mark the model `#[model(mass_prunable)]` so `model:prune` finds it.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is marked mass_prunable but does not implement `MassPrunable`",
    note = "add `impl navree_orm::prune::MassPrunable for {Self} {{ fn prunable(query: Builder<Self>) -> Builder<Self> {{ .. }} }}`"
)]
pub trait MassPrunable: Model {
    /// How many rows a statement deletes, when the model knows best (it
    /// wins over `model:prune --chunk`).
    const CHUNK_SIZE: Option<u64> = None;

    /// The records to prune: `query` narrowed (soft-deleted rows are
    /// included unless it says otherwise). A limit it sets is the chunk.
    fn prunable(query: Builder<Self>) -> Builder<Self>;

    /// Deletes every prunable row, `chunk_size` per statement; returns how
    /// many were deleted.
    fn prune_all<E: Executor + ?Sized>(
        exec: &E,
        chunk_size: u64,
    ) -> impl Future<Output = Result<u64>> + Send {
        prune_mass::<Self, E>(exec, chunk_size)
    }
}

impl<M: Model> Builder<M> {
    /// Includes soft-deleted rows unless the query chose otherwise.
    pub(crate) fn including_trashed(mut self) -> Self {
        if self.scopes.trashed == Trashed::Hidden {
            self.scopes.trashed = Trashed::Included;
        }
        self
    }
}

/// [`Prunable::prune_all`].
async fn prune_each<M: Prunable, E: Executor + ?Sized>(exec: &E, chunk_size: u64) -> Result<u64> {
    let chunk = chunk_size.max(1);
    let ctx = exec.target(Access::Write).await?.context();
    let mut total = 0;
    let mut after: Option<M::Key> = None;
    loop {
        let mut query = M::prunable(M::query())
            .including_trashed()
            .reorder_by(key_expr::<M>(), Direction::Asc)
            .limit(chunk);
        if let Some(after) = &after {
            query = query.where_(crate::column::Filter::compare(
                key_expr::<M>(),
                Operator::Gt,
                crate::ir::Expr::Value(after.to_value()),
            ));
        }
        let records = query.get(exec).await?;
        let fetched = records.len() as u64;
        let Some(last) = records.last().map(Record::key) else {
            break;
        };
        for mut record in records {
            match prune_record(exec, &ctx, &mut record).await {
                Ok(true) => total += 1,
                Ok(false) => {}
                Err(error) => PruneFailed::of(&record, error).report(&ctx).await,
            }
        }
        announce(&ctx, ModelsPruned::new(M::meta().name(), total)).await;
        if fetched < chunk {
            break;
        }
        after = Some(last);
    }
    Ok(total)
}

/// Runs `record`'s `pruning` hook, then deletes it (for good when its
/// model soft-deletes).
async fn prune_record<M: Prunable, E: Executor + ?Sized>(
    exec: &E,
    ctx: &ExecContext,
    record: &mut Record<M>,
) -> Result<bool> {
    record.model.pruning(ctx).await?;
    if M::meta().soft_deletes_enabled() {
        record.force_delete(exec).await
    } else {
        record.delete(exec).await
    }
}

/// [`MassPrunable::prune_all`].
async fn prune_mass<M: MassPrunable, E: Executor + ?Sized>(
    exec: &E,
    chunk_size: u64,
) -> Result<u64> {
    let mut query = M::prunable(M::query());
    if query.ir.limit.is_none() {
        query = query.limit(chunk_size.max(1));
    }
    let ctx = exec.target(Access::Write).await?.context();
    let mut total = 0;
    loop {
        let deleted = query.clone().force_delete(exec).await?;
        if deleted == 0 {
            break;
        }
        total += deleted;
        announce(&ctx, ModelsPruned::new(M::meta().name(), total)).await;
    }
    Ok(total)
}

/// Emits `event` through the app's event bus, if it has one; a failing
/// listener is logged, never a pruning error.
async fn announce<E: Event>(ctx: &ExecContext, event: E) {
    let Some(app) = ctx.app() else {
        return;
    };
    announce_in(app, event).await;
}

async fn announce_in<E: Event>(app: &AppContext, event: E) {
    let Some(bus) = app.try_get::<EventBus>().await else {
        return;
    };
    if let Err(error) = bus.emit(event).await {
        tracing::warn!(target: "navree_orm::prune", event = E::NAME, %error, "a pruning event listener failed");
    }
}

/// A record pruning skipped because its hook or delete failed.
#[derive(Debug, thiserror::Error)]
#[error("cannot prune {model} {key}: {source}")]
struct PruneFailed {
    model: &'static str,
    key: String,
    #[source]
    source: OrmError,
}

impl PruneFailed {
    fn of<M: Model>(record: &Record<M>, source: OrmError) -> Self {
        PruneFailed {
            model: M::meta().name(),
            key: record.key().to_key_string(),
            source,
        }
    }

    /// Logs the failure and reports it to the app's telemetry sinks.
    async fn report(&self, ctx: &ExecContext) {
        tracing::error!(target: "navree_orm::prune", model = self.model, key = %self.key, error = %self.source, "a record could not be pruned");
        if let Some(app) = ctx.app() {
            navree_telemetry::Telemetry::from_app(app)
                .await
                .report_exception(self);
        }
    }
}

/// Records of a model were pruned: emitted after each chunk, with the
/// running total of the run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelsPruned {
    /// The model, e.g. `Deal`.
    pub model: String,
    /// How many of its records the run has pruned so far.
    pub count: u64,
}

impl ModelsPruned {
    fn new(model: &str, count: u64) -> Self {
        ModelsPruned {
            model: model.to_owned(),
            count,
        }
    }
}

impl Event for ModelsPruned {
    const NAME: &'static str = "orm.models_pruned";
}

/// `model:prune` is about to prune these models.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPruningStarting {
    /// The models, in the order they are pruned.
    pub models: Vec<String>,
}

impl Event for ModelPruningStarting {
    const NAME: &'static str = "orm.model_pruning_starting";
}

/// `model:prune` pruned these models.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPruningFinished {
    /// The models, in the order they were pruned.
    pub models: Vec<String>,
}

impl Event for ModelPruningFinished {
    const NAME: &'static str = "orm.model_pruning_finished";
}

/// How a model is pruned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PruneKind {
    /// Record by record, with events ([`Prunable`]).
    EachRecord,
    /// A chunk per statement ([`MassPrunable`]).
    MassDelete,
}

/// How `model:prune` prunes a model: what `#[model(prunable)]` and
/// `#[model(mass_prunable)]` put in its [`ModelMeta`](crate::ModelMeta).
#[derive(Clone, Copy)]
pub struct Pruning {
    kind: PruneKind,
    chunk_size: Option<u64>,
    run: fn(Db, u64) -> BoxFuture<'static, Result<u64>>,
    count: fn(Db) -> BoxFuture<'static, Result<u64>>,
}

impl Pruning {
    /// Record by record ([`Prunable`]).
    pub const fn each<M: Prunable>() -> Self {
        Pruning {
            kind: PruneKind::EachRecord,
            chunk_size: <M as Prunable>::CHUNK_SIZE,
            run: run_each::<M>,
            count: count_each::<M>,
        }
    }

    /// A chunk per statement ([`MassPrunable`]).
    pub const fn mass<M: MassPrunable>() -> Self {
        Pruning {
            kind: PruneKind::MassDelete,
            chunk_size: <M as MassPrunable>::CHUNK_SIZE,
            run: run_mass::<M>,
            count: count_mass::<M>,
        }
    }

    /// How the model is pruned.
    pub fn kind(&self) -> PruneKind {
        self.kind
    }

    /// The model's own chunk size, if it sets one.
    pub fn chunk_size(&self) -> Option<u64> {
        self.chunk_size
    }

    /// Prunes the model on `db`, `chunk_size` at a time.
    pub async fn run(&self, db: Db, chunk_size: u64) -> Result<u64> {
        (self.run)(db, chunk_size).await
    }

    /// How many records a run would prune now.
    pub async fn count(&self, db: Db) -> Result<u64> {
        (self.count)(db).await
    }
}

impl fmt::Debug for Pruning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pruning")
            .field("kind", &self.kind)
            .field("chunk_size", &self.chunk_size)
            .finish_non_exhaustive()
    }
}

fn run_each<M: Prunable>(db: Db, chunk_size: u64) -> BoxFuture<'static, Result<u64>> {
    Box::pin(async move { M::prune_all(&db, chunk_size).await })
}

fn count_each<M: Prunable>(db: Db) -> BoxFuture<'static, Result<u64>> {
    Box::pin(async move { M::prunable(M::query()).including_trashed().count(&db).await })
}

fn run_mass<M: MassPrunable>(db: Db, chunk_size: u64) -> BoxFuture<'static, Result<u64>> {
    Box::pin(async move { M::prune_all(&db, chunk_size).await })
}

fn count_mass<M: MassPrunable>(db: Db) -> BoxFuture<'static, Result<u64>> {
    Box::pin(async move {
        let mut query = M::prunable(M::query()).including_trashed();
        // The limit is a chunk, not a cap on the run.
        query.ir.limit = None;
        query.count(&db).await
    })
}

/// What [`prune`] does: which registered models, how big the chunks, or
/// only counting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PruneRun {
    /// Only these models (by name); every prunable model when empty.
    pub models: Vec<String>,
    /// Every prunable model but these (by name).
    pub except: Vec<String>,
    /// Records per chunk, unless the model sets its own; `[orm]
    /// prune_chunk` when `None`.
    pub chunk: Option<u64>,
    /// Count what would be pruned instead of pruning.
    pub pretend: bool,
}

/// What [`prune`] did for one model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pruned {
    /// The model.
    pub model: &'static str,
    /// The records pruned (or that would be, when pretending).
    pub count: u64,
}

/// Prunes the registered prunable models of `app` (what `model:prune`
/// runs), on the app's [`Db`].
pub async fn prune(app: &AppContext, run: &PruneRun) -> std::result::Result<Vec<Pruned>, BoxError> {
    if !run.models.is_empty() && !run.except.is_empty() {
        return Err("--model and --except cannot be combined".into());
    }
    let prunable: Vec<(&'static str, Pruning)> = crate::module::registered_models(app)
        .into_iter()
        .filter_map(|model| {
            model
                .meta()
                .pruning()
                .map(|pruning| (model.name(), *pruning))
        })
        .collect();
    for name in &run.models {
        if !prunable.iter().any(|(known, _)| known == name) {
            let known: Vec<&str> = prunable.iter().map(|(name, _)| *name).collect();
            return Err(format!(
                "no prunable model is named `{name}` (prunable: {}; register models with m.model::<M>() and mark them #[model(prunable)] or #[model(mass_prunable)])",
                if known.is_empty() { "none".to_owned() } else { known.join(", ") }
            )
            .into());
        }
    }
    let chosen: Vec<(&'static str, Pruning)> = prunable
        .into_iter()
        .filter(|(name, _)| run.models.is_empty() || run.models.iter().any(|m| m == name))
        .filter(|(name, _)| !run.except.iter().any(|m| m == name))
        .collect();
    if chosen.is_empty() {
        return Ok(Vec::new());
    }
    let db = Db::clone(&*app.get::<Db>().await?);
    let default_chunk = match run.chunk {
        Some(chunk) => chunk,
        None => match app.try_get::<crate::OrmRuntime>().await {
            Some(runtime) => runtime.options().prune_chunk,
            None => crate::DEFAULT_PRUNE_CHUNK,
        },
    };
    let names: Vec<String> = chosen.iter().map(|(name, _)| (*name).to_owned()).collect();
    let mut done = Vec::with_capacity(chosen.len());
    if run.pretend {
        for (model, pruning) in chosen {
            done.push(Pruned {
                model,
                count: pruning.count(db.clone()).await?,
            });
        }
        return Ok(done);
    }
    announce_in(
        app,
        ModelPruningStarting {
            models: names.clone(),
        },
    )
    .await;
    for (model, pruning) in chosen {
        let chunk = pruning.chunk_size().unwrap_or(default_chunk);
        done.push(Pruned {
            model,
            count: pruning.run(db.clone(), chunk).await?,
        });
    }
    announce_in(app, ModelPruningFinished { models: names }).await;
    Ok(done)
}

/// The `model:prune` command: `[--model M]... [--except M]... [--chunk N]
/// [--pretend]`.
pub(crate) async fn prune_command(
    app: AppContext,
    args: Vec<String>,
) -> std::result::Result<(), BoxError> {
    let run = parse_args(&args).map_err(|message| format!("model:prune: {message}"))?;
    let pruned = prune(&app, &run)
        .await
        .map_err(|error| format!("model:prune: {error}"))?;
    if pruned.is_empty() {
        println!("No prunable models found.");
    }
    for Pruned { model, count } in pruned {
        match (count, run.pretend) {
            (0, _) => println!("No prunable {model} records found."),
            (count, true) => println!("{count} {model} records will be pruned."),
            (count, false) => println!("Pruned {count} {model} records."),
        }
    }
    Ok(())
}

/// Reads `model:prune`'s options (`--name value` or `--name=value`).
fn parse_args(args: &[String]) -> std::result::Result<PruneRun, String> {
    let mut run = PruneRun::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (arg.as_str(), None),
        };
        let mut value = |option: &str| {
            inline
                .clone()
                .or_else(|| args.next().cloned())
                .ok_or_else(|| format!("{option} needs a value"))
        };
        match name {
            "--model" => run.models.push(value("--model")?),
            "--except" => run.except.push(value("--except")?),
            "--chunk" => {
                let text = value("--chunk")?;
                match text.parse::<u64>() {
                    Ok(chunk) if chunk > 0 => run.chunk = Some(chunk),
                    _ => {
                        return Err(format!(
                            "--chunk must be a whole number of at least 1, not `{text}`"
                        ));
                    }
                }
            }
            "--pretend" if inline.is_none() => run.pretend = true,
            other => {
                return Err(format!(
                    "unknown option `{other}` (expected --model, --except, --chunk or --pretend)"
                ));
            }
        }
    }
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn options_parse_in_both_spellings() {
        let run = parse_args(&args(&[
            "--model",
            "Deal",
            "--model=Note",
            "--chunk=50",
            "--pretend",
        ]))
        .unwrap();
        assert_eq!(run.models, ["Deal", "Note"]);
        assert_eq!(run.chunk, Some(50));
        assert!(run.pretend);
        assert_eq!(
            parse_args(&args(&["--chunk"])).unwrap_err(),
            "--chunk needs a value"
        );
        assert_eq!(
            parse_args(&args(&["--chunk", "x"])).unwrap_err(),
            "--chunk must be a whole number of at least 1, not `x`"
        );
        assert!(parse_args(&args(&["--pretend=yes"])).is_err());
    }
}
