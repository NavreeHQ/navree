//! Turning [`RawRecord`]s into what the screens and the API show.

use chrono::{DateTime, Duration, Utc};
use navree_access::{DELETE, READ, UPDATE};
use navree_http::AppError;
use navree_storage::Storage;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use crate::cx::Cx;
use crate::field::{Field, FieldKind};
use crate::handle::{ErasedResource, RawRecord, ResourceInfo};
use crate::values::key_text;
use crate::{markdown, money, save};

/// Where a record is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Screen {
    Index,
    Detail,
}

/// How one value is shown.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Display {
    /// No value.
    Empty,
    /// A value the user's rules do not let them read.
    Hidden,
    Text {
        text: String,
    },
    Multiline {
        text: String,
    },
    /// Trusted HTML: rendered Markdown or a view template.
    Html {
        html: String,
    },
    Link {
        href: String,
        text: String,
        external: bool,
    },
    Badge {
        text: String,
        tone: u8,
    },
    Bool {
        value: bool,
    },
    Time {
        iso: String,
        text: String,
        date_only: bool,
    },
    Image {
        src: String,
        href: String,
        text: String,
    },
}

impl Display {
    /// The value as plain text (the API's `display`).
    pub(crate) fn text(&self) -> String {
        match self {
            Display::Empty | Display::Hidden => String::new(),
            Display::Text { text }
            | Display::Multiline { text }
            | Display::Link { text, .. }
            | Display::Badge { text, .. }
            | Display::Time { text, .. }
            | Display::Image { text, .. } => text.clone(),
            Display::Html { html } => strip_tags(html),
            Display::Bool { value } => if *value { "Yes" } else { "No" }.to_owned(),
        }
    }
}

/// One cell of a row, or one line of a detail page.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Cell {
    pub(crate) name: String,
    pub(crate) label: String,
    pub(crate) kind: &'static str,
    pub(crate) display: Display,
}

/// What the user may do with a record.
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Can {
    pub(crate) update: bool,
    pub(crate) delete: bool,
}

/// A record, ready to show.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) values: Map<String, Value>,
    pub(crate) cells: Vec<Cell>,
    pub(crate) can: Can,
    /// The fields the user may not change (detail pages).
    pub(crate) readonly: Vec<String>,
}

impl Item {
    /// The API form: `{id, title, values, display, can}` (and `readonly`
    /// on detail).
    pub(crate) fn api(&self, detail: bool) -> Value {
        let display: Map<String, Value> = self
            .cells
            .iter()
            .filter(|c| c.display != Display::Hidden)
            .map(|c| (c.name.clone(), Value::String(c.display.text())))
            .collect();
        let mut out = json!({
            "id": self.id,
            "title": self.title,
            "values": self.values,
            "display": display,
            "can": self.can,
        });
        if detail {
            out["readonly"] = json!(self.readonly);
        }
        out
    }
}

/// The fields of `info` shown on `screen` (not `has_many` panels).
pub(crate) fn screen_fields(info: &ResourceInfo, screen: Screen) -> Vec<&Field> {
    info.fields
        .iter()
        .filter(|f| match screen {
            Screen::Index => f.on_index,
            Screen::Detail => f.on_detail,
        })
        .filter(|f| !matches!(f.kind, FieldKind::HasMany(_)))
        .collect()
}

/// Presents `records` of `resource` for `screen`.
pub(crate) async fn present(
    cx: &Cx,
    resource: &dyn ErasedResource,
    records: &[RawRecord],
    screen: Screen,
) -> Result<Vec<Item>, AppError> {
    let info = resource.info();
    let base = cx.core.options.base().to_owned();
    let fields = screen_fields(info, screen);
    // The titles of related records, per field.
    let mut related: HashMap<String, (Option<String>, HashMap<String, String>)> = HashMap::new();
    for field in &fields {
        let FieldKind::BelongsTo(target) = &field.kind else {
            continue;
        };
        let Some(handle) = cx.core.registry.by_type(target.type_id) else {
            continue;
        };
        let mut ids: Vec<String> = records
            .iter()
            .filter_map(|r| r.json.get(&field.name).and_then(key_text))
            .collect();
        ids.sort();
        ids.dedup();
        let titles = if ids.is_empty() {
            HashMap::new()
        } else {
            handle
                .lookup(cx, None, Some(&ids), ids.len() as u64)
                .await?
                .into_iter()
                .collect()
        };
        let readable = cx.can(READ, handle.info().subject, None);
        let slug = readable.then(|| handle.info().slug.clone());
        related.insert(field.name.clone(), (slug, titles));
    }
    let storage = cx.app.try_get::<Storage>().await;
    let now = save::now(&cx.app).await;
    let ability = cx.ability();
    let subject = info.subject;
    let mut items = Vec::with_capacity(records.len());
    let mut workspaces = HashMap::new();
    for record in records {
        let json = &record.json;
        let mut values = Map::new();
        let mut cells = Vec::new();
        for field in &fields {
            let readable =
                !field.is_column() || ability.can_field(READ, subject, Some(json), &field.name);
            let display = if readable {
                display(
                    field,
                    record,
                    screen,
                    &base,
                    related.get(&field.name),
                    storage.as_deref(),
                    now,
                )
                .await
            } else {
                Display::Hidden
            };
            if readable {
                let value = match &field.kind {
                    FieldKind::Computed(_) => record
                        .computed
                        .get(&field.name)
                        .cloned()
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                    FieldKind::View { .. } => Value::Null,
                    _ => json.get(&field.name).cloned().unwrap_or(Value::Null),
                };
                if !matches!(field.kind, FieldKind::View { .. }) {
                    values.insert(field.name.clone(), value);
                }
            }
            cells.push(Cell {
                name: field.name.clone(),
                label: field.label_text(),
                kind: field.kind.name(),
                display,
            });
        }
        let readonly = if screen == Screen::Detail {
            info.fields
                .iter()
                .filter(|f| f.in_forms())
                .filter(|f| {
                    !f.editable() || !ability.can_field(UPDATE, subject, Some(json), &f.name)
                })
                .map(|f| f.name.clone())
                .collect()
        } else {
            Vec::new()
        };
        // The workspace of the record, for a resource that shows several.
        let owner = info
            .workspace_column
            .and_then(|column| json.get(column))
            .and_then(key_text)
            .and_then(|text| uuid::Uuid::parse_str(&text).ok());
        let can = Can {
            update: ability.can(UPDATE, subject, Some(json))
                && cx.offers_for(info, UPDATE, owner, &mut workspaces).await,
            delete: ability.can(DELETE, subject, Some(json))
                && cx.offers_for(info, DELETE, owner, &mut workspaces).await,
        };
        items.push(Item {
            url: format!("{base}/resources/{}/{}", info.slug, encode(&record.id)),
            id: record.id.clone(),
            title: record.title.clone(),
            values,
            cells,
            can,
            readonly,
        });
    }
    Ok(items)
}

/// How `field` of `record` is shown.
async fn display(
    field: &Field,
    record: &RawRecord,
    screen: Screen,
    base: &str,
    related: Option<&(Option<String>, HashMap<String, String>)>,
    storage: Option<&Storage>,
    now: DateTime<Utc>,
) -> Display {
    let value = record.json.get(&field.name).unwrap_or(&Value::Null);
    match &field.kind {
        FieldKind::Computed(_) => match record.computed.get(&field.name) {
            Some(text) if !text.is_empty() => Display::Text { text: text.clone() },
            _ => Display::Empty,
        },
        FieldKind::View { .. } => match record.html.get(&field.name) {
            Some(html) => Display::Html { html: html.clone() },
            None => Display::Empty,
        },
        FieldKind::HasMany(_) => Display::Empty,
        _ if value.is_null() => Display::Empty,
        FieldKind::Id | FieldKind::Text => Display::Text {
            text: key_text(value).unwrap_or_else(|| value.to_string()),
        },
        FieldKind::Textarea => {
            let text = key_text(value).unwrap_or_default();
            match screen {
                Screen::Index => Display::Text {
                    text: markdown::truncate(text.lines().next().unwrap_or(""), 120),
                },
                Screen::Detail => Display::Multiline { text },
            }
        }
        FieldKind::Markdown => {
            let text = key_text(value).unwrap_or_default();
            match screen {
                Screen::Index => Display::Text {
                    text: markdown::first_line(&text, 120),
                },
                Screen::Detail => Display::Html {
                    html: markdown::render(&text),
                },
            }
        }
        FieldKind::Number => match value.as_f64() {
            Some(n) => Display::Text {
                text: money::format_number(n),
            },
            None => Display::Text {
                text: value.to_string(),
            },
        },
        FieldKind::Money { currency_column } => {
            let currency = record
                .json
                .get(currency_column)
                .and_then(Value::as_str)
                .filter(|c| !c.trim().is_empty())
                .unwrap_or("USD");
            match value
                .as_i64()
                .or_else(|| value.as_f64().map(|f| f.round() as i64))
            {
                Some(cents) => Display::Text {
                    text: money::format(cents, currency),
                },
                None => Display::Text {
                    text: value.to_string(),
                },
            }
        }
        FieldKind::Boolean => Display::Bool {
            value: crate::values::boolean(value).unwrap_or(false),
        },
        FieldKind::Select => {
            let key = key_text(value).unwrap_or_default();
            let text = field
                .options
                .iter()
                .find(|(v, _)| *v == key)
                .map(|(_, label)| label.clone())
                .unwrap_or_else(|| key.clone());
            if field.badge {
                Display::Badge {
                    tone: tone(&key),
                    text,
                }
            } else {
                Display::Text { text }
            }
        }
        FieldKind::Date => {
            let text = key_text(value).unwrap_or_default();
            let pretty = crate::values::date(value)
                .map(|d| d.format("%-d %b %Y").to_string())
                .unwrap_or_else(|| text.clone());
            Display::Time {
                iso: text,
                text: pretty,
                date_only: true,
            }
        }
        FieldKind::DateTime => {
            let text = key_text(value).unwrap_or_default();
            let pretty = DateTime::parse_from_rfc3339(&text)
                .map(|d| {
                    d.with_timezone(&Utc)
                        .format("%-d %b %Y %H:%M UTC")
                        .to_string()
                })
                .unwrap_or_else(|_| text.clone());
            Display::Time {
                iso: text,
                text: pretty,
                date_only: false,
            }
        }
        FieldKind::Email => {
            let text = key_text(value).unwrap_or_default();
            if crate::field::is_email(&text) {
                Display::Link {
                    href: format!("mailto:{text}"),
                    text,
                    external: true,
                }
            } else {
                Display::Text { text }
            }
        }
        FieldKind::Url => {
            let text = key_text(value).unwrap_or_default();
            if crate::field::is_http_url(&text) {
                Display::Link {
                    href: text.clone(),
                    text,
                    external: true,
                }
            } else {
                Display::Text { text }
            }
        }
        FieldKind::File { disk } => {
            let path = key_text(value).unwrap_or_default();
            let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
            // The disk's public URL, else a link signed for an hour.
            let url = match storage.and_then(|s| s.disk(disk).ok()) {
                Some(disk) => match disk.url(&path) {
                    Ok(url) => Some(url),
                    Err(_) => disk
                        .temporary_url(&path, now + Duration::hours(1))
                        .await
                        .ok(),
                },
                None => None,
            };
            match url {
                Some(href) if is_image(&path) => Display::Image {
                    src: href.clone(),
                    href,
                    text: name,
                },
                Some(href) => Display::Link {
                    href,
                    text: name,
                    external: false,
                },
                None => Display::Text { text: name },
            }
        }
        FieldKind::BelongsTo(_) => {
            let id = key_text(value).unwrap_or_default();
            match related {
                Some((slug, titles)) => match (slug, titles.get(&id)) {
                    (Some(slug), Some(title)) => Display::Link {
                        href: format!("{base}/resources/{slug}/{}", encode(&id)),
                        text: title.clone(),
                        external: false,
                    },
                    (None, Some(title)) => Display::Text {
                        text: title.clone(),
                    },
                    _ => Display::Text { text: id },
                },
                None => Display::Text { text: id },
            }
        }
    }
}

/// A stable badge color (0 to 5) for a value.
fn tone(value: &str) -> u8 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    (hasher.finish() % 6) as u8
}

/// Whether a stored file is an image the browser can preview (never SVG).
fn is_image(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Percent-encodes a path segment.
pub(crate) fn encode(segment: &str) -> String {
    form_urlencoded::byte_serialize(segment.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

/// HTML without its tags (for plain-text display of rendered Markdown).
fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_text_is_plain() {
        assert_eq!(Display::Bool { value: true }.text(), "Yes");
        assert_eq!(
            Display::Html {
                html: "<p>Hello <strong>you</strong></p>".into()
            }
            .text(),
            "Hello you"
        );
        assert_eq!(Display::Hidden.text(), "");
    }

    #[test]
    fn badge_tones_are_stable() {
        assert_eq!(tone("won"), tone("won"));
        assert!(tone("lead") < 6);
        assert!(is_image("avatars/a.PNG"));
        assert!(!is_image("logo.svg"));
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
    }
}
