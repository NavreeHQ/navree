//! Reading what a Helm form sent, and announcing changes to webhook
//! endpoints.
//!
//! A form value is `None` when the form did not send the field, `Some(None)`
//! when it sent it empty (the field is cleared) and `Some(Some(value))`
//! when it sent a value. A value that cannot be read is a validation error
//! under its field, which the panel shows next to it.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use navree::helm::{ActionContext, FormData};
use navree::prelude::*;
use navree::webhooks::Webhooks;
use serde_json::Value;
use uuid::Uuid;

/// A validation error on one field.
pub fn invalid(field: &str, message: &str) -> AppError {
    let mut errors = BTreeMap::new();
    errors.insert(field.to_owned(), vec![message.to_owned()]);
    AppError::validation(errors)
}

/// Whether a value counts as empty: `null`, or text of blanks.
fn blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        _ => false,
    }
}

/// The trimmed text of `field`.
pub fn text(form: &FormData, field: &str) -> Option<Option<String>> {
    let value = form.get(field)?;
    if blank(value) {
        return Some(None);
    }
    Some(Some(match value {
        Value::String(text) => text.trim().to_owned(),
        other => other.to_string(),
    }))
}

/// The id in `field`.
pub fn uuid(form: &FormData, field: &str) -> Result<Option<Option<Uuid>>, AppError> {
    let Some(value) = form.get(field) else {
        return Ok(None);
    };
    if blank(value) {
        return Ok(Some(None));
    }
    value
        .as_str()
        .and_then(|text| text.trim().parse::<Uuid>().ok())
        .map(|id| Some(Some(id)))
        .ok_or_else(|| invalid(field, &format!("{field} must be a UUID")))
}

/// The date in `field` (`YYYY-MM-DD`).
pub fn date(form: &FormData, field: &str) -> Result<Option<Option<NaiveDate>>, AppError> {
    let Some(value) = form.get(field) else {
        return Ok(None);
    };
    if blank(value) {
        return Ok(Some(None));
    }
    value
        .as_str()
        .and_then(|text| NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok())
        .map(|date| Some(Some(date)))
        .ok_or_else(|| {
            invalid(
                field,
                &format!("{field} must be a valid ISO 8601 date string"),
            )
        })
}

/// The whole number in `field`, sent as a number or as text.
pub fn number(form: &FormData, field: &str) -> Result<Option<Option<i32>>, AppError> {
    let Some(value) = form.get(field) else {
        return Ok(None);
    };
    number_of(field, value)
}

/// The whole number in `value`, which belongs to `field`.
pub fn number_of(field: &str, value: &Value) -> Result<Option<Option<i32>>, AppError> {
    if blank(value) {
        return Ok(Some(None));
    }
    let parsed = match value {
        Value::Number(n) => n.as_i64(),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    };
    parsed
        .and_then(|n| i32::try_from(n).ok())
        .map(|n| Some(Some(n)))
        .ok_or_else(|| invalid(field, &format!("{field} must be a whole number")))
}

/// Announces `event_type` to the webhook endpoints of `workspace`. A
/// failure to queue the announcement is logged: the change it announces
/// stands.
pub async fn announce(ctx: &ActionContext, event_type: &str, workspace: Uuid, data: Value) {
    let result = async {
        let webhooks = ctx.get::<Webhooks>().await?;
        webhooks
            .dispatch_json(event_type.to_owned(), data)
            .workspace(workspace)
            .send()
            .await
            .map_err(AppError::internal)
    }
    .await;
    if let Err(error) = result {
        let reason = error
            .source
            .as_ref()
            .map_or_else(|| error.title.clone(), ToString::to_string);
        tracing::warn!(event = event_type, %workspace, "the webhook announcement was not queued: {reason}");
    }
}
