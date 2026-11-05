//! What the dashboard's Alerts, Releases and Settings pages read and write:
//! the releases seen, the settings changed from the dashboard and the
//! history of alert deliveries. Plain functions on [`LighthouseStore`]; the
//! pages and the JSON API only format what they return.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, TimeDelta, Utc};
use navree_db::{Executor as _, Statement};
use navree_orm::{ConflictUpdate, Model as _, Record};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::entity::{alert, issue, release, setting};
use crate::model::{IssueKind, IssueStatus, Level};
use crate::release::{ReleaseMarker, ReleaseReport, ReleaseSummary};
use crate::settings::{self, Tuning};
use crate::store::{LighthouseError, LighthouseStore, ListPage, PageMeta};

/// The release label a process has when nothing sets one.
const UNKNOWN_RELEASE: &str = "unknown";

/// A setting changed from the dashboard.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StoredSetting {
    /// The value.
    pub value: Value,
    /// Who changed it (a user id).
    pub updated_by: Option<String>,
    /// When.
    pub updated_at: DateTime<Utc>,
}

/// One alert delivery as the Alerts page lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AlertHistoryRow {
    /// The id.
    pub id: Uuid,
    /// The issue, when the alert is about one.
    pub issue_id: Option<Uuid>,
    /// The issue's title when the alert was raised.
    pub title: String,
    /// The route, job or host it was about.
    pub culprit: String,
    /// The kind (`new_issue`).
    pub kind: String,
    /// The channel.
    pub channel: String,
    /// `pending`, `delivered` or `failed`.
    pub status: String,
    /// Delivery attempts.
    pub attempts: i32,
    /// The last delivery error.
    pub error: Option<String>,
    /// When it was raised.
    pub created_at: DateTime<Utc>,
    /// When it was delivered.
    pub delivered_at: Option<DateTime<Utc>>,
}

/// How many alerts were delivered, are waiting and failed in a window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct AlertCounts {
    /// Delivered.
    pub delivered: u64,
    /// Waiting for a delivery or a retry.
    pub pending: u64,
    /// Given up on.
    pub failed: u64,
}

/// A stored release with its counts.
fn summary_of(
    row: release::Model,
    introduced: &HashMap<String, i64>,
    regressed: &HashMap<String, i64>,
    current: bool,
) -> ReleaseSummary {
    ReleaseSummary {
        introduced: introduced.get(&row.name).copied().unwrap_or(0),
        regressed: regressed.get(&row.name).copied().unwrap_or(0),
        current,
        name: row.name,
        environment: row.environment,
        commit: row.commit_sha,
        url: row.url,
        notes: row.notes,
        source: row.source,
        deployed_at: row.deployed_at,
        first_seen: row.first_seen,
        last_seen: row.last_seen,
    }
}

impl LighthouseStore {
    // -----------------------------------------------------------------
    // Releases
    // -----------------------------------------------------------------

    /// Notes that a process runs `name` in `environment`: adds the release
    /// (deployed now, as far as is known) or marks it seen again. A process
    /// on a release other than the environment's newest deploys it again:
    /// that is a rollback, or a release coming back. The label `unknown`
    /// (no release set) is not a release.
    pub(crate) async fn note_release(
        &self,
        name: &str,
        environment: &str,
    ) -> Result<(), LighthouseError> {
        if name.trim().is_empty() || name == UNKNOWN_RELEASE {
            return Ok(());
        }
        let now = Utc::now();
        self.insert_release_if_missing(name, environment, "process", now)
            .await?;
        let mut sets = vec![
            release::Model::LAST_SEEN.set(now),
            release::Model::UPDATED_AT.set(now),
        ];
        if self
            .newest_release(environment)
            .await?
            .is_some_and(|newest| newest.name != name)
        {
            sets.push(release::Model::DEPLOYED_AT.set(now));
        }
        release::Model::query()
            .where_(release::Model::NAME.eq(name))
            .where_(release::Model::ENVIRONMENT.eq(environment))
            .update(&self.db, sets)
            .await?;
        Ok(())
    }

    async fn insert_release_if_missing(
        &self,
        name: &str,
        environment: &str,
        source: &str,
        deployed_at: DateTime<Utc>,
    ) -> Result<(), LighthouseError> {
        let now = Utc::now();
        release::Model::query()
            .upsert(
                &self.db,
                [release::Model {
                    id: Uuid::now_v7(),
                    name: name.to_owned(),
                    environment: environment.to_owned(),
                    commit_sha: None,
                    url: None,
                    notes: None,
                    source: source.to_owned(),
                    deployed_at,
                    first_seen: now,
                    last_seen: now,
                    created_at: now,
                    updated_at: now,
                }],
                [release::Model::ENVIRONMENT, release::Model::NAME],
                Vec::<ConflictUpdate>::new(),
            )
            .await?;
        Ok(())
    }

    /// The newest deploy in `environment`.
    async fn newest_release(
        &self,
        environment: &str,
    ) -> Result<Option<release::Model>, LighthouseError> {
        Ok(release::Model::query()
            .where_(release::Model::ENVIRONMENT.eq(environment))
            .order_by_desc(release::Model::DEPLOYED_AT)
            .use_write()
            .first(&self.db)
            .await?
            .map(Record::into_inner))
    }

    /// Adds a release, or updates what is known of it: what the report
    /// gives replaces what was there. A release first seen by a process
    /// takes the reported deploy time.
    pub async fn report_release(
        &self,
        report: ReleaseReport,
        default_environment: &str,
        source: &str,
    ) -> Result<ReleaseSummary, LighthouseError> {
        let report = report.checked().map_err(LighthouseError::Other)?;
        let environment = report
            .environment
            .clone()
            .unwrap_or_else(|| default_environment.to_owned());
        let now = Utc::now();
        let deployed_at = report.deployed_at.map(|at| at.min(now));
        self.insert_release_if_missing(
            &report.release,
            &environment,
            source,
            deployed_at.unwrap_or(now),
        )
        .await?;
        let mut sets = vec![
            release::Model::SOURCE.set(source),
            release::Model::UPDATED_AT.set(now),
        ];
        if let Some(at) = deployed_at {
            sets.push(release::Model::DEPLOYED_AT.set(at));
        }
        if let Some(commit) = report.commit {
            sets.push(release::Model::COMMIT_SHA.set(Some(commit)));
        }
        if let Some(url) = report.url {
            sets.push(release::Model::URL.set(Some(url)));
        }
        if let Some(notes) = report.notes {
            sets.push(release::Model::NOTES.set(Some(notes)));
        }
        release::Model::query()
            .where_(release::Model::NAME.eq(report.release.as_str()))
            .where_(release::Model::ENVIRONMENT.eq(environment.as_str()))
            .update(&self.db, sets)
            .await?;
        let row = release::Model::query()
            .where_(release::Model::NAME.eq(report.release.as_str()))
            .where_(release::Model::ENVIRONMENT.eq(environment.as_str()))
            .use_write()
            .first(&self.db)
            .await?
            .map(Record::into_inner)
            .ok_or(LighthouseError::NotFound("release"))?;
        let names = std::slice::from_ref(&row.name);
        let introduced = self.issue_counts_by("first_release", names).await?;
        let regressed = self.issue_counts_by("regressed_in_release", names).await?;
        let current = environment == self.options.environment_label()
            && self
                .newest_release(&environment)
                .await?
                .is_some_and(|newest| newest.id == row.id);
        Ok(summary_of(row, &introduced, &regressed, current))
    }

    /// The releases seen, newest deploy first, with the issues each
    /// introduced and the ones that came back in it.
    pub async fn releases(&self, limit: u64) -> Result<Vec<ReleaseSummary>, LighthouseError> {
        let rows = release::Model::query()
            .order_by_desc(release::Model::DEPLOYED_AT)
            .limit(limit.clamp(1, 500))
            .use_write()
            .get(&self.db)
            .await?
            .into_models();
        let names: Vec<String> = rows.iter().map(|row| row.name.clone()).collect();
        let introduced = self.issue_counts_by("first_release", &names).await?;
        let regressed = self.issue_counts_by("regressed_in_release", &names).await?;
        let environment = self.options.environment_label();
        let current = rows
            .iter()
            .find(|row| row.environment == environment)
            .map(|row| row.id);
        Ok(rows
            .into_iter()
            .map(|row| {
                let is_current = Some(row.id) == current;
                summary_of(row, &introduced, &regressed, is_current)
            })
            .collect())
    }

    /// How many issues each of `names` has in `column` (`first_release` or
    /// `regressed_in_release`): counted by the database, on the column's
    /// index.
    async fn issue_counts_by(
        &self,
        column: &'static str,
        names: &[String],
    ) -> Result<HashMap<String, i64>, LighthouseError> {
        let mut counts: HashMap<String, i64> = HashMap::new();
        for chunk in names.chunks(200) {
            let places = vec!["?"; chunk.len()].join(", ");
            let statement = Statement::portable(
                format!(
                    "SELECT {column} AS name, COUNT(*) AS issues FROM lighthouse_issues \
                     WHERE {column} IN ({places}) GROUP BY {column}"
                ),
                chunk.iter().map(|name| name.as_str().into()),
            );
            // `column` is one of two column names this module passes as literals, and
            // `places` holds only `?` placeholders: every release name is bound.
            // nosemgrep: navree-sql-built-from-strings
            for row in self.db.fetch_all(&statement).await? {
                let name: String = row.get("name")?;
                *counts.entry(name).or_default() += row.get::<i64>("issues")?;
            }
        }
        Ok(counts)
    }

    /// The release the app is at, and when it was deployed: the newest deploy
    /// in its environment. Every page's header reads it.
    pub(crate) async fn current_release_label(
        &self,
    ) -> Result<Option<(String, DateTime<Utc>)>, LighthouseError> {
        let environment = self.options.environment_label();
        Ok(self
            .newest_release(&environment)
            .await?
            .map(|row| (row.name, row.deployed_at)))
    }

    /// The release the app is at, with its counts: the newest deploy in its
    /// environment.
    pub async fn current_release(&self) -> Result<Option<ReleaseSummary>, LighthouseError> {
        let environment = self.options.environment_label();
        let Some(row) = self.newest_release(&environment).await? else {
            return Ok(None);
        };
        let names = std::slice::from_ref(&row.name);
        let introduced = self.issue_counts_by("first_release", names).await?;
        let regressed = self.issue_counts_by("regressed_in_release", names).await?;
        Ok(Some(summary_of(row, &introduced, &regressed, true)))
    }

    /// The deploys since `since` in this environment, oldest first: the
    /// marks on a chart.
    pub async fn release_markers(
        &self,
        since: DateTime<Utc>,
    ) -> Result<Vec<ReleaseMarker>, LighthouseError> {
        let environment = self.options.environment_label();
        Ok(release::Model::query()
            .where_(release::Model::ENVIRONMENT.eq(environment.as_str()))
            .where_(release::Model::DEPLOYED_AT.gte(since))
            .order_by(release::Model::DEPLOYED_AT)
            .limit(20)
            .get(&self.db)
            .await?
            .into_models()
            .into_iter()
            .map(|row| ReleaseMarker {
                name: row.name,
                at: row.deployed_at,
            })
            .collect())
    }

    /// Issues that began in `name` (`introduced`), or that came back in it,
    /// most frequent first.
    pub async fn release_issues(
        &self,
        name: &str,
        introduced: bool,
        limit: u64,
    ) -> Result<Vec<(Uuid, String, IssueKind, Level, IssueStatus, i64)>, LighthouseError> {
        let column = if introduced {
            issue::Model::FIRST_RELEASE
        } else {
            issue::Model::REGRESSED_IN_RELEASE
        };
        Ok(issue::Model::query()
            .where_(column.eq(name))
            .order_by_desc(issue::Model::COUNT)
            .limit(limit.clamp(1, 100))
            .get(&self.db)
            .await?
            .into_models()
            .into_iter()
            .map(|row| {
                (
                    row.id,
                    row.title,
                    IssueKind::parse(&row.kind).unwrap_or(IssueKind::Exception),
                    Level::parse(&row.level).unwrap_or_default(),
                    IssueStatus::parse(&row.status).unwrap_or(IssueStatus::Open),
                    row.count,
                )
            })
            .collect())
    }

    // -----------------------------------------------------------------
    // Settings
    // -----------------------------------------------------------------

    /// The settings changed from the dashboard, by key.
    pub async fn stored_settings(
        &self,
    ) -> Result<BTreeMap<String, StoredSetting>, LighthouseError> {
        Ok(setting::Model::query()
            .use_write()
            .get(&self.db)
            .await?
            .into_models()
            .into_iter()
            .filter_map(|row| {
                Some((
                    row.setting_key,
                    StoredSetting {
                        value: serde_json::from_str(&row.value).ok()?,
                        updated_by: row.updated_by,
                        updated_at: row.updated_at,
                    },
                ))
            })
            .collect())
    }

    /// Reads the stored settings and makes them the ones `tuning` uses.
    pub(crate) async fn reload_settings(&self, tuning: &Tuning) -> Result<(), LighthouseError> {
        let stored = self.stored_settings().await?;
        let values: BTreeMap<String, Value> = stored
            .into_iter()
            .map(|(key, stored)| (key, stored.value))
            .collect();
        tuning.apply(&values);
        Ok(())
    }

    /// Stores changes to settings: a value sets it, `None` returns it to
    /// what the configuration says. The caller has checked every value
    /// ([`settings::check`]). Applies them to this process at once.
    pub(crate) async fn save_settings(
        &self,
        changes: BTreeMap<String, Option<Value>>,
        by: Option<String>,
    ) -> Result<(), LighthouseError> {
        let now = Utc::now();
        for (key, value) in changes {
            match value {
                Some(value) => {
                    setting::Model::query()
                        .upsert(
                            &self.db,
                            [setting::Model {
                                id: Uuid::now_v7(),
                                setting_key: key.clone(),
                                value: value.to_string(),
                                updated_by: by.clone(),
                                updated_at: now,
                            }],
                            [setting::Model::SETTING_KEY],
                            [
                                ConflictUpdate::from(setting::Model::VALUE),
                                setting::Model::UPDATED_BY.into(),
                                setting::Model::UPDATED_AT.into(),
                            ],
                        )
                        .await?;
                }
                None => {
                    setting::Model::query()
                        .where_(setting::Model::SETTING_KEY.eq(key.as_str()))
                        .delete(&self.db)
                        .await?;
                }
            }
        }
        self.reload_settings(&self.tuning).await
    }

    // -----------------------------------------------------------------
    // Alert history
    // -----------------------------------------------------------------

    /// A page of alert deliveries, newest first, with what each was about.
    pub async fn alert_history(
        &self,
        page: u64,
        per_page: u64,
    ) -> Result<ListPage<AlertHistoryRow>, LighthouseError> {
        let per_page = per_page.clamp(1, 100);
        let page = page.max(1);
        let paged = alert::Model::query()
            .order_by_desc(alert::Model::CREATED_AT)
            .paginate(&self.db, (page, per_page))
            .await?;
        let total = paged.meta.total;
        let data = paged
            .data
            .into_iter()
            .map(Record::into_inner)
            .map(|row| {
                let payload: Value = serde_json::from_str(&row.payload).unwrap_or(Value::Null);
                let text = |pointer: &str| {
                    payload
                        .pointer(pointer)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                AlertHistoryRow {
                    id: row.id,
                    issue_id: row.issue_id,
                    title: text("/issue/title"),
                    culprit: text("/issue/culprit"),
                    kind: row.kind,
                    channel: row.channel,
                    status: row.status,
                    attempts: row.attempts,
                    error: row.error,
                    created_at: row.created_at,
                    delivered_at: row.delivered_at,
                }
            })
            .collect();
        Ok(ListPage {
            data,
            meta: PageMeta {
                current_page: page,
                per_page,
                total,
                last_page: total.div_ceil(per_page).max(1),
            },
        })
    }

    /// Alerts raised in the last `hours`, by how their delivery stands.
    pub async fn alert_counts(&self, hours: i64) -> Result<AlertCounts, LighthouseError> {
        let since = Utc::now() - TimeDelta::hours(hours);
        let count = |status: &'static str| async move {
            alert::Model::query()
                .where_(alert::Model::CREATED_AT.gte(since))
                .where_(alert::Model::STATUS.eq(status))
                .count(&self.db)
                .await
        };
        Ok(AlertCounts {
            delivered: count("delivered").await?,
            pending: count("pending").await?,
            failed: count("failed").await?,
        })
    }
}

/// The settings page's rows: every setting that can change, with the value
/// in use, the configured one and who changed it.
pub(crate) fn setting_rows(
    tuning: &Tuning,
    stored: &BTreeMap<String, StoredSetting>,
) -> Vec<Value> {
    settings::SETTINGS
        .iter()
        .map(|def| {
            let changed = stored.get(def.key);
            serde_json::json!({
                "key": def.key,
                "group": def.group,
                "label": def.label,
                "help": def.help,
                "unit": def.unit,
                "kind": def.kind,
                "min": def.min,
                "max": def.max,
                "current": tuning.current(def.key),
                "configured": tuning.configured(def.key),
                "changed": changed.is_some(),
                "updated_by": changed.and_then(|c| c.updated_by.clone()),
                "updated_at": changed.map(|c| c.updated_at),
            })
        })
        .collect()
}
