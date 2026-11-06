//! The Settings page: the numbers that can change while the app runs, and
//! the options that are set in code or configuration.

use std::collections::BTreeMap;

use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, ClientIp};
use navree_ui::{
    Button, ButtonVariant, Field, Heading, Input, Node, Reply, Row, Stack, TableRowEl, Text,
    ToastTone,
};
use serde_json::{Value, json};

use super::{cell, code, head, post_form, table, text_of, titled};
use crate::config_api::{Actor, SettingsInput};
use crate::config_store::setting_rows;
use crate::dashboard::{LighthouseDashboardController, Viewer};
use crate::settings as tuning_settings;
use crate::settings_view::read_only;

/// The form's field for a setting.
fn field_name(key: &str) -> String {
    format!("value.{key}")
}

/// One setting: its label, what it does, what the configuration says and the
/// number.
fn setting(viewer: &Viewer, row: &Value) -> Node {
    let key = text_of(row, "key");
    let unit = text_of(row, "unit");
    let id = format!("setting-{}", key.replace('.', "-"));
    let step = if text_of(row, "kind") == "fraction" {
        "0.01"
    } else {
        "1"
    };
    let input = Input::number(field_name(&key))
        .value(text_of(row, "current"))
        .attr("min", text_of(row, "min"))
        .attr("max", text_of(row, "max"))
        .attr("step", step)
        .disabled(!viewer.can_configure);
    let changed = row["changed"].as_bool().unwrap_or(false);
    let mut configured = format!("Configured: {}", text_of(row, "configured"));
    if !unit.is_empty() {
        configured.push_str(&format!(" {unit}"));
    }
    if changed {
        let at = text_of(row, "updated_at");
        let at = at.get(..16).unwrap_or(&at).replace('T', " ");
        configured.push_str(&format!(" · changed {at} UTC"));
    }
    let mut controls: Vec<Node> = vec![input.into()];
    if !unit.is_empty() {
        controls.push(Text::muted(unit).inline().into());
    }
    if viewer.can_configure && changed {
        controls.push(
            Button::new("Use configured")
                .variant(ButtonVariant::Outline)
                .named("reset", key.clone())
                .submit()
                .into(),
        );
    }
    Field::new(id, text_of(row, "label"))
        .help(Node::list(vec![
            Node::from(text_of(row, "help")),
            Node::from(" "),
            code(configured),
        ]))
        .control(Row::gap(2).wrap().children(controls))
        .into()
}

/// The settings, grouped.
fn changeable(viewer: &Viewer, mount: &str, rows: &[Value]) -> Node {
    let mut groups: Vec<(String, Vec<Node>)> = Vec::new();
    for row in rows {
        let group = text_of(row, "group");
        let node = setting(viewer, row);
        match groups.iter_mut().find(|(name, _)| *name == group) {
            Some((_, items)) => items.push(node),
            None => groups.push((group, vec![node])),
        }
    }
    let mut parts: Vec<Node> = vec![
        Text::muted(
            "A change is stored with the database and reaches every process (web servers, workers, the scheduler) within fifteen seconds. Returning a setting to the configured value removes the change.",
        )
        .into(),
    ];
    if !viewer.can_configure {
        parts.push(
            Text::muted(Node::list(vec![
                Node::from("Read-only: changing settings needs the "),
                code("lighthouse.configure"),
                Node::from(" permission."),
            ]))
            .into(),
        );
    }
    for (name, items) in groups {
        parts.push(Heading::h3(name).into());
        parts.push(Stack::gap(4).children(items).into());
    }
    if viewer.can_configure {
        parts.push(
            Row::gap(2)
                .children(
                    Button::primary("Save changes")
                        .named("action", "save")
                        .submit(),
                )
                .into(),
        );
    }
    let body = Stack::gap(4).children(parts);
    if viewer.can_configure {
        post_form(format!("{mount}/settings"), viewer.csrf.as_deref(), body).into()
    } else {
        body.into()
    }
}

/// The options set at boot, in groups, with how each is set.
fn fixed(options: &[Value]) -> Node {
    let mut parts: Vec<Node> = vec![
        Text::muted(Node::list(vec![
            Node::from("These are read when the app boots and cannot be changed here. Set them under "),
            code("[lighthouse]"),
            Node::from(" in the configuration, with the environment variable shown, or in "),
            code("LighthouseOptions"),
            Node::from(" in code. Secrets are never shown, only whether they are set."),
        ]))
        .into(),
    ];
    for group in options {
        parts.push(Heading::h3(text_of(group, "group")).into());
        let items = group["items"].as_array().cloned().unwrap_or_default();
        let rows: Vec<TableRowEl> = items
            .iter()
            .map(|item| {
                let key = text_of(item, "key");
                let value = text_of(item, "value");
                let value: Node = if item["secret"].as_bool().unwrap_or(false) {
                    Text::muted(value).inline().into()
                } else {
                    code(value)
                };
                TableRowEl::new(vec![
                    cell(Node::list(vec![
                        Node::from(text_of(item, "label")),
                        Text::muted(format!("{key} · {}", text_of(item, "env"))).into(),
                    ])),
                    cell(value),
                ])
                .key(key)
            })
            .collect();
        parts.push(table(vec![head("Option"), head("Value")], rows, "Nothing to show."));
    }
    Stack::gap(4).children(parts).into()
}

/// The page.
pub(crate) async fn page(
    c: &LighthouseDashboardController,
    viewer: Viewer,
) -> Result<Response, AppError> {
    let stored = c.store()?.stored_settings().await?;
    let rows = setting_rows(c.lighthouse.tuning(), &stored);
    let mount = c.mount().to_owned();
    let options = read_only(c.lighthouse.options());
    let page = c.page(&viewer, "Settings").await?.body((
        Text::muted(
            "What can be changed here while the app runs, and what is set in code or configuration.",
        ),
        titled("Changeable here", changeable(&viewer, &mount, &rows)),
        titled("Set in code or configuration", fixed(&options)),
    ));
    Ok(page.into_response())
}

/// The changes a posted form asks for: setting key to its new number, or
/// `None` to return it to the configured value. A field left as it is, or a
/// number equal to the one in use, is no change.
fn changes_of(
    form: &[(String, String)],
    current: impl Fn(&str) -> Value,
) -> Result<BTreeMap<String, Option<Value>>, AppError> {
    let mut changes: BTreeMap<String, Option<Value>> = BTreeMap::new();
    if let Some((_, key)) = form.iter().find(|(name, _)| name == "reset") {
        if tuning_settings::definition(key).is_none() {
            return Err(AppError::bad_request(format!(
                "`{key}` is not a setting that can change"
            )));
        }
        changes.insert(key.clone(), None);
        return Ok(changes);
    }
    for (name, text) in form {
        let Some(key) = name.strip_prefix("value.") else {
            continue;
        };
        let Some(def) = tuning_settings::definition(key) else {
            return Err(AppError::bad_request(format!(
                "`{key}` is not a setting that can change"
            )));
        };
        let text = text.trim();
        if text.is_empty() {
            changes.insert(key.to_owned(), None);
            continue;
        }
        let number: f64 = text
            .parse()
            .map_err(|_| AppError::bad_request(format!("{} must be a number", def.label)))?;
        let unchanged = current(key)
            .as_f64()
            .is_some_and(|now| (now - number).abs() < 1e-9);
        if !unchanged {
            changes.insert(key.to_owned(), Some(json!(number)));
        }
    }
    Ok(changes)
}

/// Saves the changes of the form.
pub(crate) async fn change(
    c: &LighthouseDashboardController,
    viewer: &Viewer,
    ip: ClientIp,
    form: Vec<(String, String)>,
) -> Result<Reply, AppError> {
    let tuning = c.lighthouse.tuning();
    let changes = changes_of(&form, |key| tuning.current(key))?;
    let back = Reply::redirect(format!("{}/settings", c.mount()));
    if changes.is_empty() {
        return Ok(back.toast(ToastTone::Info, "Nothing changed."));
    }
    let actor = Actor::new(viewer.user_id, ip, &viewer.headers);
    c.configure
        .change_settings(actor, SettingsInput::from_changes(changes))
        .await?;
    Ok(back.toast(
        ToastTone::Success,
        "Saved. Every process picks the change up within fifteen seconds.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn current(key: &str) -> Value {
        match key {
            "thresholds.slow_request_ms" => json!(500),
            "sample.requests" => json!(0.1),
            _ => Value::Null,
        }
    }

    #[test]
    fn only_a_number_that_differs_is_a_change() {
        let form = pairs(&[
            ("action", "save"),
            ("value.thresholds.slow_request_ms", "500"),
            ("value.sample.requests", "0.25"),
        ]);
        let changes = changes_of(&form, current).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes["sample.requests"], Some(json!(0.25)));
    }

    #[test]
    fn an_emptied_field_and_a_reset_return_to_the_configured_value() {
        let emptied = pairs(&[("value.thresholds.slow_request_ms", "  ")]);
        let changes = changes_of(&emptied, current).unwrap();
        assert_eq!(changes["thresholds.slow_request_ms"], None);
        let reset = pairs(&[
            ("reset", "sample.requests"),
            ("value.thresholds.slow_request_ms", "900"),
        ]);
        let changes = changes_of(&reset, current).unwrap();
        assert_eq!(changes.len(), 1, "a reset changes only its own setting");
        assert_eq!(changes["sample.requests"], None);
    }

    #[test]
    fn unknown_keys_and_words_for_numbers_are_refused() {
        assert!(changes_of(&pairs(&[("value.nope", "1")]), current).is_err());
        assert!(changes_of(&pairs(&[("reset", "nope")]), current).is_err());
        assert!(
            changes_of(
                &pairs(&[("value.thresholds.slow_request_ms", "soon")]),
                current
            )
            .is_err()
        );
        assert!(changes_of(&pairs(&[("action", "save")]), current)
            .unwrap()
            .is_empty());
    }
}
