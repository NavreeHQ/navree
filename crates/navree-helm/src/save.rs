//! The default writes of a resource: form values straight to the model's
//! columns, as ORM statements in the request's unit of work, each
//! recording a change event with the record's serialized form, so hidden
//! fields (`#[model(hidden)]`) and fields serde skips never reach an
//! observer or the audit trail.

use std::collections::BTreeMap;

use chrono::{DateTime, SubsecRound, Utc};
use navree_contracts::Clock;
use navree_db::{ChangeEvent, ChangeOp, Uow};
use navree_http::AppError;
use navree_kernel::AppContext;
use navree_orm::strict;
use navree_orm::{Assignment, Field, Model, OrmError, PrimaryKey, Record, Row, Value, col};
use navree_validation::Messages;
use serde_json::{Map, Value as Json};
use uuid::Uuid;

use crate::values::{self, Invalid, Shape};
use crate::workspace;
use crate::{FormData, SaveContext};

/// The time now, from the app's `dyn Clock` when it has one.
pub(crate) async fn now(app: &AppContext) -> DateTime<Utc> {
    match app.try_get::<dyn Clock>().await {
        Some(clock) => clock.now(),
        None => Utc::now(),
    }
}

/// The app's validation messages, or the defaults.
pub(crate) async fn messages(app: &AppContext) -> Messages {
    match app.try_get::<Messages>().await {
        Some(messages) => (*messages).clone(),
        None => Messages::default(),
    }
}

/// Sets the form's values on `model`, through each field's conversion
/// (`Model::set_attribute`: casts and mutators included); a 400 naming the
/// fields whose values do not fit their columns.
async fn apply<M: Model>(model: &mut M, form: &FormData, app: &AppContext) -> Result<(), AppError> {
    let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut messages = None;
    for (name, value) in form.values() {
        let Some(column) = values::column::<M>(name) else {
            continue;
        };
        let result =
            values::to_input(Shape::of(column), value, form.tz_offset()).and_then(|input| {
                model
                    .set_attribute(name, input)
                    .map_err(|_| Invalid("invalid"))
            });
        if let Err(Invalid(rule)) = result {
            if messages.is_none() {
                messages = Some(self::messages(app).await);
            }
            let message = messages
                .as_ref()
                .map(|m| m.render(name, rule, &[]))
                .unwrap_or_default();
            errors.entry(name.clone()).or_default().push(message);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(errors))
    }
}

/// Sets the date or date-time column `name` to `now` when `M` has it.
fn touch<M: Model>(model: &mut M, name: &str, now: DateTime<Utc>) {
    let shape = values::shape::<M>(name);
    if values::column::<M>(name).is_some() && shape.is_temporal() {
        let _ = model.set_value(name, values::date_bound(shape, now));
    }
}

/// A new, empty model: each field at its type's default. A field whose
/// type has none makes this fail; such a resource overrides
/// `Resource::create`.
async fn blank<M: Model>() -> Result<M, AppError> {
    let lenient = strict::current().prevent_missing_attributes(false);
    strict::scope(lenient, async {
        M::from_row(&Row::from_values(Vec::new()))
    })
    .await
    .map_err(|error| {
        AppError::internal(format!(
            "Helm cannot make a new {} from a form ({error}): override `Resource::create`",
            M::meta().name()
        ))
    })
}

/// The row with `key` as the database stored it, whatever the model's
/// global scopes.
async fn stored<M: Model>(uow: &Uow, key: M::Key) -> Result<Record<M>, AppError> {
    M::query()
        .without_global_scopes()
        .where_key(key.clone())
        .first(uow)
        .await?
        .ok_or_else(|| missing::<M>(&key).into())
}

/// The error for a row that is not there (a 404).
fn missing<M: Model>(key: &M::Key) -> OrmError {
    OrmError::not_found(M::meta().name(), vec![key.to_key_string()])
}

/// `model` as a change event's values: the record's serialized form
/// (hidden fields and fields serde skips left out), without appended
/// accessors.
pub(crate) fn event_json<M: Model>(model: &M) -> Result<Json, AppError> {
    let mut record = Record::new(model.clone());
    record.without_appends();
    Ok(record.to_value()?)
}

/// The fields of `new` whose values differ from `old`: (old values, new
/// values).
fn changed_fields(old: Json, new: Json) -> (Map<String, Json>, Map<String, Json>) {
    let (Json::Object(old), Json::Object(new)) = (old, new) else {
        return (Map::new(), Map::new());
    };
    let mut before = Map::new();
    let mut after = Map::new();
    for (key, value) in new {
        let previous = old.get(&key).cloned().unwrap_or(Json::Null);
        if previous != value {
            before.insert(key.clone(), previous);
            after.insert(key, value);
        }
    }
    (before, after)
}

fn record_change<M: Model>(
    uow: &Uow,
    key: &M::Key,
    op: ChangeOp,
    old: Option<Json>,
    new: Option<Json>,
) {
    uow.record_change(ChangeEvent {
        entity: M::meta().table().to_owned(),
        id: key.to_key_string(),
        op,
        old,
        new,
    });
}

/// The workspace `model` belongs to: `None` when `M` is not scoped to
/// workspaces, `Some(None)` when its workspace column is empty.
fn owner<M: Model>(model: &M) -> Result<Option<Option<Uuid>>, AppError> {
    let Some(column) = M::meta().workspace_column() else {
        return Ok(None);
    };
    let value = model.value(column)?.ok_or_else(|| {
        OrmError::InvalidQuery(format!(
            "`{column}` is not a column of `{}`",
            M::meta().table()
        ))
    })?;
    let id = <Option<Uuid>>::from_value(value).map_err(|error| {
        AppError::internal(format!(
            "the workspace of a {} is not a UUID: {error}",
            M::meta().name()
        ))
    })?;
    Ok(Some(id.filter(|id| !id.is_nil())))
}

/// `Resource::create`'s default. A record of a model that belongs to
/// workspaces is made inside its workspace: the one the unit of work works
/// in, which a form that names another (`workspace_id`) is refused for, or
/// the one the form names when the unit of work is in none.
pub(crate) async fn create<M: Model>(
    ctx: &SaveContext,
    form: FormData,
) -> Result<Record<M>, AppError> {
    let mut model = blank::<M>().await?;
    apply(&mut model, &form, ctx.app()).await?;
    let now = now(ctx.app()).await.trunc_subsecs(6);
    for column in ["created_at", "updated_at"] {
        if !form.contains(column) {
            touch(&mut model, column, now);
        }
    }
    let uow = ctx.uow();
    let Some(named) = owner(&model)? else {
        return insert::<M>(uow, model).await;
    };
    let target = match (named, uow.workspace()) {
        (Some(named), Some(current)) if named != current => {
            return Err(AppError::bad_request(format!(
                "A {} cannot be made in another workspace than the one you are working in.",
                M::meta().name()
            )));
        }
        (_, Some(current)) => current,
        (Some(named), None) => named,
        (None, None) => {
            return Err(AppError::bad_request(format!(
                "Name the workspace of the new {} (`{}`) or choose a workspace first.",
                M::meta().name(),
                M::meta().workspace_column().unwrap_or("workspace_id")
            )));
        }
    };
    // The insert does not fill the workspace itself.
    if let Some(column) = M::meta().workspace_column() {
        model.set_value(column, Value::from(target))?;
    }
    workspace::within(uow, target, insert::<M>(uow, model)).await
}

/// Inserts `model` with a generated key and records the change.
async fn insert<M: Model>(uow: &Uow, model: M) -> Result<Record<M>, AppError> {
    let key = M::query().fill_and_insert_get_id(uow, model).await?;
    let record = stored::<M>(uow, key.clone()).await?;
    record_change::<M>(
        uow,
        &key,
        ChangeOp::Created,
        None,
        Some(event_json(record.model())?),
    );
    Ok(record)
}

/// `Resource::update`'s default. A record of a model that belongs to
/// workspaces is written inside its own workspace, and only its row of that
/// workspace is touched.
pub(crate) async fn update<M: Model>(
    ctx: &SaveContext,
    record: Record<M>,
    form: FormData,
) -> Result<Record<M>, AppError> {
    let old = record.into_inner();
    let mut new = old.clone();
    apply(&mut new, &form, ctx.app()).await?;
    if !form.contains("updated_at") {
        let now = now(ctx.app()).await.trunc_subsecs(6);
        touch(&mut new, "updated_at", now);
    }
    let mut sets = Vec::new();
    for column in new.changed_columns(&old)? {
        let value = new.value(column)?.ok_or_else(|| {
            OrmError::InvalidQuery(format!(
                "`{column}` is not a column of `{}`",
                M::meta().table()
            ))
        })?;
        sets.push(Assignment::value(column, value));
    }
    let uow = ctx.uow();
    match owner(&old)? {
        Some(Some(workspace)) => {
            workspace::within(uow, workspace, write::<M>(uow, old, sets, Some(workspace))).await
        }
        _ => write::<M>(uow, old, sets, None).await,
    }
}

/// The changed columns of an update, written to the row of `old` (of its
/// `workspace`, when it has one).
async fn write<M: Model>(
    uow: &Uow,
    old: M,
    sets: Vec<Assignment>,
    workspace: Option<Uuid>,
) -> Result<Record<M>, AppError> {
    let key = old.key();
    if sets.is_empty() {
        return stored::<M>(uow, key).await;
    }
    let mut query = M::query().without_global_scopes().where_key(key.clone());
    if let (Some(workspace), Some(column)) = (workspace, M::meta().workspace_column()) {
        query = query.where_(col(format!("{}.{column}", M::meta().table())).eq(workspace));
    }
    let updated = query.update(uow, sets).await?;
    if updated == 0 {
        return Err(missing::<M>(&key).into());
    }
    let record = stored::<M>(uow, key.clone()).await?;
    let (before, after) = changed_fields(event_json(&old)?, event_json(record.model())?);
    if !after.is_empty() {
        record_change::<M>(
            uow,
            &key,
            ChangeOp::Updated,
            Some(before.into()),
            Some(after.into()),
        );
    }
    Ok(record)
}

/// `Resource::delete`'s default: a soft delete for a model that soft
/// deletes. A record of a model that belongs to workspaces is deleted inside
/// its own workspace.
pub(crate) async fn delete<M: Model>(ctx: &SaveContext, record: Record<M>) -> Result<(), AppError> {
    let uow = ctx.uow();
    match owner(record.model())? {
        Some(Some(workspace)) => workspace::within(uow, workspace, remove::<M>(uow, record)).await,
        _ => remove::<M>(uow, record).await,
    }
}

async fn remove<M: Model>(uow: &Uow, record: Record<M>) -> Result<(), AppError> {
    let key = record.key();
    if M::query().where_key(key.clone()).delete(uow).await? == 0 {
        return Err(missing::<M>(&key).into());
    }
    record_change::<M>(
        uow,
        &key,
        ChangeOp::Deleted,
        Some(event_json(record.model())?),
        None,
    );
    Ok(())
}
