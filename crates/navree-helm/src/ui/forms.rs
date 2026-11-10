//! A resource's create and edit forms, and an action's, drawn as a Navree UI form
//! described at run time: one field of the form for each field of the resource,
//! with the rules the browser can check, the values to edit and the errors.

use std::collections::{BTreeMap, HashMap};

use navree_http::AppError;
use navree_ui::described::{Described, DescribedForm, FieldSpec, name};
use navree_ui::form::Widget;
use navree_ui::{
    FactCard, FactGrid, FormView, InputKind, Node, Upload, UploadLimits, UploadedFile,
};
use serde_json::Value;

use super::cells;
use crate::cx::Cx;
use crate::field::{Field, FieldKind};
use crate::handle::ErasedResource;
use crate::money;
use crate::present::{Cell, Display};
use crate::values::key_text;

/// What a form is drawn from.
pub(super) struct FormSpec<'a> {
    /// The form's id: a name, the same for every form of its kind.
    pub(super) id: &'static str,
    pub(super) fields: &'a [&'a Field],
    /// What the user may read of the record being edited.
    pub(super) record: Option<&'a Value>,
    /// The record's page, for what a field shows of its current value (a file).
    pub(super) shown: Option<&'a [Cell]>,
    /// What was posted, after a refused submit: it is shown again.
    pub(super) submitted: Option<&'a HashMap<String, String>>,
    pub(super) errors: &'a BTreeMap<String, Vec<String>>,
    /// The fields of the record the user may not change.
    pub(super) locked: &'a [String],
}

/// `0`, `1.5`: a bound as the browser reads it.
fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

/// The current text of a field in a form; a date-time on `zone`'s clock.
fn input_text(
    field: &Field,
    value: &Value,
    record: Option<&Value>,
    zone: navree_time::Zone,
) -> String {
    match (&field.kind, value) {
        (_, Value::Null) => String::new(),
        (FieldKind::Money { currency_column }, Value::Number(n)) => {
            let currency = record
                .and_then(|r| r.get(currency_column))
                .and_then(Value::as_str)
                .unwrap_or("USD");
            n.as_i64()
                .map(|cents| money::to_major(cents, currency))
                .unwrap_or_else(|| n.to_string())
        }
        (FieldKind::DateTime, Value::String(text)) => chrono::DateTime::parse_from_rfc3339(text)
            .map(|d| {
                zone.local(d.with_timezone(&chrono::Utc))
                    .format("%Y-%m-%dT%H:%M")
                    .to_string()
            })
            .unwrap_or_else(|_| text.clone()),
        (FieldKind::Boolean, value) => match crate::values::boolean(value) {
            Some(true) => "true".to_owned(),
            _ => String::new(),
        },
        (_, value) => key_text(value).unwrap_or_else(|| value.to_string()),
    }
}

/// `(value, label)` of a relationship's choices: the first records of the related
/// resource the user may read, and the `selected` ones.
pub(super) async fn relation_options(
    cx: &Cx,
    related: &dyn ErasedResource,
    selected: &[String],
) -> Result<Vec<(String, String)>, AppError> {
    let mut found = related.lookup(cx, None, None, 100).await?;
    let missing: Vec<String> = selected
        .iter()
        .filter(|id| !found.iter().any(|(f, _)| f == *id))
        .cloned()
        .collect();
    if !missing.is_empty() {
        found.extend(
            related
                .lookup(cx, None, Some(&missing), missing.len() as u64)
                .await?,
        );
    }
    Ok(found)
}

/// The form of `spec`, filled and with its errors, not yet pointed anywhere; and the
/// record's values the user may not change, shown beside it.
pub(super) async fn view(
    cx: &Cx,
    spec: FormSpec<'_>,
) -> Result<(FormView<Described>, Option<Node>), AppError> {
    let FormSpec {
        id,
        fields,
        record,
        shown,
        submitted,
        errors,
        locked,
    } = spec;
    let base = cx.core.options.base();
    let mut form = DescribedForm::new(id);
    let mut values: Vec<(String, String)> = Vec::new();
    let mut choices: Vec<(&'static str, Vec<(String, String)>)> = Vec::new();
    let mut searches: Vec<(&'static str, String)> = Vec::new();
    let mut uploads: Vec<(&'static str, Upload)> = Vec::new();
    let mut fixed: Vec<FactCard> = Vec::new();
    let in_form = |column: &str| fields.iter().any(|field| field.name == column);
    for field in fields {
        let label = field.label_text();
        let stored = record
            .and_then(|r| r.get(&field.name))
            .cloned()
            .or_else(|| {
                if record.is_none() {
                    field.default.clone()
                } else {
                    None
                }
            })
            .unwrap_or(Value::Null);
        // What the user may not change is shown, not edited.
        if field.readonly || locked.contains(&field.name) {
            if record.is_some() {
                let display = shown
                    .and_then(|cells| cells.iter().find(|cell| cell.name == field.name))
                    .map_or(Display::Empty, |cell| cell.display.clone());
                let mut card = FactCard::new(label, cells::node(&display));
                if !field.readonly {
                    card = card.detail("You may not change this field.");
                }
                fixed.push(card);
            }
            continue;
        }
        let text = match submitted.and_then(|s| s.get(&field.name)) {
            Some(text) => text.clone(),
            None => input_text(field, &stored, record, cx.zone),
        };
        // A column of the record the user may not read is left blank, and blank
        // leaves it as it is.
        let unread = record.is_some_and(|r| r.get(&field.name).is_none());
        let required = field.required && !unread;
        let widget = match &field.kind {
            FieldKind::Email => Widget::Input(InputKind::Email),
            FieldKind::Url => Widget::Input(InputKind::Url),
            FieldKind::Number => Widget::Input(InputKind::Number),
            FieldKind::Money { .. } => Widget::Money,
            FieldKind::Textarea => Widget::Textarea { rows: 5 },
            FieldKind::Markdown => Widget::Textarea { rows: 10 },
            FieldKind::Boolean => Widget::Switch,
            FieldKind::Select => Widget::Select,
            FieldKind::Date => Widget::Input(InputKind::Date),
            FieldKind::DateTime => Widget::Input(InputKind::DateTime),
            FieldKind::File { .. } => Widget::File,
            FieldKind::BelongsTo(_) => Widget::Combobox,
            _ => Widget::Input(InputKind::Text),
        };
        let mut spec = FieldSpec::new(&field.name, &label, widget);
        if required {
            spec = spec.required();
        } else {
            spec = spec.optional();
        }
        let help = match (&field.help, unread) {
            (Some(help), _) => Some(help.clone()),
            (None, true) => Some("Leave it blank to keep the current value.".to_owned()),
            (None, false) => None,
        };
        if field.min_length.is_some() || field.max_length.is_some() {
            spec = spec.length(field.min_length, field.max_length);
        }
        let field_name = name(&field.name);
        match &field.kind {
            FieldKind::Number => {
                if field.min.is_some() || field.max.is_some() {
                    spec = spec.range(
                        field.min.map(number_text).as_deref(),
                        field.max.map(number_text).as_deref(),
                    );
                }
            }
            FieldKind::Money { currency_column } => {
                if in_form(currency_column) {
                    spec = spec.addon(currency_column);
                } else if help.is_none() {
                    let currency = record
                        .and_then(|r| r.get(currency_column))
                        .and_then(Value::as_str)
                        .unwrap_or("USD");
                    spec = spec.help(&format!("In {currency}"));
                }
            }
            FieldKind::Markdown if help.is_none() => {
                spec = spec.help("Markdown: **bold**, _italic_, [a link](https://example.com).");
            }
            FieldKind::Select => {
                spec = spec.options(field.options.iter().map(|(v, l)| (v.as_str(), l.as_str())));
                if !required || text.is_empty() {
                    spec = spec.prompt("—");
                }
            }
            FieldKind::BelongsTo(related) => {
                if let Some(target) = cx.core.registry.by_type(related.type_id) {
                    let selected: Vec<String> = (!text.is_empty())
                        .then(|| text.clone())
                        .into_iter()
                        .collect();
                    choices.push((
                        field_name,
                        relation_options(cx, target.as_ref(), &selected).await?,
                    ));
                    searches.push((
                        field_name,
                        format!("{base}/resources/{}/choices", target.info().slug),
                    ));
                }
                spec = spec.placeholder(&format!("Search {}…", label.to_lowercase()));
            }
            FieldKind::File { .. } => {
                let mut limits = UploadLimits::new(field.max_size);
                if !field.accept.is_empty() {
                    limits = limits.accept(field.accept.iter());
                }
                let mut upload = Upload::new(field.name.clone(), label.clone()).limits(limits);
                let current = shown
                    .and_then(|cells| cells.iter().find(|cell| cell.name == field.name))
                    .map(|cell| &cell.display);
                match current {
                    Some(Display::Link { href, text, .. } | Display::Image { href, text, .. }) => {
                        upload = upload.file(UploadedFile::new(text.clone()).href(href.clone()));
                    }
                    Some(Display::Text { text }) if !text.is_empty() => {
                        upload = upload.file(UploadedFile::new(text.clone()));
                    }
                    _ => {}
                }
                uploads.push((field_name, upload));
            }
            _ => {}
        }
        let date_time = matches!(field.kind, FieldKind::DateTime);
        if date_time && cx.zone != navree_time::Zone::UTC {
            // Typed on the reader's clock, which the field names; the toolkit's own instant
            // field is typed in UTC.
            let zone = format!("Time in {}.", cx.zone.name());
            spec = spec.help(&match &help {
                Some(help) if help.ends_with(['.', '!', '?']) => format!("{help} {zone}"),
                Some(help) => format!("{help}. {zone}"),
                None => zone,
            });
        } else {
            if let Some(help) = &help {
                spec = spec.help(help);
            }
            if date_time {
                spec = spec.instant();
            }
        }
        form = form.field(spec);
        // A file field's value is the file chosen, never the path it is stored at.
        if !matches!(field.kind, FieldKind::File { .. }) && !text.is_empty() {
            values.push((field.name.clone(), text));
        }
    }
    // The post says it is this form's, so a switch it left off reads as off.
    let mut view = form
        .view()
        .hidden("_form", id)
        .with_values(values)
        .with_errors(errors.clone());
    for (field, options) in choices {
        view = view.options(field, options);
    }
    for (field, url) in searches {
        view = view.search(field, url);
    }
    for (field, upload) in uploads {
        view = view.upload(field, upload);
    }
    let fixed = (!fixed.is_empty()).then(|| FactGrid::new().cards(fixed).into());
    Ok((view, fixed))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn values_are_shown_as_the_form_takes_them() {
        const UTC: navree_time::Zone = navree_time::Zone::UTC;
        let money = Field::money("amount_cents", "currency");
        assert_eq!(
            input_text(
                &money,
                &json!(1250),
                Some(&json!({ "currency": "USD" })),
                UTC
            ),
            "12.50"
        );
        let at = Field::datetime("closed_at");
        assert_eq!(
            input_text(&at, &json!("2026-10-26T12:00:00Z"), None, UTC),
            "2026-10-26T12:00"
        );
        let kolkata = navree_time::Zone::parse("Asia/Kolkata").unwrap();
        assert_eq!(
            input_text(&at, &json!("2026-10-26T12:00:00Z"), None, kolkata),
            "2026-10-26T17:30",
            "a date-time is typed on the reader's clock"
        );
        let flag = Field::boolean("priority");
        assert_eq!(input_text(&flag, &json!(true), None, UTC), "true");
        assert_eq!(input_text(&flag, &json!(false), None, UTC), "");
        assert_eq!(
            input_text(&Field::text("title"), &Value::Null, None, UTC),
            ""
        );
        assert_eq!(number_text(0.0), "0");
        assert_eq!(number_text(1.5), "1.5");
    }
}
