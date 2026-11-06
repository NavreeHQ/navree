//! The dashboard's pages, drawn with Navree UI: what each reads from the
//! store and how it is laid out. The shell, the routes and the sign-in are
//! in `dashboard.rs`.

use navree_ui::form::csrf_field;
use navree_ui::{
    Badge, BadgeVariant, Button, ButtonGroup, ButtonVariant, Card, Element, Empty, IntoNodes, Node,
    Size, Table, TableCell, TableHead, TableRowEl, Text, TextVariant, Tone, Uses, el,
};

use crate::model::{IssueKind, IssueStatus, Level};
use crate::options::Period;

pub(crate) mod alerts;
pub(crate) mod issues;
pub(crate) mod lists;
pub(crate) mod overview;
pub(crate) mod releases;
pub(crate) mod settings;
pub(crate) mod traces;

pub(crate) use releases::ReleaseQuery;

/// A duration for people: `0.42 ms`, `4.1 ms`, `64 ms`, `1.24 s`.
pub(crate) fn ms(value: f64) -> String {
    if value >= 1000.0 {
        format!("{:.2} s", value / 1000.0)
    } else if value >= 10.0 {
        format!("{value:.0} ms")
    } else if value >= 1.0 {
        format!("{value:.1} ms")
    } else {
        format!("{value:.2} ms")
    }
}

/// A share from 0 to 1 as a percentage with one decimal.
pub(crate) fn percent(rate: f64) -> String {
    format!("{:.1}%", rate * 100.0)
}

/// A time as `2026-11-02 10:15`, in UTC.
pub(crate) fn when(at: chrono::DateTime<chrono::Utc>) -> String {
    at.format("%Y-%m-%d %H:%M").to_string()
}

/// A time as `2026-11-02 10:15:30`, in UTC.
pub(crate) fn when_exact(at: chrono::DateTime<chrono::Utc>) -> String {
    at.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Text in the monospace face, inline.
pub(crate) fn code(text: impl Into<String>) -> Node {
    Text::new(text.into())
        .variant(TextVariant::Code)
        .inline()
        .into()
}

/// A link to a page of this site.
pub(crate) fn link(href: impl Into<String>, text: impl Into<Node>) -> Node {
    el("a").href(href.into()).child(text.into()).into()
}

/// The tone of an issue's level.
pub(crate) fn level_tone(level: Level) -> Tone {
    match level {
        Level::Error => Tone::Danger,
        Level::Warning => Tone::Warning,
        Level::Info => Tone::Info,
    }
}

/// What an issue is, coloured by how bad it is.
pub(crate) fn kind_badge(kind: IssueKind, level: Level) -> Badge {
    Badge::new(kind.label()).tone(level_tone(level)).dot()
}

/// Where an issue stands.
pub(crate) fn status_badge(status: IssueStatus) -> Badge {
    let variant = match status {
        IssueStatus::Open => BadgeVariant::Danger,
        IssueStatus::Resolved | IssueStatus::ResolvedInNextRelease => BadgeVariant::Success,
        IssueStatus::Ignored => BadgeVariant::Outline,
    };
    Badge::new(status.label()).variant(variant)
}

/// The badge of an alert delivery's status (`pending`, `delivered`,
/// `failed`).
pub(crate) fn delivery_badge(status: &str) -> Badge {
    let variant = match status {
        "delivered" => BadgeVariant::Success,
        "failed" => BadgeVariant::Danger,
        _ => BadgeVariant::Outline,
    };
    Badge::new(status.to_owned()).variant(variant)
}

/// A heading cell.
pub(crate) fn head(label: &'static str) -> TableHead {
    TableHead::new(label)
}

/// A heading cell over numbers.
pub(crate) fn head_num(label: &'static str) -> TableHead {
    TableHead::new(label).numeric()
}

/// A cell holding `content`.
pub(crate) fn cell(content: impl Into<Node>) -> TableCell {
    TableCell::new(content.into())
}

/// A cell holding a number or a figure.
pub(crate) fn cell_num(content: impl Into<Node>) -> TableCell {
    TableCell::new(content.into()).numeric()
}

/// A table, or `empty` when there are no rows.
pub(crate) fn table(head: Vec<TableHead>, rows: Vec<TableRowEl>, empty: &'static str) -> Node {
    if rows.is_empty() {
        return Empty::new(empty).into();
    }
    Table::new().head(head).rows(rows).into()
}

/// A card with a title around `body`.
pub(crate) fn titled(title: &'static str, body: impl Into<Node>) -> Node {
    Card::titled(title).body(body.into()).into()
}

/// The three periods as a row of buttons; `base` ends in `?` or `&`.
pub(crate) fn period_switch(base: &str, current: Period) -> Node {
    let buttons: Vec<Button> = Period::ALL
        .into_iter()
        .map(|period| {
            let button = Button::new(period.as_str())
                .size(Size::Sm)
                .href(format!("{base}period={}", period.as_str()));
            if period == current {
                button
                    .variant(ButtonVariant::Primary)
                    .attr("aria-current", "true")
            } else {
                button.variant(ButtonVariant::Outline)
            }
        })
        .collect();
    ButtonGroup::new("Period").children(buttons).into()
}

/// `name` and `value` rows of a two-column table: the facts of a record.
pub(crate) fn facts(rows: Vec<(&'static str, Node)>) -> Node {
    let rows: Vec<TableRowEl> = rows
        .into_iter()
        .map(|(label, value)| TableRowEl::new(vec![cell(label), cell(value)]))
        .collect();
    Table::new()
        .head(vec![head("Field"), head("Value")])
        .rows(rows)
        .into()
}

/// A form that posts to `action`. It carries the session's CSRF token as a
/// field, so it works with JavaScript off, and the runtime sends it without
/// leaving the page when it can.
pub(crate) fn post_form(
    action: String,
    csrf: Option<&str>,
    children: impl IntoNodes,
) -> Element {
    el("form")
        .attr("method", "post")
        .attr("action", action)
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .child(csrf.map(|token| csrf_field(token.to_owned())))
        .child(children)
}

/// A comment, trimmed; `None` when blank.
pub(crate) fn clean(comment: String) -> Option<String> {
    let comment = comment.trim();
    (!comment.is_empty()).then(|| comment.to_owned())
}

/// A value of a JSON row as text, or empty.
pub(crate) fn text_of(row: &serde_json::Value, key: &str) -> String {
    match row.get(key) {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Number(number)) => number.to_string(),
        Some(serde_json::Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_keep_the_precision_that_matters() {
        assert_eq!(ms(0.42), "0.42 ms");
        assert_eq!(ms(4.1), "4.1 ms");
        assert_eq!(ms(64.4), "64 ms");
        assert_eq!(ms(1240.0), "1.24 s");
    }

    #[test]
    fn rates_are_percentages() {
        assert_eq!(percent(0.0), "0.0%");
        assert_eq!(percent(0.0345), "3.5%");
        assert_eq!(percent(1.0), "100.0%");
    }

    #[test]
    fn a_period_switch_marks_the_current_period() {
        let html = navree_ui::render(&period_switch("/_lighthouse/jobs?", Period::Day));
        assert!(html.contains(r#"href="/_lighthouse/jobs?period=1h""#), "{html}");
        assert!(html.contains(r#"href="/_lighthouse/jobs?period=7d""#), "{html}");
        assert_eq!(html.matches("aria-current").count(), 1, "{html}");
    }

    #[test]
    fn a_form_carries_the_sessions_token_and_works_without_script() {
        let form = post_form("/_lighthouse/x".to_owned(), Some("tok\"en"), "Go");
        let html = navree_ui::render(&Node::from(form));
        assert!(html.contains(r#"method="post""#), "{html}");
        assert!(html.contains(r#"name="_csrf""#), "{html}");
        assert!(html.contains("tok&quot;en"), "{html}");
        let bare = navree_ui::render(&Node::from(post_form("/x".to_owned(), None, "Go")));
        assert!(!bare.contains("_csrf"), "{bare}");
    }

    #[test]
    fn comments_are_trimmed_and_blank_ones_dropped() {
        assert_eq!(clean("  fixed in 4.2 ".to_owned()).as_deref(), Some("fixed in 4.2"));
        assert_eq!(clean("   ".to_owned()), None);
    }

    #[test]
    fn a_table_without_rows_says_so() {
        let html = navree_ui::render(&table(vec![head("Route")], Vec::new(), "Nothing yet."));
        assert!(html.contains("Nothing yet."), "{html}");
        assert!(!html.contains("<table"), "{html}");
    }
}
