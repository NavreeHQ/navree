//! The Alerts page: the channels with what they get, the throttling and the
//! delivery history, and the form that changes a channel.

use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, ClientIp, Json};
use navree_ui::{
    Badge, BadgeVariant, Button, ButtonVariant, Card, Checkbox, Empty, NativeSelect, Node,
    Pagination, Reply, Row, Stack, TableRowEl, Text, ToastTone,
};
use serde_json::{Value, json};

use super::{
    cell, cell_num, code, delivery_badge, facts, head, head_num, link, post_form, table, titled,
};
use crate::alert::AlertKind;
use crate::api::PageQuery;
use crate::config_api::Actor;
use crate::dashboard::{LighthouseDashboardController, Viewer, age_short, encode};
use crate::dispatch::ChannelInfo;

/// `New issue`, `New issue and Spike`, `New issue, Regression and Spike`.
fn kinds_text(kinds: &[AlertKind]) -> String {
    let labels: Vec<&str> = kinds.iter().map(|kind| kind.label()).collect();
    match labels.as_slice() {
        [] => "nothing".to_owned(),
        [only] => (*only).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// A channel's name as a path segment.
fn segment(name: &str) -> String {
    encode(name).replace('+', "%20")
}

/// The form that changes a channel: its kinds, muting it and a test alert.
/// Each button names what it does; the checkboxes are `kind.<kind>`.
fn channel_form(mount: &str, viewer: &Viewer, info: &ChannelInfo) -> Node {
    let kinds: Vec<Node> = AlertKind::ALL
        .iter()
        .map(|kind| {
            Checkbox::new(format!("kind.{}", kind.as_str()))
                .label(kind.label())
                .checked(info.kinds.contains(kind))
                .into()
        })
        .collect();
    let reset: Option<Button> = (info.kinds != info.configured_kinds).then(|| {
        Button::new("Use the configured kinds")
            .variant(ButtonVariant::Outline)
            .named("action", "reset-kinds")
            .submit()
    });
    let mute: Vec<Node> = if info.muted {
        vec![
            Button::new("Unmute")
                .variant(ButtonVariant::Outline)
                .named("action", "unmute")
                .submit()
                .into(),
        ]
    } else {
        vec![
            NativeSelect::new("minutes")
                .option("60", "for an hour")
                .option("480", "for 8 hours")
                .option("1440", "for a day")
                .option("", "until I unmute it")
                .into(),
            Button::new("Mute")
                .variant(ButtonVariant::Outline)
                .named("action", "mute")
                .submit()
                .into(),
        ]
    };
    post_form(
        format!("{mount}/alerts/channels/{}", segment(&info.name)),
        viewer.csrf.as_deref(),
        Stack::gap(3).children((
            Row::gap(3).wrap().children(kinds),
            Row::gap(2).wrap().children((
                Button::primary("Save kinds")
                    .named("action", "save-kinds")
                    .submit(),
                reset,
            )),
            Row::gap(2).wrap().children((
                mute,
                Button::new("Send a test alert")
                    .variant(ButtonVariant::Outline)
                    .named("action", "test")
                    .submit(),
            )),
        )),
    )
    .into()
}

/// A channel as a card.
fn channel_card(mount: &str, viewer: &Viewer, info: &ChannelInfo) -> Node {
    let state: Badge = if info.muted {
        let until = info
            .state
            .muted_until
            .map(|at| format!(" until {}", at.format("%Y-%m-%d %H:%M UTC")))
            .unwrap_or_default();
        Badge::new(format!("Muted{until}")).variant(BadgeVariant::Warning)
    } else {
        Badge::new("Active").variant(BadgeVariant::Success)
    };
    let mut body: Vec<Node> = vec![Text::new(format!("Gets {}.", kinds_text(&info.kinds))).into()];
    if info.kinds != info.configured_kinds {
        body.push(
            Text::muted(format!(
                "Set here; the configuration says {}.",
                kinds_text(&info.configured_kinds)
            ))
            .into(),
        );
    }
    if viewer.can_configure {
        body.push(channel_form(mount, viewer, info));
    }
    Card::new()
        .title(info.name.clone())
        .description(info.detail.clone())
        .action(state)
        .body(Stack::gap(3).children(body))
        .into()
}

/// The page.
pub(crate) async fn page(
    c: &LighthouseDashboardController,
    viewer: Viewer,
    query: PageQuery,
) -> Result<Response, AppError> {
    let store = c.store()?;
    let tuning = c.lighthouse.tuning();
    let mount = c.mount().to_owned();
    let infos = c
        .lighthouse
        .alerter()
        .map(|alerter| alerter.channel_infos())
        .unwrap_or_default();
    let history = store.alert_history(query.page.unwrap_or(1), 25).await?;
    let counts = store.alert_counts(24).await?;

    let mut last_day = format!("Last 24 hours: {} delivered", counts.delivered);
    if counts.failed > 0 {
        last_day.push_str(&format!(", {} failed", counts.failed));
    }
    if counts.pending > 0 {
        last_day.push_str(&format!(", {} waiting", counts.pending));
    }

    let channels: Node = if infos.is_empty() {
        Empty::new("No alert channel is configured, so nothing is sent.")
            .description(Node::list(vec![
                Node::from("Add one under "),
                code("lighthouse.alerts"),
                Node::from(" in the configuration, for example "),
                code(r#"lighthouse.alerts.email.to = ["oncall@navree.test"]"#),
                Node::from(", or register your own in code. Channels hold secrets, so they are set there and not here."),
            ]))
            .into()
    } else {
        let mut list: Vec<Node> = infos
            .iter()
            .map(|info| channel_card(&mount, &viewer, info))
            .collect();
        if !viewer.can_configure {
            list.push(
                Text::muted(vec![
                    Node::from("Read-only: changing, muting and testing channels needs the "),
                    code("lighthouse.configure"),
                    Node::from(" permission."),
                ])
                .into(),
            );
        }
        list.push(
            Text::muted(
                "Channels are defined in configuration or in code because they hold secrets (a \
                 Slack webhook URL, a signing secret), so they are not created or deleted here. \
                 What each one gets, muting and testing are changed here.",
            )
            .into(),
        );
        Stack::gap(3).children(list).into()
    };

    let throttling = facts(vec![
        (
            "Cooldown",
            Node::from(format!(
                "At most one alert per issue and kind every {} minutes.",
                tuning.cooldown_minutes()
            )),
        ),
        (
            "Spike",
            Node::from(format!(
                "{} occurrences of one issue within {} minutes.",
                tuning.spike_occurrences(),
                tuning.spike_minutes()
            )),
        ),
        (
            "Never alert",
            Node::from("Ignored issues, and issues at level info."),
        ),
    ]);

    let rows: Vec<TableRowEl> = history
        .data
        .iter()
        .map(|row| {
            let kind = AlertKind::parse(&row.kind)
                .map_or_else(|| row.kind.clone(), |kind| kind.label().to_owned());
            let mut what: Vec<Node> = Vec::new();
            what.push(match row.issue_id {
                Some(id) => link(format!("{mount}/issues/{id}"), row.title.clone()),
                None => Node::from(row.title.clone()),
            });
            what.push(Text::muted(row.culprit.clone()).into());
            if let Some(error) = &row.error {
                what.push(Text::muted(error.clone()).into());
            }
            TableRowEl::new(vec![
                cell(kind).hide_on_mobile(),
                cell(Node::list(what)),
                cell(row.channel.clone()),
                cell(delivery_badge(&row.status)),
                cell_num(row.attempts.to_string()).hide_on_mobile(),
                cell_num(format!("{} ago", age_short(row.created_at))).hide_on_mobile(),
            ])
            .key(row.id.to_string())
        })
        .collect();
    let pager: Option<Node> = (history.meta.last_page > 1).then(|| {
        Pagination::new(
            u32::try_from(history.meta.current_page).unwrap_or(1),
            u32::try_from(history.meta.last_page).unwrap_or(1),
            |n| format!("{mount}/alerts?page={n}"),
        )
        .into()
    });

    let page = c.page(&viewer, "Alerts").await?.body((
        Text::muted(format!(
            "Where Lighthouse sends an alert when something needs a person, and what it has sent. {last_day}."
        )),
        titled("Channels", channels),
        titled(
            "Throttling",
            Stack::gap(2).children((
                throttling,
                Text::muted(vec![
                    Node::from("Change these under "),
                    link(format!("{mount}/settings"), "Settings"),
                    Node::from("."),
                ]),
            )),
        ),
        titled(
            "Delivery history",
            table(
                vec![
                    head("Alert").hide_on_mobile(),
                    head("Issue"),
                    head("Channel"),
                    head("Status"),
                    head_num("Attempts").hide_on_mobile(),
                    head_num("Raised").hide_on_mobile(),
                ],
                rows,
                "No alerts yet. They appear here when an issue opens, comes back or spikes.",
            ),
        ),
        pager,
    ));
    Ok(page.into_response())
}

/// Changes a channel: the form's button says what to do.
pub(crate) async fn change(
    c: &LighthouseDashboardController,
    viewer: &Viewer,
    ip: ClientIp,
    name: &str,
    form: Vec<(String, String)>,
) -> Result<Reply, AppError> {
    let actor = Actor::new(viewer.user_id, ip, &viewer.headers);
    let value_of = |key: &str| {
        form.iter()
            .find(|(field, _)| field == key)
            .map(|(_, value)| value.as_str())
    };
    let back = Reply::redirect(format!("{}/alerts", c.mount()));
    match value_of("action").unwrap_or_default() {
        "save-kinds" => {
            let kinds: Vec<Value> = AlertKind::ALL
                .iter()
                .filter(|kind| {
                    let field = format!("kind.{}", kind.as_str());
                    form.iter().any(|(key, _)| *key == field)
                })
                .map(|kind| json!(kind.as_str()))
                .collect();
            if kinds.is_empty() {
                return Ok(back.toast(
                    ToastTone::Danger,
                    "Pick at least one kind, or mute the channel.",
                ));
            }
            c.configure
                .change_channel(actor, name, json!({ "on": kinds }))
                .await?;
            Ok(back.toast(ToastTone::Success, format!("Saved what {name} gets.")))
        }
        "reset-kinds" => {
            c.configure
                .change_channel(actor, name, json!({ "on": Value::Null }))
                .await?;
            Ok(back.toast(
                ToastTone::Success,
                format!("{name} gets the configured kinds."),
            ))
        }
        "mute" => {
            let body = match value_of("minutes").filter(|minutes| !minutes.is_empty()) {
                Some(minutes) => {
                    let minutes: u64 = minutes
                        .parse()
                        .map_err(|_| AppError::bad_request("minutes must be a number"))?;
                    json!({ "muted": true, "minutes": minutes })
                }
                None => json!({ "muted": true }),
            };
            c.configure.change_channel(actor, name, body).await?;
            Ok(back.toast(ToastTone::Success, format!("Muted {name}.")))
        }
        "unmute" => {
            c.configure
                .change_channel(actor, name, json!({ "muted": false }))
                .await?;
            Ok(back.toast(ToastTone::Success, format!("Unmuted {name}.")))
        }
        "test" => {
            let Json(result) = c.configure.test_channel(actor, name).await?;
            if result["ok"].as_bool().unwrap_or(false) {
                Ok(back.toast(ToastTone::Success, format!("Delivered a test alert to {name}.")))
            } else {
                let error = result["error"].as_str().unwrap_or("no answer");
                Ok(back.toast(ToastTone::Danger, format!("The test alert failed: {error}")))
            }
        }
        _ => Err(AppError::bad_request("choose what to do with the channel")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_read_as_a_sentence() {
        assert_eq!(kinds_text(&[]), "nothing");
        assert_eq!(kinds_text(&[AlertKind::Spike]), "Spike");
        assert_eq!(
            kinds_text(&[AlertKind::NewIssue, AlertKind::Spike]),
            "New issue and Spike"
        );
        assert_eq!(
            kinds_text(&[AlertKind::NewIssue, AlertKind::Regression, AlertKind::Spike]),
            "New issue, Regression and Spike"
        );
    }

    #[test]
    fn a_channel_name_is_a_safe_path_segment() {
        assert_eq!(segment("slack"), "slack");
        assert_eq!(segment("on call/team"), "on%20call%2Fteam");
    }
}
