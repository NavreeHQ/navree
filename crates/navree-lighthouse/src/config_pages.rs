//! The dashboard's Alerts, Releases and Settings pages: what they read from
//! the store, shaped for their templates.

use navree_http::AppError;
use navree_http::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::alert::AlertKind;
use crate::api::PageQuery;
use crate::config_api::channel_json;
use crate::config_store::setting_rows;
use crate::dashboard::{LighthouseDashboardController, Viewer, age_short, ago};
use crate::model::{IssueKind, IssueStatus, Level};

/// `?release=` on the Releases page: the release to look into.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ReleaseQuery {
    release: Option<String>,
}

/// An issue of a release, for its template.
fn issue_row(
    mount: &str,
    (id, title, kind, level, status, count): (
        uuid::Uuid,
        String,
        IssueKind,
        Level,
        IssueStatus,
        i64,
    ),
) -> Value {
    json!({
        "id": id,
        "title": title,
        "kind": kind.as_str(),
        "kind_label": kind.label(),
        "level": level.as_str(),
        "status": status.as_str(),
        "status_label": status.label(),
        "count": count,
        "url": format!("{mount}/issues/{id}"),
    })
}

impl LighthouseDashboardController {
    /// The Alerts page: the channels with what they get, the throttling,
    /// and the delivery history.
    pub(crate) async fn alerts(
        &self,
        viewer: Viewer,
        query: PageQuery,
    ) -> Result<Response, AppError> {
        let store = self.store()?;
        let tuning = self.lighthouse.tuning();
        let channels: Vec<Value> = self
            .lighthouse
            .alerter()
            .map(|alerter| alerter.channel_infos())
            .unwrap_or_default()
            .iter()
            .map(|info| {
                let mut row = channel_json(info);
                row["kind_choices"] = json!(
                    AlertKind::ALL
                        .iter()
                        .map(|kind| json!({
                            "value": kind.as_str(),
                            "label": kind.label(),
                            "on": info.kinds.contains(kind),
                        }))
                        .collect::<Vec<_>>()
                );
                row["kind_labels"] =
                    json!(info.kinds.iter().map(|k| k.label()).collect::<Vec<_>>());
                row["kinds_changed"] = json!(info.kinds != info.configured_kinds);
                row["configured_labels"] = json!(
                    info.configured_kinds
                        .iter()
                        .map(|k| k.label())
                        .collect::<Vec<_>>()
                );
                row["until"] = json!(
                    info.state
                        .muted_until
                        .filter(|_| info.muted)
                        .map(|at| at.format("%Y-%m-%d %H:%M UTC").to_string())
                );
                row
            })
            .collect();
        let history = store.alert_history(query.page.unwrap_or(1), 25).await?;
        let rows: Vec<Value> = history
            .data
            .iter()
            .map(|row| {
                let mut value = serde_json::to_value(row).unwrap_or(Value::Null);
                value["kind_label"] = match AlertKind::parse(&row.kind) {
                    Some(kind) => json!(kind.label()),
                    None => json!(row.kind),
                };
                value["age"] = json!(age_short(row.created_at));
                value
            })
            .collect();
        let counts = store.alert_counts(24).await?;
        let data = json!({
            "channels": channels,
            "history": rows,
            "meta": history.meta,
            "counts": counts,
            "cooldown": tuning.cooldown_minutes(),
            "spike_occurrences": tuning.spike_occurrences(),
            "spike_minutes": tuning.spike_minutes(),
            "can_configure": viewer.can_configure,
            "mute_choices": [
                { "value": "60", "label": "for an hour" },
                { "value": "480", "label": "for 8 hours" },
                { "value": "1440", "label": "for a day" },
                { "value": "", "label": "until I unmute it" },
            ],
        });
        self.render(
            &viewer,
            "lighthouse/alerts.html",
            "alerts",
            "Alerts",
            None,
            data,
        )
        .await
    }

    /// The Releases page: the releases seen, and for one of them the issues
    /// it introduced and the ones that came back in it.
    pub(crate) async fn releases(
        &self,
        viewer: Viewer,
        query: ReleaseQuery,
    ) -> Result<Response, AppError> {
        let store = self.store()?;
        let releases = store.releases(50).await?;
        let selected = query
            .release
            .as_deref()
            .and_then(|name| releases.iter().find(|r| r.name == name))
            .or_else(|| releases.iter().find(|r| r.current))
            .or_else(|| releases.first())
            .cloned();
        let mount = self.mount().to_owned();
        let (introduced, regressed) = match &selected {
            Some(release) => (
                store
                    .release_issues(&release.name, true, 20)
                    .await?
                    .into_iter()
                    .map(|row| issue_row(&mount, row))
                    .collect::<Vec<_>>(),
                store
                    .release_issues(&release.name, false, 20)
                    .await?
                    .into_iter()
                    .map(|row| issue_row(&mount, row))
                    .collect::<Vec<_>>(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        let rows: Vec<Value> = releases
            .iter()
            .map(|release| {
                let mut row = serde_json::to_value(release).unwrap_or(Value::Null);
                row["deployed_ago"] = json!(ago(release.deployed_at));
                row["selected"] = json!(selected.as_ref().is_some_and(
                    |s| s.name == release.name && s.environment == release.environment
                ));
                row["link"] = json!(
                    form_urlencoded::byte_serialize(release.name.as_bytes()).collect::<String>()
                );
                row
            })
            .collect();
        let data = json!({
            "releases": rows,
            "selected": selected,
            "introduced": introduced,
            "regressed": regressed,
            "report_url": format!("{}/api/releases", store.base_url()),
            "environment": self.lighthouse.options().environment_label(),
        });
        self.render(
            &viewer,
            "lighthouse/releases.html",
            "releases",
            "Releases",
            None,
            data,
        )
        .await
    }

    /// The Settings page: what can change (with the value in use and the
    /// configured one), and what is set in code or configuration.
    pub(crate) async fn settings(&self, viewer: Viewer) -> Result<Response, AppError> {
        let stored = self.store()?.stored_settings().await?;
        let rows = setting_rows(self.lighthouse.tuning(), &stored);
        let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
        for row in rows {
            let group = row["group"].as_str().unwrap_or_default().to_owned();
            match groups.iter_mut().find(|(name, _)| *name == group) {
                Some((_, items)) => items.push(row),
                None => groups.push((group, vec![row])),
            }
        }
        let data = json!({
            "groups": groups
                .into_iter()
                .map(|(group, items)| json!({ "group": group, "items": items }))
                .collect::<Vec<_>>(),
            "read_only": crate::settings_view::read_only(self.lighthouse.options()),
            "can_configure": viewer.can_configure,
        });
        self.render(
            &viewer,
            "lighthouse/settings.html",
            "settings",
            "Settings",
            None,
            data,
        )
        .await
    }
}
