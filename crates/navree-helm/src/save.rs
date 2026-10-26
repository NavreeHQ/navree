//! The default writes of a resource: form values straight to the entity's
//! columns, through `navree_db::Repo`.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use navree_contracts::Clock;
use navree_db::Repo;
use navree_http::AppError;
use navree_kernel::AppContext;
use navree_validation::Messages;
use sea_orm::sea_query::Value as SqlValue;
use sea_orm::{ActiveModelTrait, EntityTrait, IdenStatic, IntoActiveModel};
use serde::Serialize;

use crate::values::{self, Invalid};
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

/// Sets the form's values on `am`; a 400 naming the fields whose values do
/// not fit their columns.
async fn apply<E: EntityTrait>(
    am: &mut E::ActiveModel,
    form: &FormData,
    app: &AppContext,
) -> Result<(), AppError> {
    let templates = values::templates::<E>();
    let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut messages = None;
    for (name, value) in form.values() {
        let Some(column) = values::column::<E>(name) else {
            continue;
        };
        let template = &templates[name.as_str()];
        let result = values::to_sql(template, value, form.tz_offset())
            .and_then(|sql| am.try_set(column, sql).map_err(|_| Invalid("invalid")));
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

/// Sets the timestamp column `name` to `now` when the entity has it and it
/// is not set already.
fn touch<E: EntityTrait>(am: &mut E::ActiveModel, name: &str, now: DateTime<Utc>, force: bool) {
    let Some(column) = values::column::<E>(name) else {
        return;
    };
    let templates = values::templates::<E>();
    let template = &templates[name];
    if !values::is_temporal(template) {
        return;
    }
    if force || am.is_not_set(column) {
        let _ = am.try_set(column, values::date_bound(template, now));
    }
}

/// `Resource::create`'s default.
pub(crate) async fn create<E>(ctx: &SaveContext, form: FormData) -> Result<E::Model, AppError>
where
    E: EntityTrait,
    E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
    E::ActiveModel: Send + Sync,
{
    let mut am = <E::ActiveModel as ActiveModelTrait>::default();
    apply::<E>(&mut am, &form, ctx.app()).await?;
    let key = values::primary_key::<E>().map_err(AppError::internal)?;
    if am.is_not_set(key) {
        let templates = values::templates::<E>();
        if matches!(templates[key.as_str()], SqlValue::Uuid(_)) {
            let _ = am.try_set(key, SqlValue::Uuid(Some(uuid::Uuid::now_v7())));
        }
    }
    let now = now(ctx.app()).await;
    touch::<E>(&mut am, "created_at", now, false);
    touch::<E>(&mut am, "updated_at", now, false);
    Ok(Repo::<E>::insert(ctx.uow(), am).await?)
}

/// `Resource::update`'s default.
pub(crate) async fn update<E>(
    ctx: &SaveContext,
    record: E::Model,
    form: FormData,
) -> Result<E::Model, AppError>
where
    E: EntityTrait,
    E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
    E::ActiveModel: Send + Sync,
{
    let mut am: E::ActiveModel = record.into_active_model();
    apply::<E>(&mut am, &form, ctx.app()).await?;
    if !form.contains("updated_at") {
        let now = now(ctx.app()).await;
        touch::<E>(&mut am, "updated_at", now, true);
    }
    Ok(Repo::<E>::update(ctx.uow(), am).await?)
}

/// `Resource::delete`'s default.
pub(crate) async fn delete<E>(ctx: &SaveContext, record: E::Model) -> Result<(), AppError>
where
    E: EntityTrait,
    E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
    E::ActiveModel: Send + Sync,
{
    Ok(Repo::<E>::delete(ctx.uow(), record).await?)
}
