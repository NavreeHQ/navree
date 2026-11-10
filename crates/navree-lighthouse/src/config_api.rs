//! The JSON API of the Alerts, Releases and Settings pages, under
//! `{lighthouse.path}/api`: reading is the `view` ability, every change the
//! `configure` ability. A change through a browser's session carries the
//! session's CSRF token (checked by navree-session); a change through an
//! access token needs none. Every change to settings and channels is
//! audited when the app has an audit log. The dashboard's forms make the
//! same changes through `Configure`.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::{TimeDelta, Utc};
use http::{HeaderMap, Method};
use navree_access::Subject as _;
use navree_contracts::{AuditEntry, AuditKind, AuditSink};
use navree_http::{
    ApiMeta, AppError, ClientIp, Controller, CurrentPrincipal, Json, Path, Query, RouteDef,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::alert::AlertKind;
use crate::api::PageQuery;
use crate::config_store::setting_rows;
use crate::dispatch::ChannelInfo;
use crate::module::{CONFIGURE, LighthouseSubject, VIEW};
use crate::options::StoreKind;
use crate::release::ReleaseReport;
use crate::service::Lighthouse;
use crate::settings::{self, CHANNEL_PREFIX, ChannelState};
use crate::store::LighthouseStore;

/// The OpenAPI tag of the routes.
const TAG: &str = "Lighthouse";

/// The routes behind the Alerts, Releases and Settings pages:
///
/// | Route | Answer |
/// |---|---|
/// | `GET {path}/api/releases` | the releases seen (`view`) |
/// | `POST {path}/api/releases` `{release, environment?, commit?, url?, notes?, deployed_at?}` | the release (`configure`) |
/// | `GET {path}/api/settings` | the settings that can change and the ones that cannot (`view`) |
/// | `PUT {path}/api/settings` `{values: {key: value \| null}}` | the settings (`configure`) |
/// | `GET {path}/api/alerts/channels` | the channels and their state (`view`) |
/// | `PUT {path}/api/alerts/channels/{name}` `{on?, muted?, minutes?}` | the channel (`configure`) |
/// | `POST {path}/api/alerts/channels/{name}/test` | `{channel, ok, error?}` (`configure`) |
/// | `GET {path}/api/alerts/history?page=&per_page=` | a page of deliveries (`view`) |
pub struct LighthouseConfigController {
    configure: Arc<Configure>,
}

/// The changes to settings, alert channels and releases, and the audit
/// entries they leave: one service behind both the JSON API and the
/// dashboard's forms, so a test alert is limited per channel across both.
pub(crate) struct Configure {
    pub(crate) lighthouse: Arc<Lighthouse>,
    audit: Option<Arc<dyn AuditSink>>,
    /// When each channel's last test alerts went out.
    tests: Mutex<HashMap<String, VecDeque<Instant>>>,
}

/// Test alerts one channel may send through this process in
/// [`TEST_WINDOW`]: enough to try a channel, too few to flood it.
const TESTS_PER_WINDOW: usize = 5;
const TEST_WINDOW: Duration = Duration::from_secs(60);

impl Injectable for Configure {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Lighthouse>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Configure {
                lighthouse: r.get::<Lighthouse>().await?,
                audit: r.app().try_get::<dyn AuditSink>().await,
                tests: Mutex::new(HashMap::new()),
            })
        })
    }
}

impl Injectable for LighthouseConfigController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Configure>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(LighthouseConfigController {
                configure: r.get::<Configure>().await?,
            })
        })
    }
}

/// `PUT {path}/api/settings`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct SettingsInput {
    /// Setting key to its new value, or `null` for what the configuration
    /// says.
    values: BTreeMap<String, Value>,
}

impl SettingsInput {
    /// The changes of a form: setting key to its new value, or `None` for
    /// what the configuration says.
    pub(crate) fn from_changes(changes: BTreeMap<String, Option<Value>>) -> SettingsInput {
        SettingsInput {
            values: changes
                .into_iter()
                .map(|(key, value)| (key, value.unwrap_or(Value::Null)))
                .collect(),
        }
    }
}

fn api(summary: &str) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec![TAG.to_owned()],
        ..ApiMeta::default()
    }
}

/// The store, or a 503 when Lighthouse keeps nothing.
fn store_of(lighthouse: &Lighthouse) -> Result<&LighthouseStore, AppError> {
    lighthouse
        .store()
        .ok_or_else(|| AppError::service_unavailable("lighthouse.store is off"))
}

impl Controller for LighthouseConfigController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let configure = &self.configure;
        let options = configure.lighthouse.options();
        if !options.dashboard
            || options.store == StoreKind::Off
            || configure.lighthouse.store().is_none()
        {
            return Vec::new();
        }
        let base = format!("!{}/api", options.mount_path());
        let path = |rest: &str| format!("{base}{rest}");
        let (releases, report, settings_get, settings_put) = (
            configure.clone(),
            configure.clone(),
            configure.clone(),
            configure.clone(),
        );
        let (channels, channel_put, channel_test, history) = (
            configure.clone(),
            configure.clone(),
            configure.clone(),
            configure.clone(),
        );
        vec![
            RouteDef::on(Method::GET, path("/releases"), move || {
                let this = releases.clone();
                async move { this.releases().await }
            })
            .can(VIEW, LighthouseSubject::SUBJECT, None)
            .name("lighthouse.api.releases")
            .json_only()
            .api(api(
                "The releases seen, with the issues each introduced and regressed",
            )),
            RouteDef::on(
                Method::POST,
                path("/releases"),
                move |CurrentPrincipal(principal): CurrentPrincipal,
                      ip: ClientIp,
                      headers: HeaderMap,
                      input: Json<ReleaseReport>| {
                    let this = report.clone();
                    async move {
                        this.report_release(Actor::new(principal.user_id, ip, &headers), input.0)
                            .await
                    }
                },
            )
            .can(CONFIGURE, LighthouseSubject::SUBJECT, None)
            .name("lighthouse.api.releases.report")
            .json_only()
            .api(api("Report the release the app is at, as a deploy does")),
            RouteDef::on(Method::GET, path("/settings"), move || {
                let this = settings_get.clone();
                async move { this.settings().await }
            })
            .can(VIEW, LighthouseSubject::SUBJECT, None)
            .name("lighthouse.api.settings")
            .json_only()
            .api(api(
                "The settings that can change, and the options that cannot",
            )),
            RouteDef::on(
                Method::PUT,
                path("/settings"),
                move |CurrentPrincipal(principal): CurrentPrincipal,
                      ip: ClientIp,
                      headers: HeaderMap,
                      input: Json<SettingsInput>| {
                    let this = settings_put.clone();
                    async move {
                        this.change_settings(Actor::new(principal.user_id, ip, &headers), input.0)
                            .await
                    }
                },
            )
            .can(CONFIGURE, LighthouseSubject::SUBJECT, None)
            .name("lighthouse.api.settings.update")
            .json_only()
            .api(api(
                "Change settings; null returns one to what the configuration says",
            )),
            RouteDef::on(Method::GET, path("/alerts/channels"), move || {
                let this = channels.clone();
                async move { this.channels().await }
            })
            .can(VIEW, LighthouseSubject::SUBJECT, None)
            .name("lighthouse.api.alerts.channels")
            .json_only()
            .api(api(
                "The alert channels, their kinds and whether they are muted",
            )),
            RouteDef::on(
                Method::PUT,
                path("/alerts/channels/{name}"),
                move |CurrentPrincipal(principal): CurrentPrincipal,
                      ip: ClientIp,
                      headers: HeaderMap,
                      name: Path<String>,
                      input: Json<Value>| {
                    let this = channel_put.clone();
                    async move {
                        this.change_channel(
                            Actor::new(principal.user_id, ip, &headers),
                            &name.0,
                            input.0,
                        )
                        .await
                    }
                },
            )
            .can(CONFIGURE, LighthouseSubject::SUBJECT, None)
            .name("lighthouse.api.alerts.channels.update")
            .json_only()
            .api(api("Mute or unmute a channel, or change the kinds it gets")),
            RouteDef::on(
                Method::POST,
                path("/alerts/channels/{name}/test"),
                move |CurrentPrincipal(principal): CurrentPrincipal,
                      ip: ClientIp,
                      headers: HeaderMap,
                      name: Path<String>| {
                    let this = channel_test.clone();
                    async move {
                        this.test_channel(Actor::new(principal.user_id, ip, &headers), &name.0)
                            .await
                    }
                },
            )
            .can(CONFIGURE, LighthouseSubject::SUBJECT, None)
            .name("lighthouse.api.alerts.channels.test")
            .json_only()
            .api(api("Send a test alert through one channel")),
            RouteDef::on(
                Method::GET,
                path("/alerts/history"),
                move |q: Query<PageQuery>| {
                    let this = history.clone();
                    async move { this.history(q.0).await }
                },
            )
            .can(VIEW, LighthouseSubject::SUBJECT, None)
            .name("lighthouse.api.alerts.history")
            .json_only()
            .api(api("Alert deliveries, newest first")),
        ]
    }
}

/// Who made a change, for the audit entry.
pub(crate) struct Actor {
    user: Uuid,
    ip: ClientIp,
    agent: Option<String>,
}

impl Actor {
    pub(crate) fn new(user: Uuid, ip: ClientIp, headers: &HeaderMap) -> Actor {
        Actor {
            user,
            ip,
            agent: headers
                .get(http::header::USER_AGENT)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        }
    }
}

/// A channel as the API shows it.
pub(crate) fn channel_json(info: &ChannelInfo) -> Value {
    json!({
        "name": info.name,
        "detail": info.detail,
        "kinds": info.kinds,
        "configured_kinds": info.configured_kinds,
        "muted": info.muted,
        "muted_until": info.state.muted_until.filter(|_| info.muted),
        "customised": !info.state.is_default(),
    })
}

impl Configure {
    fn store(&self) -> Result<&LighthouseStore, AppError> {
        store_of(&self.lighthouse)
    }

    async fn audit(
        &self,
        actor: &Actor,
        action: &str,
        subject: (&str, String),
        old: Value,
        new: Value,
    ) {
        let Some(sink) = &self.audit else {
            return;
        };
        let mut entry = AuditEntry::new(AuditKind::Custom, action)
            .actor(actor.user)
            .ip(actor.ip.0.to_string())
            .subject(subject.0, subject.1)
            .old_values(old)
            .new_values(new);
        if let Some(agent) = &actor.agent {
            entry = entry.user_agent(agent.clone());
        }
        if let Err(error) = sink.record(entry).await {
            tracing::warn!(target: "navree::lighthouse", %error, action, "could not write an audit entry");
        }
    }

    async fn releases(&self) -> Result<Json<Value>, AppError> {
        let releases = self.store()?.releases(100).await?;
        Ok(Json(json!(releases)))
    }

    async fn report_release(
        &self,
        actor: Actor,
        report: ReleaseReport,
    ) -> Result<Json<Value>, AppError> {
        let name = report.release.trim().to_owned();
        let summary = self.lighthouse.report_release(report).await?;
        self.audit(
            &actor,
            "lighthouse.release.reported",
            ("lighthouse_release", name),
            Value::Null,
            json!({ "release": summary.name, "environment": summary.environment, "deployed_at": summary.deployed_at }),
        )
        .await;
        Ok(Json(json!(summary)))
    }

    async fn settings(&self) -> Result<Json<Value>, AppError> {
        Ok(Json(self.settings_json().await?))
    }

    /// The settings that can change, and the options that cannot.
    pub(crate) async fn settings_json(&self) -> Result<Value, AppError> {
        let stored = self.store()?.stored_settings().await?;
        Ok(json!({
            "settings": setting_rows(self.lighthouse.tuning(), &stored),
            "read_only": crate::settings_view::read_only(self.lighthouse.options()),
        }))
    }

    pub(crate) async fn change_settings(
        &self,
        actor: Actor,
        input: SettingsInput,
    ) -> Result<Json<Value>, AppError> {
        if input.values.is_empty() {
            return Err(AppError::bad_request(
                "values must name at least one setting",
            ));
        }
        let tuning = self.lighthouse.tuning();
        let mut changes: BTreeMap<String, Option<Value>> = BTreeMap::new();
        let mut problems = Vec::new();
        for (key, value) in input.values {
            if value.is_null() {
                if settings::definition(&key).is_some() {
                    changes.insert(key, None);
                } else {
                    problems.push(format!("`{key}` is not a setting that can change"));
                }
                continue;
            }
            match settings::check(&key, &value) {
                Ok(value) => {
                    changes.insert(key, Some(value));
                }
                Err(problem) => problems.push(problem),
            }
        }
        if !problems.is_empty() {
            return Err(AppError::bad_request(problems.join("; ")));
        }
        let old: Map<String, Value> = changes
            .keys()
            .map(|key| (key.clone(), tuning.current(key)))
            .collect();
        self.store()?
            .save_settings(changes.clone(), Some(actor.user.to_string()))
            .await?;
        let new: Map<String, Value> = changes
            .keys()
            .map(|key| (key.clone(), tuning.current(key)))
            .collect();
        self.audit(
            &actor,
            "lighthouse.settings.updated",
            (
                "lighthouse_settings",
                changes.keys().cloned().collect::<Vec<_>>().join(","),
            ),
            Value::Object(old),
            Value::Object(new),
        )
        .await;
        Ok(Json(self.settings_json().await?))
    }

    async fn channels(&self) -> Result<Json<Value>, AppError> {
        Ok(Json(self.channels_json()))
    }

    /// The channels as the API shows them.
    pub(crate) fn channels_json(&self) -> Value {
        let infos = self
            .lighthouse
            .alerter()
            .map(|alerter| alerter.channel_infos())
            .unwrap_or_default();
        Value::Array(infos.iter().map(channel_json).collect())
    }

    pub(crate) fn channel_info(&self, name: &str) -> Result<ChannelInfo, AppError> {
        self.lighthouse
            .alerter()
            .and_then(|alerter| {
                alerter
                    .channel_infos()
                    .into_iter()
                    .find(|info| info.name == name)
            })
            .ok_or_else(|| {
                AppError::not_found().with_detail(format!("alert channel `{name}` not found"))
            })
    }

    pub(crate) async fn change_channel(
        &self,
        actor: Actor,
        name: &str,
        body: Value,
    ) -> Result<Json<Value>, AppError> {
        let before = self.channel_info(name)?;
        let Value::Object(body) = body else {
            return Err(AppError::bad_request("the body must be an object"));
        };
        let mut state = before.state.clone();
        for key in body.keys() {
            if !["on", "muted", "minutes"].contains(&key.as_str()) {
                return Err(AppError::bad_request(format!(
                    "`{key}` is not a channel setting (on, muted, minutes)"
                )));
            }
        }
        match body.get("on") {
            None => {}
            Some(Value::Null) => state.on = None,
            Some(Value::Array(items)) => {
                let mut kinds = Vec::new();
                for item in items {
                    let kind = item.as_str().and_then(AlertKind::parse).ok_or_else(|| {
                        AppError::bad_request(format!(
                            "`{item}` is not an alert kind ({})",
                            AlertKind::ALL.map(|k| k.as_str()).join(", ")
                        ))
                    })?;
                    kinds.push(kind);
                }
                // In the order the kinds are listed, each once.
                let kinds: Vec<AlertKind> = AlertKind::ALL
                    .into_iter()
                    .filter(|kind| kinds.contains(kind))
                    .collect();
                if kinds.is_empty() {
                    return Err(AppError::bad_request(
                        "give at least one kind, or mute the channel",
                    ));
                }
                state.on = Some(kinds);
            }
            Some(_) => {
                return Err(AppError::bad_request(
                    "on must be a list of kinds, or null for the configured ones",
                ));
            }
        }
        let minutes = match body.get("minutes") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .filter(|m| (1..=525_600).contains(m))
                    .ok_or_else(|| AppError::bad_request("minutes must be from 1 to 525600"))?,
            ),
        };
        match body.get("muted") {
            None => {
                if minutes.is_some() {
                    return Err(AppError::bad_request("minutes goes with muted: true"));
                }
            }
            Some(Value::Bool(true)) => {
                state.muted = true;
                state.muted_until = minutes.map(|m| Utc::now() + TimeDelta::minutes(m as i64));
            }
            Some(Value::Bool(false)) => {
                if minutes.is_some() {
                    return Err(AppError::bad_request("minutes goes with muted: true"));
                }
                state.muted = false;
                state.muted_until = None;
            }
            Some(_) => return Err(AppError::bad_request("muted must be true or false")),
        }
        let key = format!("{CHANNEL_PREFIX}{name}");
        let stored = if state.is_default() {
            None
        } else {
            Some(serde_json::to_value(&state).map_err(AppError::internal)?)
        };
        self.store()?
            .save_settings(
                BTreeMap::from([(key, stored)]),
                Some(actor.user.to_string()),
            )
            .await?;
        let after = self.channel_info(name)?;
        self.audit(
            &actor,
            "lighthouse.alert_channel.updated",
            ("lighthouse_alert_channel", name.to_owned()),
            channel_state_json(&before.state),
            channel_state_json(&after.state),
        )
        .await;
        Ok(Json(channel_json(&after)))
    }

    /// Takes a place in `name`'s window of test alerts, or says how long
    /// until there is one.
    fn take_test_slot(&self, name: &str, now: Instant) -> Result<(), Duration> {
        let mut tests = self.tests.lock().unwrap_or_else(PoisonError::into_inner);
        let sent = tests.entry(name.to_owned()).or_default();
        while sent
            .front()
            .is_some_and(|at| now.duration_since(*at) >= TEST_WINDOW)
        {
            sent.pop_front();
        }
        if sent.len() >= TESTS_PER_WINDOW {
            let oldest = sent.front().copied().unwrap_or(now);
            return Err(TEST_WINDOW.saturating_sub(now.duration_since(oldest)));
        }
        sent.push_back(now);
        Ok(())
    }

    pub(crate) async fn test_channel(
        &self,
        actor: Actor,
        name: &str,
    ) -> Result<Json<Value>, AppError> {
        self.channel_info(name)?;
        self.take_test_slot(name, Instant::now())
            .map_err(AppError::too_many_requests)?;
        let results = self.lighthouse.test_alerts(Some(name)).await?;
        let alerter = self.lighthouse.alerter();
        let result = results.into_iter().next();
        let (ok, error) = match result {
            Some(result) => match result.result {
                Ok(()) => (true, None),
                Err(error) => (
                    false,
                    Some(alerter.map_or(error.clone(), |alerter| alerter.scrub(&error))),
                ),
            },
            None => (false, Some("the channel did not answer".to_owned())),
        };
        self.audit(
            &actor,
            "lighthouse.alert_channel.tested",
            ("lighthouse_alert_channel", name.to_owned()),
            Value::Null,
            json!({ "ok": ok, "error": error }),
        )
        .await;
        Ok(Json(json!({ "channel": name, "ok": ok, "error": error })))
    }

    async fn history(&self, query: PageQuery) -> Result<Json<Value>, AppError> {
        let page = self
            .store()?
            .alert_history(query.page.unwrap_or(1), query.per_page.unwrap_or(25))
            .await?;
        Ok(Json(json!({ "data": page.data, "meta": page.meta })))
    }
}

fn channel_state_json(state: &ChannelState) -> Value {
    serde_json::to_value(state).unwrap_or(Value::Null)
}
