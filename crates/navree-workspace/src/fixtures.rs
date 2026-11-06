//! Demo fixtures: JSON files under `workspace.demo.fixtures`, with dates
//! written relative to the time of seeding.
//!
//! A string that is exactly a placeholder is replaced before the fixture is
//! deserialized:
//!
//! | Placeholder | Becomes |
//! |---|---|
//! | `{{ now }}` | the seeding time, RFC 3339 (`2026-10-27T09:00:00Z`) |
//! | `{{ now + 3d }}`, `{{ now - 90m }}` | that time moved by minutes (`m`), hours (`h`), days (`d`) or weeks (`w`) |
//! | `{{ today }}` | the seeding date (`2026-10-27`) |
//! | `{{ today - 1w }}`, `{{ today + 2d }}` | that date moved by days (`d`) or weeks (`w`) |
//!
//! Other strings, `{{ name }}` in a template body among them, are left as
//! they are.

use std::path::{Component, Path};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use navree_kernel::BoxError;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Parses fixture `text` (JSON), resolves its placeholders against `now`
/// and deserializes it.
pub(crate) fn parse<T: DeserializeOwned>(text: &str, now: DateTime<Utc>) -> Result<T, BoxError> {
    let mut value: Value = serde_json::from_str(text)?;
    resolve(&mut value, now)?;
    Ok(serde_json::from_value(value)?)
}

/// Reads fixture `name` (`<dir>/<name>.json`), resolves its placeholders
/// against `now` and deserializes it.
pub(crate) fn read<T: DeserializeOwned>(
    dir: &Path,
    name: &str,
    now: DateTime<Utc>,
) -> Result<T, BoxError> {
    let relative = Path::new(name);
    let plain = !name.trim().is_empty()
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if !plain {
        return Err(format!(
            "fixture name `{name}` must be a relative path without `..` (such as `crm/deals`)"
        )
        .into());
    }
    let path = dir.join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read fixture `{}`: {error}", path.display()))?;
    parse(&text, now).map_err(|error| format!("fixture `{}`: {error}", path.display()).into())
}

/// Replaces every placeholder string in `value`.
pub(crate) fn resolve(value: &mut Value, now: DateTime<Utc>) -> Result<(), BoxError> {
    match value {
        Value::String(text) => {
            if let Some(replacement) = placeholder(text, now)? {
                *text = replacement;
            }
        }
        Value::Array(items) => {
            for item in items {
                resolve(item, now)?;
            }
        }
        Value::Object(members) => {
            for member in members.values_mut() {
                resolve(member, now)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

/// The replacement of `text` when it is a `now` or `today` placeholder;
/// `None` for any other string; an error for a malformed one.
fn placeholder(text: &str, now: DateTime<Utc>) -> Result<Option<String>, BoxError> {
    let Some(inner) = text
        .trim()
        .strip_prefix("{{")
        .and_then(|rest| rest.strip_suffix("}}"))
    else {
        return Ok(None);
    };
    let expression: String = inner.chars().filter(|c| !c.is_whitespace()).collect();
    let (base, offset) = if let Some(offset) = expression.strip_prefix("today") {
        ("today", offset)
    } else if let Some(offset) = expression.strip_prefix("now") {
        ("now", offset)
    } else {
        return Ok(None);
    };
    let malformed = || -> BoxError {
        format!(
            "malformed fixture placeholder `{text}`: write `{{{{ now }}}}`, `{{{{ now + 3d }}}}` or `{{{{ today - 1w }}}}` (units m, h, d, w; dates take d and w)"
        )
        .into()
    };
    let shift = if offset.is_empty() {
        Duration::zero()
    } else {
        let (sign, amount) = if let Some(amount) = offset.strip_prefix('+') {
            (1, amount)
        } else if let Some(amount) = offset.strip_prefix('-') {
            (-1, amount)
        } else {
            // `{{ nowhere }}`: not a placeholder.
            return Ok(None);
        };
        let unit = amount.chars().last().ok_or_else(malformed)?;
        let number: i64 = amount[..amount.len() - unit.len_utf8()]
            .parse()
            .map_err(|_| malformed())?;
        let shift = match (base, unit) {
            ("now", 'm') => Duration::try_minutes(number),
            ("now", 'h') => Duration::try_hours(number),
            (_, 'd') => Duration::try_days(number),
            (_, 'w') => Duration::try_weeks(number),
            _ => None,
        };
        shift.ok_or_else(malformed)? * sign
    };
    let at = now.checked_add_signed(shift).ok_or_else(malformed)?;
    Ok(Some(match base {
        "today" => at.date_naive().format("%Y-%m-%d").to_string(),
        _ => at.to_rfc3339_opts(SecondsFormat::Secs, true),
    }))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 27, 9, 30, 0).unwrap()
    }

    fn resolved(value: Value) -> Value {
        let mut value = value;
        resolve(&mut value, now()).unwrap();
        value
    }

    #[test]
    fn placeholders_become_times_and_dates_relative_to_now() {
        assert_eq!(
            resolved(json!({
                "now": "{{now}}",
                "later": "{{ now + 3d }}",
                "earlier": "{{now-90m}}",
                "hours": "{{ now + 2h }}",
                "today": "{{ today }}",
                "last_week": "{{today - 1w}}",
                "nested": [{ "due": "{{ today + 2d }}" }],
            })),
            json!({
                "now": "2026-10-27T09:30:00Z",
                "later": "2026-10-30T09:30:00Z",
                "earlier": "2026-10-27T08:00:00Z",
                "hours": "2026-10-27T11:30:00Z",
                "today": "2026-10-27",
                "last_week": "2026-10-20",
                "nested": [{ "due": "2026-10-29" }],
            })
        );
    }

    #[test]
    fn other_strings_and_values_are_kept() {
        let value = json!({
            "template": "Hello {{ name }}",
            "whole": "{{ name }}",
            "number": 3,
            "flag": true,
            "nothing": null,
            "text": "now + 3d",
            "lookalike": "{{ nowhere }}",
            "times": "{{ today * 2 }}",
        });
        assert_eq!(resolved(value.clone()), value);
    }

    #[test]
    fn malformed_placeholders_are_errors() {
        for text in [
            "{{ now + 3x }}",
            "{{ now + d }}",
            "{{ today + 3h }}",
            "{{ now + }}",
            "{{ now - €d }}",
            "{{ now + 99999999999w }}",
        ] {
            let mut value = json!(text);
            let error = resolve(&mut value, now()).unwrap_err().to_string();
            assert!(
                error.contains("malformed fixture placeholder"),
                "{text}: {error}"
            );
        }
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Task {
        title: String,
        due: chrono::NaiveDate,
        created_at: DateTime<Utc>,
    }

    #[test]
    fn fixtures_are_read_and_deserialized() {
        let dir =
            std::env::temp_dir().join(format!("navree-test-fixtures-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(dir.join("crm")).unwrap();
        std::fs::write(
            dir.join("crm").join("tasks.json"),
            r#"[{ "title": "Call Anu", "due": "{{ today + 1d }}", "created_at": "{{ now - 2d }}" }]"#,
        )
        .unwrap();
        let tasks: Vec<Task> = read(&dir, "crm/tasks", now()).unwrap();
        assert_eq!(
            tasks,
            [Task {
                title: "Call Anu".into(),
                due: chrono::NaiveDate::from_ymd_opt(2026, 10, 28).unwrap(),
                created_at: Utc.with_ymd_and_hms(2026, 10, 25, 9, 30, 0).unwrap(),
            }]
        );
        let missing = read::<Value>(&dir, "missing", now())
            .unwrap_err()
            .to_string();
        assert!(missing.contains("missing.json"), "{missing}");
        for name in ["../secrets", "", "/etc/passwd"] {
            let error = read::<Value>(&dir, name, now()).unwrap_err().to_string();
            assert!(error.contains("must be a relative path"), "{name}: {error}");
        }
        std::fs::write(dir.join("bad.json"), "[{").unwrap();
        let bad = read::<Value>(&dir, "bad", now()).unwrap_err().to_string();
        assert!(bad.contains("bad.json"), "{bad}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
