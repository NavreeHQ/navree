//! [`Webhooks`]: endpoints, dispatching, delivery and the delivery log.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use http::header::{CONTENT_TYPE, HeaderName, HeaderValue, USER_AGENT};
use navree_contracts::{Clock, Dispatcher, Job, JobEnvelope, JobOptions, SystemClock};
use navree_crypto::Encrypter;
use navree_db::{Db, DbError, Page, PageQuery, Uow, prune_time_series};
use navree_events::EventBus;
use navree_http_client::{HttpClient, HttpClientError};
use navree_kernel::{AppContext, WeakAppContext};
use navree_orm::{Builder, Executor, Model as _, Record};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::changes;
use crate::entity::{attempt, delivery, endpoint, receipt};
use crate::event::{WebhookContext, WebhookTypeRegistration};
use crate::events::{WebhookDeliveryFailed, WebhookEndpointDisabled};
use crate::jobs::{Deliver, FanOut, MAX_ATTEMPTS, retry_delay};
use crate::migration::{ATTEMPTS_TABLE, DELIVERIES_TABLE};
use crate::signing::{SECRET_PREFIX, generate_secret, standard_key, standard_signature};
use crate::{
    Attempt, CreatedEndpoint, Delivery, DeliveryStatus, Endpoint, EndpointChanges, EventType,
    NewEndpoint, WebhooksError, WebhooksOptions,
};

/// The `User-Agent` of every delivery.
pub const USER_AGENT_VALUE: &str = "Navree-Webhooks/1";

/// The event type of [`Webhooks::send_test`] messages.
pub const TEST_EVENT_TYPE: &str = "webhook.test";

/// How long a rotated-out secret keeps signing.
pub const ROTATION_OVERLAP: Duration = Duration::from_secs(24 * 60 * 60);

/// The most response body kept per attempt.
const RESPONSE_BODY_LIMIT: usize = 4096;
/// The longest endpoint URL.
const MAX_URL_LEN: usize = 2048;
/// The longest description.
const MAX_DESCRIPTION_LEN: usize = 500;
/// The most extra headers per endpoint.
const MAX_HEADERS: usize = 20;
/// Headers an endpoint may not set: the module sends them.
const RESERVED_HEADERS: [&str; 7] = [
    "host",
    "content-length",
    "content-type",
    "transfer-encoding",
    "connection",
    "user-agent",
    "te",
];
/// Extra seconds a delivery job gets beyond the request timeout.
const JOB_TIMEOUT_SLACK_SECS: u64 = 30;

/// The webhooks service, provided and exported by
/// [`WebhooksModule`](crate::WebhooksModule): endpoints, dispatching,
/// delivery and the delivery log.
///
/// Writes take the unit of work first (`create_endpoint(&uow, ..)`) and are
/// recorded by navree-db's change observers (the secrets never are). The
/// methods without one (`send_test`, `retry`) join the task's current unit
/// of work when there is an unfinished one (a request handler's), and use
/// their own otherwise.
///
/// Outgoing webhooks need `DatabaseModule`, `QueueModule` (any `dyn
/// Dispatcher`) and `HttpClientModule` in the app; `EventsModule` receives
/// the module's events, and `CryptoModule` with an app key encrypts the
/// endpoint secrets at rest.
#[derive(Clone)]
pub struct Webhooks {
    inner: Arc<Inner>,
}

struct Inner {
    app: WeakAppContext,
    options: Arc<WebhooksOptions>,
    /// Registered event types, by name (first registration with a payload
    /// builder wins).
    types: Vec<WebhookTypeRegistration>,
    /// The services, resolved on first use (an app that only verifies
    /// incoming webhooks has none of them).
    deps: OnceCell<Deps>,
    /// Whether the "secrets are stored unencrypted" warning was logged.
    warned_plain: AtomicBool,
}

/// What the service uses from the app.
struct Deps {
    db: Option<Db>,
    client: Option<Arc<HttpClient>>,
    encrypter: Option<Arc<Encrypter>>,
    dispatcher: Option<Arc<dyn Dispatcher>>,
    events: Option<Arc<EventBus>>,
    clock: Arc<dyn Clock>,
}

impl Deps {
    fn db(&self) -> Result<&Db, WebhooksError> {
        self.db
            .as_ref()
            .ok_or(WebhooksError::Missing("DatabaseModule"))
    }

    fn client(&self) -> Result<&HttpClient, WebhooksError> {
        self.client
            .as_deref()
            .ok_or(WebhooksError::Missing("HttpClientModule"))
    }

    fn dispatcher(&self) -> Result<&Arc<dyn Dispatcher>, WebhooksError> {
        self.dispatcher
            .as_ref()
            .ok_or(WebhooksError::Missing("QueueModule (a job dispatcher)"))
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }
}

/// What one HTTP attempt gave.
struct SendOutcome {
    status_code: Option<u16>,
    succeeded: bool,
    response_body: Option<String>,
    error: Option<String>,
    duration_ms: u64,
}

impl SendOutcome {
    /// The delivery's `last_error` for this attempt.
    fn last_error(&self) -> Option<String> {
        match (&self.error, self.status_code) {
            (Some(error), _) => Some(error.clone()),
            (None, Some(code)) if !self.succeeded => Some(format!("the endpoint answered {code}")),
            _ => None,
        }
    }
}

/// An event to emit once the unit of work commits.
enum Emitted {
    Failed(WebhookDeliveryFailed),
    Disabled(WebhookEndpointDisabled),
}

/// How many rows [`Webhooks::prune`] deleted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    /// Deliveries (with their attempts).
    pub deliveries: u64,
    /// Incoming receipts.
    pub receipts: u64,
}

impl Webhooks {
    /// The service for `app` (built by the module at boot).
    pub(crate) fn new(app: &AppContext, options: Arc<WebhooksOptions>) -> Self {
        let mut types: Vec<WebhookTypeRegistration> = Vec::new();
        for contributed in app.contributions::<WebhookTypeRegistration>() {
            let registration = (*contributed.value).clone();
            match types.iter_mut().find(|t| t.name == registration.name) {
                Some(existing) => {
                    if existing.build.is_none() && registration.build.is_some() {
                        *existing = registration;
                    }
                }
                None => types.push(registration),
            }
        }
        types.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            inner: Arc::new(Inner {
                app: app.downgrade(),
                options,
                types,
                deps: OnceCell::new(),
                warned_plain: AtomicBool::new(false),
            }),
        }
    }

    /// The options the module runs with.
    pub fn options(&self) -> &WebhooksOptions {
        &self.inner.options
    }

    /// Every registered event type, by name.
    pub fn event_types(&self) -> Vec<EventType> {
        self.inner
            .types
            .iter()
            .map(|t| EventType {
                name: t.name.to_string(),
                description: t.description.to_string(),
            })
            .collect()
    }

    /// Whether `name` is a registered event type.
    pub fn is_registered(&self, name: &str) -> bool {
        self.registration(name).is_some()
    }

    fn registration(&self, name: &str) -> Option<&WebhookTypeRegistration> {
        self.inner.types.iter().find(|t| t.name == name)
    }

    fn app(&self) -> Result<AppContext, WebhooksError> {
        self.inner
            .app
            .upgrade()
            .ok_or(WebhooksError::Missing("a running app"))
    }

    async fn deps(&self) -> Result<&Deps, WebhooksError> {
        self.inner
            .deps
            .get_or_try_init(|| async {
                let app = self.app()?;
                Ok(Deps {
                    db: app.try_get::<Db>().await.map(|db| (*db).clone()),
                    client: app.try_get::<HttpClient>().await,
                    encrypter: app.try_get::<Encrypter>().await,
                    dispatcher: app.try_get::<dyn Dispatcher>().await,
                    events: app.try_get::<EventBus>().await,
                    clock: match app.try_get::<dyn Clock>().await {
                        Some(clock) => clock,
                        None => Arc::new(SystemClock),
                    },
                })
            })
            .await
    }

    /// The current time (the app's `dyn Clock` when it has one).
    pub async fn now(&self) -> DateTime<Utc> {
        match self.deps().await {
            Ok(deps) => deps.now(),
            Err(_) => Utc::now(),
        }
    }

    // ------------------------------------------------------------------
    // Endpoints
    // ------------------------------------------------------------------

    /// Creates an endpoint and returns it with its secret (`whsec_...`),
    /// which is shown only here.
    ///
    /// The URL must be `https` (unless `webhooks.require_https` is off),
    /// without credentials, and must not resolve to a private, loopback or
    /// cloud metadata address under the HTTP client's SSRF policy. The
    /// events must be registered types or `*`.
    pub async fn create_endpoint(
        &self,
        uow: &Uow,
        new: NewEndpoint,
    ) -> Result<CreatedEndpoint, WebhooksError> {
        let deps = self.deps().await?;
        let url = self.check_url(deps, &new.url).await?;
        let events = self.check_events(new.events)?;
        let headers = check_headers(new.headers)?;
        let description = check_description(new.description)?;
        let secret = generate_secret();
        let now = deps.now();
        let model = endpoint::Model {
            id: Uuid::now_v7(),
            workspace_id: new.workspace_id,
            url,
            description,
            events: json!(events),
            headers: json!(headers),
            secret: self.seal(deps, &secret)?,
            previous_secret: None,
            previous_secret_expires_at: None,
            enabled: true,
            disabled_at: None,
            disabled_reason: None,
            failing_since: None,
            created_at: now,
            updated_at: now,
        };
        // Written as it is, without the ORM's change event (which would
        // carry the secret); the event below leaves the secrets out.
        endpoint::Model::insert(uow, [model.clone()]).await?;
        changes::created(uow, &model);
        Ok(CreatedEndpoint {
            endpoint: Endpoint::from(&model),
            secret,
        })
    }

    /// Changes an endpoint's URL, events, description or headers (each
    /// checked as on create).
    pub async fn update_endpoint(
        &self,
        uow: &Uow,
        id: Uuid,
        changes: EndpointChanges,
    ) -> Result<Endpoint, WebhooksError> {
        let deps = self.deps().await?;
        let current = self.find_in(uow, id).await?;
        let mut model = current.clone();
        model.updated_at = deps.now();
        if let Some(url) = changes.url {
            model.url = self.check_url(deps, &url).await?;
        }
        if let Some(events) = changes.events {
            model.events = json!(self.check_events(events)?);
        }
        if let Some(description) = changes.description {
            model.description = check_description(Some(description))?;
        }
        if let Some(headers) = changes.headers {
            model.headers = json!(check_headers(headers)?);
        }
        write_endpoint(uow, &current, &model).await?;
        Ok(Endpoint::from(&model))
    }

    /// Deletes an endpoint with its deliveries and attempts (pending
    /// deliveries are dropped).
    pub async fn delete_endpoint(&self, uow: &Uow, id: Uuid) -> Result<(), WebhooksError> {
        let current = self.find_in(uow, id).await?;
        let deleted = endpoint::Model::query()
            .where_key(current.id)
            .delete(uow)
            .await?;
        if deleted == 0 {
            return Err(WebhooksError::EndpointNotFound(id));
        }
        changes::deleted(uow, &current);
        Ok(())
    }

    /// Gives an endpoint a new secret and returns it. The old secret keeps
    /// signing for 24 hours ([`ROTATION_OVERLAP`]): until then every
    /// delivery carries two signatures, one per secret.
    pub async fn rotate_secret(&self, uow: &Uow, id: Uuid) -> Result<String, WebhooksError> {
        let deps = self.deps().await?;
        let current = self.find_in(uow, id).await?;
        let secret = generate_secret();
        let now = deps.now();
        let overlap = TimeDelta::from_std(ROTATION_OVERLAP).unwrap_or(TimeDelta::MAX);
        let mut model = current.clone();
        model.secret = self.seal(deps, &secret)?;
        model.previous_secret = Some(current.secret.clone());
        model.previous_secret_expires_at = Some(now + overlap);
        model.updated_at = now;
        write_endpoint(uow, &current, &model).await?;
        Ok(secret)
    }

    /// Turns delivery to an endpoint back on (and forgets its failures).
    pub async fn enable(&self, uow: &Uow, id: Uuid) -> Result<Endpoint, WebhooksError> {
        let deps = self.deps().await?;
        let current = self.find_in(uow, id).await?;
        let mut model = current.clone();
        model.enabled = true;
        model.disabled_at = None;
        model.disabled_reason = None;
        model.failing_since = None;
        model.updated_at = deps.now();
        write_endpoint(uow, &current, &model).await?;
        Ok(Endpoint::from(&model))
    }

    /// Turns delivery to an endpoint off; its pending deliveries fail when
    /// their next attempt is due.
    pub async fn disable(&self, uow: &Uow, id: Uuid) -> Result<Endpoint, WebhooksError> {
        let deps = self.deps().await?;
        let current = self.find_in(uow, id).await?;
        let now = deps.now();
        let mut model = current.clone();
        model.enabled = false;
        model.disabled_at = Some(now);
        model.disabled_reason = Some("manual".to_owned());
        model.updated_at = now;
        write_endpoint(uow, &current, &model).await?;
        Ok(Endpoint::from(&model))
    }

    /// One endpoint.
    pub async fn endpoint(&self, id: Uuid) -> Result<Endpoint, WebhooksError> {
        let db = self.deps().await?.db()?;
        endpoint::Model::find(db, id)
            .await?
            .map(|record| Endpoint::from(record.model()))
            .ok_or(WebhooksError::EndpointNotFound(id))
    }

    /// The endpoints of a workspace (`None`: the app-wide ones), oldest
    /// first.
    pub async fn endpoints(
        &self,
        workspace_id: Option<Uuid>,
    ) -> Result<Vec<Endpoint>, WebhooksError> {
        let db = self.deps().await?.db()?;
        let models = in_workspace(endpoint::Model::query(), workspace_id)
            .order_by(endpoint::Model::CREATED_AT)
            .order_by(endpoint::Model::ID)
            .get(db)
            .await?;
        Ok(models
            .iter()
            .map(|record| Endpoint::from(record.model()))
            .collect())
    }

    /// Sends a `webhook.test` message to an endpoint at once (even a
    /// disabled one) and returns the attempt. It is recorded in the
    /// delivery log and never counts towards disabling the endpoint.
    pub async fn send_test(&self, id: Uuid) -> Result<Attempt, WebhooksError> {
        let deps = self.deps().await?;
        let db = deps.db()?;
        let endpoint = endpoint::Model::find(db, id)
            .await?
            .map(Record::into_inner)
            .ok_or(WebhooksError::EndpointNotFound(id))?;
        let now = deps.now();
        let message_id = new_message_id();
        let payload = render_body(TEST_EVENT_TYPE, now, &json!({ "endpoint_id": id }))?;
        let outcome = self.send(deps, &endpoint, &message_id, &payload).await?;
        let attempt = self
            .in_uow(db, |uow| async move {
                let delivery = delivery::Model {
                    id: Uuid::now_v7(),
                    endpoint_id: endpoint.id,
                    workspace_id: endpoint.workspace_id,
                    message_id,
                    event_type: TEST_EVENT_TYPE.to_owned(),
                    payload,
                    status: if outcome.succeeded {
                        DeliveryStatus::Succeeded
                    } else {
                        DeliveryStatus::Failed
                    }
                    .as_str()
                    .to_owned(),
                    attempts: 1,
                    max_attempts: 1,
                    next_attempt_at: None,
                    last_status_code: outcome.status_code.map(i32::from),
                    last_error: outcome.last_error(),
                    created_at: now,
                    updated_at: now,
                };
                delivery::Model::insert(&uow, [delivery.clone()]).await?;
                let attempt = insert_attempt(&uow, &delivery, 1, &outcome, now).await?;
                Ok(attempt)
            })
            .await?;
        Ok(Attempt::from(&attempt))
    }

    // ------------------------------------------------------------------
    // The delivery log
    // ------------------------------------------------------------------

    /// An endpoint's deliveries, newest first.
    pub async fn deliveries(
        &self,
        endpoint_id: Uuid,
        page: PageQuery,
    ) -> Result<Page<Delivery>, WebhooksError> {
        let db = self.deps().await?.db()?;
        if endpoint::Model::query()
            .where_key(endpoint_id)
            .doesnt_exist(db)
            .await?
        {
            return Err(WebhooksError::EndpointNotFound(endpoint_id));
        }
        let page = delivery::Model::query()
            .where_(delivery::Model::ENDPOINT_ID.eq(endpoint_id))
            .order_by_desc(delivery::Model::CREATED_AT)
            .order_by_desc(delivery::Model::ID)
            .paginate(db, page)
            .await?;
        Ok(page.map(|record| Delivery::from(record.model())))
    }

    /// One delivery.
    pub async fn delivery(&self, id: Uuid) -> Result<Delivery, WebhooksError> {
        let db = self.deps().await?.db()?;
        delivery::Model::find(db, id)
            .await?
            .map(|record| Delivery::from(record.model()))
            .ok_or(WebhooksError::DeliveryNotFound(id))
    }

    /// A delivery's attempts, first first.
    pub async fn attempts(&self, delivery_id: Uuid) -> Result<Vec<Attempt>, WebhooksError> {
        let db = self.deps().await?.db()?;
        let models = attempt::Model::query()
            .where_(attempt::Model::DELIVERY_ID.eq(delivery_id))
            .order_by(attempt::Model::ATTEMPT)
            .get(db)
            .await?;
        Ok(models
            .iter()
            .map(|record| Attempt::from(record.model()))
            .collect())
    }

    /// Queues a failed delivery again, keeping its id and `webhook-id`: it
    /// gets one more attempt, made as soon as a worker is free. Anything
    /// but a failed delivery is a [`WebhooksError::NotRetryable`] (409).
    pub async fn retry(&self, delivery_id: Uuid) -> Result<Delivery, WebhooksError> {
        let deps = self.deps().await?;
        let db = deps.db()?;
        self.in_uow(db, |uow| async move {
            let mut delivery = delivery::Model::find(&uow, delivery_id)
                .await?
                .map(Record::into_inner)
                .ok_or(WebhooksError::DeliveryNotFound(delivery_id))?;
            let status = DeliveryStatus::parse(&delivery.status);
            if status != DeliveryStatus::Failed {
                return Err(WebhooksError::NotRetryable(status));
            }
            let now = deps.now();
            delivery.status = DeliveryStatus::Pending.as_str().to_owned();
            delivery.max_attempts = delivery.attempts + 1;
            delivery.next_attempt_at = Some(now);
            delivery.updated_at = now;
            // A set-based update: the delivery log records no change events.
            delivery::Model::query()
                .where_key(delivery.id)
                .update(
                    &uow,
                    [
                        delivery::Model::STATUS.set(delivery.status.as_str()),
                        delivery::Model::MAX_ATTEMPTS.set(delivery.max_attempts),
                        delivery::Model::NEXT_ATTEMPT_AT.set(delivery.next_attempt_at),
                        delivery::Model::UPDATED_AT.set(now),
                    ],
                )
                .await?;
            self.dispatch_job(
                deps,
                &Deliver {
                    delivery_id: delivery.id,
                },
                None,
            )
            .await?;
            Ok(Delivery::from(&delivery))
        })
        .await
    }

    /// Queues every failed delivery again (see [`retry`](Webhooks::retry));
    /// returns how many.
    pub async fn retry_failed(&self) -> Result<u64, WebhooksError> {
        let db = self.deps().await?.db()?;
        let failed = delivery::Model::query()
            .where_(delivery::Model::STATUS.eq(DeliveryStatus::Failed.as_str()))
            .order_by(delivery::Model::CREATED_AT)
            .pluck(db, delivery::Model::ID)
            .await?;
        let mut count = 0;
        for delivery_id in failed {
            self.retry(delivery_id).await?;
            count += 1;
        }
        Ok(count)
    }

    /// Deletes deliveries (with their attempts) older than `days`
    /// (default `webhooks.retain_days`) and incoming receipts older than
    /// `days` (default `webhooks.incoming.receipt_days`). On TimescaleDB,
    /// where the delivery log is made of hypertables, the days entirely
    /// before the cutoff go as whole chunks (see
    /// [`prune_time_series`](navree_db::prune_time_series)).
    pub async fn prune(&self, days: Option<u64>) -> Result<Pruned, WebhooksError> {
        let deps = self.deps().await?;
        let db = deps.db()?;
        let now = deps.now();
        let cutoff = |n: u64| {
            now.checked_sub_signed(days_of(n))
                .unwrap_or(DateTime::<Utc>::MIN_UTC)
        };
        let deliveries_before = cutoff(days.unwrap_or(self.options().retain_days));
        let receipts_before = cutoff(days.unwrap_or(self.options().incoming.receipt_days));
        self.in_uow(db, |uow| async move {
            // Attempts no older than their delivery: the old ones first (by
            // whole chunks), then the late retries of the deliveries that
            // go, then those deliveries.
            prune_time_series(&uow, ATTEMPTS_TABLE, "created_at", deliveries_before).await?;
            attempt::Model::query()
                .where_(
                    attempt::Model::DELIVERY_ID.in_subquery(
                        delivery::Model::query()
                            .select([delivery::Model::ID])
                            .where_(delivery::Model::CREATED_AT.lt(deliveries_before)),
                    ),
                )
                .delete(&uow)
                .await?;
            let deliveries =
                prune_time_series(&uow, DELIVERIES_TABLE, "created_at", deliveries_before)
                    .await?;
            let receipts = receipt::Model::query()
                .where_(receipt::Model::RECEIVED_AT.lt(receipts_before))
                .delete(&uow)
                .await?;
            Ok(Pruned {
                deliveries,
                receipts,
            })
        })
        .await
    }

    /// How many failed deliveries each endpoint has.
    pub(crate) async fn failed_count(&self, endpoint_id: Uuid) -> Result<u64, WebhooksError> {
        let db = self.deps().await?.db()?;
        Ok(delivery::Model::query()
            .where_(delivery::Model::ENDPOINT_ID.eq(endpoint_id))
            .where_(delivery::Model::STATUS.eq(DeliveryStatus::Failed.as_str()))
            .count(db)
            .await?)
    }

    /// Deletes every endpoint of a deleted workspace.
    pub(crate) async fn drop_workspace(&self, workspace_id: Uuid) -> Result<u64, WebhooksError> {
        let db = self.deps().await?.db()?;
        self.in_uow(db, |uow| async move {
            Ok(endpoint::Model::query()
                .where_(endpoint::Model::WORKSPACE_ID.eq(workspace_id))
                .delete(&uow)
                .await?)
        })
        .await
    }

    // ------------------------------------------------------------------
    // Dispatching
    // ------------------------------------------------------------------

    /// Sends a webhook that is not an event: `data` goes to the endpoints
    /// subscribed to `event_type`, which must be registered (by an event,
    /// or with [`webhook_type`](crate::ModuleBuilderWebhooksExt::webhook_type)).
    ///
    /// ```ignore
    /// webhooks
    ///     .dispatch_json("export.ready", json!({ "export_id": export.id, "url": url }))
    ///     .workspace(workspace_id)
    ///     .send()
    ///     .await?;
    /// ```
    pub fn dispatch_json(&self, event_type: impl Into<String>, data: Value) -> Dispatch<'_> {
        Dispatch {
            webhooks: self,
            event_type: event_type.into(),
            data,
            workspace_id: None,
        }
    }

    /// Queues the fan-out job of an emitted event (from the listener).
    pub(crate) async fn queue_event(
        &self,
        event_type: &'static str,
        workspace_id: Option<Uuid>,
        event: Value,
    ) -> Result<String, WebhooksError> {
        let deps = self.deps().await?;
        let job = FanOut {
            event_type: event_type.to_owned(),
            workspace_id,
            message_id: new_message_id(),
            timestamp: deps.now(),
            event: Some(event),
            data: None,
        };
        self.dispatch_job(deps, &job, None).await?;
        Ok(job.message_id)
    }

    /// Queues `job` on `webhooks.queue`, after `delay` when there is one.
    /// Inside a unit of work's scope the job joins it.
    async fn dispatch_job<J: Job>(
        &self,
        deps: &Deps,
        job: &J,
        delay: Option<Duration>,
    ) -> Result<String, WebhooksError> {
        let mut options = JobOptions {
            queue: self.options().queue.clone(),
            ..J::options()
        };
        if J::NAME == Deliver::NAME {
            options.timeout_secs = self.options().timeout_secs + JOB_TIMEOUT_SLACK_SECS;
        }
        let envelope = JobEnvelope {
            job_type: J::NAME.to_owned(),
            payload: serde_json::to_value(job).map_err(|e| WebhooksError::Queue(e.into()))?,
            options,
            delay_secs: delay.map(|d| d.as_secs() + u64::from(d.subsec_nanos() > 0)),
            unique_key: None,
        };
        deps.dispatcher()?
            .dispatch_envelope(envelope)
            .await
            .map_err(WebhooksError::Queue)
    }

    /// The fan-out job: builds the payload, stores one delivery per
    /// subscribed endpoint and queues their first attempts.
    pub(crate) async fn fan_out(&self, job: FanOut) -> Result<usize, WebhooksError> {
        let deps = self.deps().await?;
        let db = deps.db()?;
        let data = match (job.event, job.data) {
            (Some(event), _) => {
                let build = self
                    .registration(&job.event_type)
                    .and_then(|r| r.build)
                    .ok_or_else(|| WebhooksError::UnknownEventType(job.event_type.clone()))?;
                let ctx = WebhookContext::new(self.app()?, db.clone());
                build(event, &ctx)
                    .await
                    .map_err(|source| WebhooksError::Payload {
                        event_type: job.event_type.clone(),
                        source,
                    })?
            }
            (None, Some(data)) => data,
            (None, None) => Value::Null,
        };
        let payload = render_body(&job.event_type, job.timestamp, &data)?;
        self.check_size(&payload)?;
        let already: HashSet<Uuid> = delivery::Model::query()
            .where_(delivery::Model::MESSAGE_ID.eq(job.message_id.as_str()))
            .pluck(db, delivery::Model::ENDPOINT_ID)
            .await?
            .into_iter()
            .collect();
        let endpoints: Vec<endpoint::Model> =
            in_workspace(endpoint::Model::query(), job.workspace_id)
                .where_(endpoint::Model::ENABLED.eq(true))
                .order_by(endpoint::Model::CREATED_AT)
                .get(db)
                .await?
                .into_models()
                .into_iter()
                .filter(|e| Endpoint::from(e).subscribes_to(&job.event_type))
                .filter(|e| !already.contains(&e.id))
                .collect();
        if endpoints.is_empty() {
            return Ok(0);
        }
        let count = endpoints.len();
        let now = deps.now();
        self.in_uow(db, |uow| async move {
            let mut ids = Vec::with_capacity(endpoints.len());
            {
                for endpoint in &endpoints {
                    let id = Uuid::now_v7();
                    let delivery = delivery::Model {
                        id,
                        endpoint_id: endpoint.id,
                        workspace_id: endpoint.workspace_id,
                        message_id: job.message_id.clone(),
                        event_type: job.event_type.clone(),
                        payload: payload.clone(),
                        status: DeliveryStatus::Pending.as_str().to_owned(),
                        attempts: 0,
                        max_attempts: MAX_ATTEMPTS as i32,
                        next_attempt_at: Some(now),
                        last_status_code: None,
                        last_error: None,
                        created_at: now,
                        updated_at: now,
                    };
                    delivery::Model::insert(&uow, [delivery]).await?;
                    ids.push(id);
                }
            }
            for delivery_id in ids {
                self.dispatch_job(deps, &Deliver { delivery_id }, None)
                    .await?;
            }
            Ok(())
        })
        .await?;
        Ok(count)
    }

    /// The delivery job: one attempt, then the outcome.
    pub(crate) async fn deliver(&self, delivery_id: Uuid) -> Result<(), WebhooksError> {
        let deps = self.deps().await?;
        let db = deps.db()?;
        let Some(delivery) = delivery::Model::find(db, delivery_id)
            .await?
            .map(Record::into_inner)
        else {
            return Ok(()); // deleted with its endpoint, or pruned
        };
        if DeliveryStatus::parse(&delivery.status) != DeliveryStatus::Pending {
            return Ok(());
        }
        let Some(endpoint) = endpoint::Model::find(db, delivery.endpoint_id)
            .await?
            .map(Record::into_inner)
        else {
            return Ok(());
        };
        if !endpoint.enabled {
            return self
                .fail_unsent(deps, delivery, "the endpoint is disabled")
                .await;
        }
        let outcome = self
            .send(deps, &endpoint, &delivery.message_id, &delivery.payload)
            .await?;
        self.record(deps, delivery, endpoint, outcome).await
    }

    /// Marks a pending delivery failed without an attempt.
    async fn fail_unsent(
        &self,
        deps: &Deps,
        delivery: delivery::Model,
        reason: &str,
    ) -> Result<(), WebhooksError> {
        let db = deps.db()?;
        let now = deps.now();
        let event = WebhookDeliveryFailed {
            delivery_id: delivery.id,
            endpoint_id: delivery.endpoint_id,
            workspace_id: delivery.workspace_id,
            event_type: delivery.event_type.clone(),
            attempts: u32::try_from(delivery.attempts).unwrap_or_default(),
            last_status_code: delivery
                .last_status_code
                .and_then(|c| u16::try_from(c).ok()),
        };
        let reason = reason.to_owned();
        self.in_uow(db, |uow| async move {
            {
                delivery::Model::query()
                    .where_key(delivery.id)
                    .update(
                        &uow,
                        [
                            delivery::Model::STATUS.set(DeliveryStatus::Failed.as_str()),
                            delivery::Model::NEXT_ATTEMPT_AT.set(None),
                            delivery::Model::LAST_ERROR.set(reason),
                            delivery::Model::UPDATED_AT.set(now),
                        ],
                    )
                    .await?;
            }
            self.emit_after_commit(deps, &uow, vec![Emitted::Failed(event)]);
            Ok(())
        })
        .await
    }

    /// Records an attempt of a real delivery: the attempt row, the
    /// delivery's new state (and its next attempt), and the endpoint's
    /// health (failing since, disabled).
    async fn record(
        &self,
        deps: &Deps,
        delivery: delivery::Model,
        endpoint: endpoint::Model,
        outcome: SendOutcome,
    ) -> Result<(), WebhooksError> {
        let db = deps.db()?;
        let now = deps.now();
        let attempts = delivery.attempts + 1;
        let options = self.options();
        // Test messages (`send_test`, and their retries) never change the
        // endpoint's health and emit nothing.
        let real = delivery.event_type != TEST_EVENT_TYPE;
        self.in_uow(db, |uow| async move {
            let mut emitted = Vec::new();
            let mut next_delay = None;
            {
                insert_attempt(&uow, &delivery, attempts, &outcome, now).await?;
                let mut endpoint_disabled = false;
                if !real {
                    tracing::debug!(target: "navree::webhooks", delivery = %delivery.id, "test message attempt recorded");
                } else if outcome.succeeded {
                    if endpoint.failing_since.is_some() {
                        endpoint::Model::query()
                            .where_key(endpoint.id)
                            .update(&uow, [endpoint::Model::FAILING_SINCE.set(None)])
                            .await?;
                    }
                } else {
                    let failing_since = endpoint.failing_since.unwrap_or(now);
                    let gone = outcome.status_code == Some(410);
                    let failing_too_long = options.disable_after_days > 0
                        && now - failing_since >= days_of(options.disable_after_days);
                    if gone || failing_too_long {
                        let reason = if gone { "gone" } else { "failing" };
                        endpoint::Model::query()
                            .where_key(endpoint.id)
                            .update(
                                &uow,
                                [
                                    endpoint::Model::ENABLED.set(false),
                                    endpoint::Model::DISABLED_AT.set(now),
                                    endpoint::Model::DISABLED_REASON.set(reason),
                                    endpoint::Model::FAILING_SINCE.set(failing_since),
                                    endpoint::Model::UPDATED_AT.set(now),
                                ],
                            )
                            .await?;
                        endpoint_disabled = true;
                        emitted.push(Emitted::Disabled(WebhookEndpointDisabled {
                            endpoint_id: endpoint.id,
                            workspace_id: endpoint.workspace_id,
                            url: endpoint.url.clone(),
                            reason: reason.to_owned(),
                        }));
                    } else if endpoint.failing_since.is_none() {
                        endpoint::Model::query()
                            .where_key(endpoint.id)
                            .update(&uow, [endpoint::Model::FAILING_SINCE.set(now)])
                            .await?;
                    }
                }
                let status = if outcome.succeeded {
                    DeliveryStatus::Succeeded
                } else if endpoint_disabled || attempts >= delivery.max_attempts {
                    DeliveryStatus::Failed
                } else {
                    match retry_delay(u32::try_from(attempts).unwrap_or(u32::MAX)) {
                        Some(delay) => {
                            next_delay = Some(delay);
                            DeliveryStatus::Pending
                        }
                        None => DeliveryStatus::Failed,
                    }
                };
                if real && status == DeliveryStatus::Failed {
                    emitted.push(Emitted::Failed(WebhookDeliveryFailed {
                        delivery_id: delivery.id,
                        endpoint_id: delivery.endpoint_id,
                        workspace_id: delivery.workspace_id,
                        event_type: delivery.event_type.clone(),
                        attempts: u32::try_from(attempts).unwrap_or_default(),
                        last_status_code: outcome.status_code,
                    }));
                }
                let next_attempt_at = next_delay
                    .map(|delay| now + TimeDelta::from_std(delay).unwrap_or(TimeDelta::MAX));
                delivery::Model::query()
                    .where_key(delivery.id)
                    .update(
                        &uow,
                        [
                            delivery::Model::STATUS.set(status.as_str()),
                            delivery::Model::ATTEMPTS.set(attempts),
                            delivery::Model::NEXT_ATTEMPT_AT.set(next_attempt_at),
                            delivery::Model::LAST_STATUS_CODE
                                .set(outcome.status_code.map(i32::from)),
                            delivery::Model::LAST_ERROR.set(outcome.last_error()),
                            delivery::Model::UPDATED_AT.set(now),
                        ],
                    )
                    .await?;
            }
            if let Some(delay) = next_delay {
                self.dispatch_job(
                    deps,
                    &Deliver {
                        delivery_id: delivery.id,
                    },
                    Some(delay),
                )
                .await?;
            }
            self.emit_after_commit(deps, &uow, emitted);
            Ok(())
        })
        .await
    }

    /// Emits `emitted` through the event bus once `uow` commits.
    fn emit_after_commit(&self, deps: &Deps, uow: &Uow, emitted: Vec<Emitted>) {
        let Some(bus) = deps.events.clone() else {
            return;
        };
        if emitted.is_empty() {
            return;
        }
        uow.after_commit(move || {
            Box::pin(async move {
                for event in emitted {
                    let result = match event {
                        Emitted::Failed(event) => bus.emit(event).await,
                        Emitted::Disabled(event) => bus.emit(event).await,
                    };
                    if let Err(error) = result {
                        tracing::warn!(target: "navree::webhooks", %error, "a webhooks event listener failed");
                    }
                }
            })
        });
    }

    /// Posts `payload` to `endpoint`, signed with its secrets.
    async fn send(
        &self,
        deps: &Deps,
        endpoint: &endpoint::Model,
        message_id: &str,
        payload: &str,
    ) -> Result<SendOutcome, WebhooksError> {
        let client = deps.client()?;
        let now = deps.now();
        let timestamp = now.timestamp();
        let signature = self
            .signing_secrets(deps, endpoint, now)?
            .iter()
            .map(|secret| {
                standard_signature(
                    &standard_key(secret),
                    message_id,
                    timestamp,
                    payload.as_bytes(),
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let headers: BTreeMap<String, String> =
            serde_json::from_value(endpoint.headers.clone()).unwrap_or_default();
        let mut request = client.post(&endpoint.url);
        for (name, value) in &headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let request = request
            .header(CONTENT_TYPE, "application/json")
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header("webhook-id", message_id)
            .header("webhook-timestamp", timestamp.to_string())
            .header("webhook-signature", signature)
            .body(payload.to_owned())
            .timeout(Duration::from_secs(self.options().timeout_secs));
        let started = Instant::now();
        let result = request.send().await;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(match result {
            Ok(response) => {
                let status = response.status().as_u16();
                SendOutcome {
                    status_code: Some(status),
                    succeeded: response.is_success(),
                    response_body: Some(truncate(response.text(), RESPONSE_BODY_LIMIT)),
                    error: None,
                    duration_ms,
                }
            }
            Err(error) => SendOutcome {
                status_code: None,
                succeeded: false,
                response_body: None,
                error: Some(describe_send_error(&error)),
                duration_ms,
            },
        })
    }

    /// The secrets that sign a delivery now: the current one, plus the
    /// previous one during the rotation overlap.
    fn signing_secrets(
        &self,
        deps: &Deps,
        endpoint: &endpoint::Model,
        now: DateTime<Utc>,
    ) -> Result<Vec<String>, WebhooksError> {
        let mut secrets = vec![self.open(deps, &endpoint.secret)?];
        if let (Some(previous), Some(until)) = (
            &endpoint.previous_secret,
            endpoint.previous_secret_expires_at,
        ) && until > now
        {
            secrets.push(self.open(deps, previous)?);
        }
        Ok(secrets)
    }

    /// The stored form of a secret: encrypted with the app key when the
    /// app has one, else as it is (with a warning, once).
    fn seal(&self, deps: &Deps, secret: &str) -> Result<String, WebhooksError> {
        match &deps.encrypter {
            Some(encrypter) if encrypter.current_kid().is_some() => {
                Ok(encrypter.encrypt_string(secret)?)
            }
            _ => {
                if !self.inner.warned_plain.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        target: "navree::webhooks",
                        "webhook endpoint secrets are stored unencrypted: add CryptoModule and set APP_KEY to encrypt them"
                    );
                }
                Ok(secret.to_owned())
            }
        }
    }

    /// A stored secret in clear.
    fn open(&self, deps: &Deps, stored: &str) -> Result<String, WebhooksError> {
        if stored.starts_with(SECRET_PREFIX) {
            return Ok(stored.to_owned());
        }
        let encrypter = deps.encrypter.as_ref().ok_or(WebhooksError::Missing(
            "CryptoModule (endpoint secrets are encrypted)",
        ))?;
        Ok(encrypter.decrypt_string(stored)?)
    }

    /// The endpoint `id`, read in `uow`.
    async fn find_in(&self, uow: &Uow, id: Uuid) -> Result<endpoint::Model, WebhooksError> {
        endpoint::Model::query()
            .where_key(id)
            .lock_for_update()
            .first(uow)
            .await?
            .map(Record::into_inner)
            .ok_or(WebhooksError::EndpointNotFound(id))
    }

    /// Runs `f` in the task's current unit of work when it is unfinished
    /// and on this database (a request's: the request commits it), else in
    /// a new one that commits when `f` succeeds.
    async fn in_uow<T, F, Fut>(&self, db: &Db, f: F) -> Result<T, WebhooksError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, WebhooksError>>,
    {
        if let Some(current) = Uow::current()
            && current.db().same_as(db)
            && !current.is_finished().await
        {
            return f(current).await;
        }
        let uow = db.uow(None, None).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await.map_err(DbError::from)?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::webhooks", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }

    // ------------------------------------------------------------------
    // Checks
    // ------------------------------------------------------------------

    async fn check_url(&self, deps: &Deps, url: &str) -> Result<String, WebhooksError> {
        let url = url.trim();
        if url.is_empty() {
            return Err(WebhooksError::invalid("url", "url should not be empty"));
        }
        if url.len() > MAX_URL_LEN {
            return Err(WebhooksError::invalid(
                "url",
                format!("url must be at most {MAX_URL_LEN} characters"),
            ));
        }
        let parsed = url::Url::parse(url)
            .map_err(|_| WebhooksError::invalid("url", "url must be a valid URL"))?;
        match parsed.scheme() {
            "https" => {}
            "http" if !self.options().require_https => {}
            "http" => return Err(WebhooksError::invalid("url", "url must use https")),
            _ => {
                return Err(WebhooksError::invalid(
                    "url",
                    "url must be an http or https URL",
                ));
            }
        }
        if parsed.host_str().is_none_or(str::is_empty) {
            return Err(WebhooksError::invalid("url", "url must have a host"));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(WebhooksError::invalid(
                "url",
                "url must not contain credentials (send them in a header)",
            ));
        }
        if let Some(client) = &deps.client {
            match client.check_destination(url).await {
                Ok(()) | Err(HttpClientError::Connect { .. }) => {}
                Err(HttpClientError::Blocked { reason, .. }) => {
                    return Err(WebhooksError::invalid(
                        "url",
                        format!("url points to an address the SSRF policy refuses ({reason})"),
                    ));
                }
                Err(_) => return Err(WebhooksError::invalid("url", "url must be a valid URL")),
            }
        }
        Ok(parsed.to_string())
    }

    fn check_events(&self, events: Vec<String>) -> Result<Vec<String>, WebhooksError> {
        let mut checked: Vec<String> = Vec::new();
        for event in events {
            let event = event.trim().to_owned();
            if event.is_empty() {
                continue;
            }
            if event != "*" && !self.is_registered(&event) {
                return Err(WebhooksError::UnknownEventType(event));
            }
            if !checked.contains(&event) {
                checked.push(event);
            }
        }
        if checked.is_empty() {
            return Err(WebhooksError::invalid(
                "events",
                "events must list at least one event type (or `*`)",
            ));
        }
        Ok(checked)
    }

    fn check_size(&self, payload: &str) -> Result<(), WebhooksError> {
        let limit = self.options().max_payload_bytes;
        if payload.len() > limit {
            return Err(WebhooksError::PayloadTooLarge {
                size: payload.len(),
                limit,
            });
        }
        Ok(())
    }
}

impl fmt::Debug for Webhooks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Webhooks")
            .field(
                "event_types",
                &self.inner.types.iter().map(|t| &t.name).collect::<Vec<_>>(),
            )
            .field("options", &self.inner.options)
            .finish_non_exhaustive()
    }
}

/// A webhook being dispatched by name ([`Webhooks::dispatch_json`]).
#[must_use = "nothing is sent until `send` is awaited"]
pub struct Dispatch<'a> {
    webhooks: &'a Webhooks,
    event_type: String,
    data: Value,
    workspace_id: Option<Uuid>,
}

impl Dispatch<'_> {
    /// Sends it to the endpoints of `workspace_id` instead of the app-wide
    /// ones.
    pub fn workspace(mut self, workspace_id: Uuid) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }

    /// Queues the fan-out job and returns the message id (`msg_...`).
    /// Inside a unit of work the job joins it, so nothing is sent if it
    /// rolls back. An unregistered type, or a payload over
    /// `webhooks.max_payload_bytes`, fails here.
    pub async fn send(self) -> Result<String, WebhooksError> {
        let webhooks = self.webhooks;
        if !webhooks.is_registered(&self.event_type) {
            return Err(WebhooksError::UnknownEventType(self.event_type));
        }
        let deps = webhooks.deps().await?;
        let timestamp = deps.now();
        webhooks.check_size(&render_body(&self.event_type, timestamp, &self.data)?)?;
        let job = FanOut {
            event_type: self.event_type,
            workspace_id: self.workspace_id,
            message_id: new_message_id(),
            timestamp,
            event: None,
            data: Some(self.data),
        };
        webhooks.dispatch_job(deps, &job, None).await?;
        Ok(job.message_id)
    }
}

impl fmt::Debug for Dispatch<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dispatch")
            .field("event_type", &self.event_type)
            .field("workspace_id", &self.workspace_id)
            .finish_non_exhaustive()
    }
}

/// `n` days (saturating).
fn days_of(n: u64) -> TimeDelta {
    i64::try_from(n)
        .ok()
        .and_then(TimeDelta::try_days)
        .unwrap_or(TimeDelta::MAX)
}

/// A new message id: `msg_` and a UUID v7 in simple form.
pub(crate) fn new_message_id() -> String {
    format!("msg_{}", Uuid::now_v7().simple())
}

/// The body of a delivery: `{"type", "timestamp", "data"}`.
pub(crate) fn render_body(
    event_type: &str,
    timestamp: DateTime<Utc>,
    data: &Value,
) -> Result<String, WebhooksError> {
    #[derive(Serialize)]
    struct Body<'a> {
        #[serde(rename = "type")]
        kind: &'a str,
        timestamp: String,
        data: &'a Value,
    }
    serde_json::to_string(&Body {
        kind: event_type,
        timestamp: timestamp.to_rfc3339_opts(SecondsFormat::Secs, true),
        data,
    })
    .map_err(|error| WebhooksError::Payload {
        event_type: event_type.to_owned(),
        source: error.into(),
    })
}

/// `column = workspace_id`, or `column IS NULL` for app-wide rows.
/// `endpoints` narrowed to the ones of workspace `workspace_id`, or to
/// the app-wide ones (no workspace) for `None`.
fn in_workspace(
    endpoints: Builder<endpoint::Model>,
    workspace_id: Option<Uuid>,
) -> Builder<endpoint::Model> {
    match workspace_id {
        Some(id) => endpoints.where_(endpoint::Model::WORKSPACE_ID.eq(id)),
        None => endpoints.where_null(endpoint::Model::WORKSPACE_ID),
    }
}

/// Writes the columns in which `new` differs from `old` (the same endpoint)
/// and records the change, without its secrets, for the audit trail.
async fn write_endpoint(
    uow: &Uow,
    old: &endpoint::Model,
    new: &endpoint::Model,
) -> Result<(), WebhooksError> {
    let mut sets = Vec::new();
    macro_rules! changed {
        ($($column:ident: $field:ident),* $(,)?) => {
            $(
                if old.$field != new.$field {
                    sets.push(endpoint::Model::$column.set(new.$field.clone()));
                }
            )*
        };
    }
    changed!(
        URL: url,
        DESCRIPTION: description,
        EVENTS: events,
        HEADERS: headers,
        SECRET: secret,
        PREVIOUS_SECRET: previous_secret,
        PREVIOUS_SECRET_EXPIRES_AT: previous_secret_expires_at,
        ENABLED: enabled,
        DISABLED_AT: disabled_at,
        DISABLED_REASON: disabled_reason,
        FAILING_SINCE: failing_since,
        UPDATED_AT: updated_at,
    );
    if sets.is_empty() {
        return Ok(());
    }
    // A set-based update: the ORM's change event would carry the secrets.
    let written = endpoint::Model::query()
        .where_key(old.id)
        .update(uow, sets)
        .await?;
    if written == 0 {
        return Err(WebhooksError::EndpointNotFound(old.id));
    }
    changes::updated(uow, old, new);
    Ok(())
}

/// Inserts the attempt row of `outcome` through `exec`.
async fn insert_attempt<E: Executor + ?Sized>(
    exec: &E,
    delivery: &delivery::Model,
    number: i32,
    outcome: &SendOutcome,
    now: DateTime<Utc>,
) -> Result<attempt::Model, WebhooksError> {
    let attempt = attempt::Model {
        id: Uuid::now_v7(),
        delivery_id: delivery.id,
        endpoint_id: delivery.endpoint_id,
        attempt: number,
        status_code: outcome.status_code.map(i32::from),
        succeeded: outcome.succeeded,
        response_body: outcome.response_body.clone(),
        error: outcome.error.clone(),
        duration_ms: i64::try_from(outcome.duration_ms).unwrap_or(i64::MAX),
        created_at: now,
    };
    attempt::Model::insert(exec, [attempt.clone()]).await?;
    Ok(attempt)
}

/// What an attempt that got no response says about it.
fn describe_send_error(error: &HttpClientError) -> String {
    match error {
        HttpClientError::Timeout { .. } => "the endpoint did not answer in time".to_owned(),
        HttpClientError::Blocked { reason, .. } => {
            format!("refused by the SSRF policy: {reason}")
        }
        other => other.to_string(),
    }
}

/// At most `limit` bytes of `text`, cut at a character boundary.
fn truncate(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

/// Checks an endpoint's extra headers.
fn check_headers(
    headers: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, WebhooksError> {
    if headers.len() > MAX_HEADERS {
        return Err(WebhooksError::invalid(
            "headers",
            format!("headers must have at most {MAX_HEADERS} entries"),
        ));
    }
    let mut checked = BTreeMap::new();
    for (name, value) in headers {
        let parsed = HeaderName::from_bytes(name.trim().as_bytes()).map_err(|_| {
            WebhooksError::invalid(
                "headers",
                format!("headers: `{name}` is not a valid header name"),
            )
        })?;
        let lower = parsed.as_str();
        if RESERVED_HEADERS.contains(&lower) || lower.starts_with("webhook-") {
            return Err(WebhooksError::invalid(
                "headers",
                format!("headers: `{name}` is set by the webhooks module"),
            ));
        }
        HeaderValue::from_str(&value).map_err(|_| {
            WebhooksError::invalid(
                "headers",
                format!("headers: the value of `{name}` is not valid"),
            )
        })?;
        checked.insert(lower.to_owned(), value);
    }
    Ok(checked)
}

/// Checks a description (an empty one is none).
fn check_description(description: Option<String>) -> Result<Option<String>, WebhooksError> {
    let Some(description) = description.map(|d| d.trim().to_owned()) else {
        return Ok(None);
    };
    if description.is_empty() {
        return Ok(None);
    }
    if description.chars().count() > MAX_DESCRIPTION_LEN {
        return Err(WebhooksError::invalid(
            "description",
            format!("description must be at most {MAX_DESCRIPTION_LEN} characters"),
        ));
    }
    Ok(Some(description))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_carry_type_timestamp_and_data_in_order() {
        let at = DateTime::from_timestamp(1_793_002_627, 0).unwrap();
        let body = render_body("deal.won", at, &json!({ "deal": { "id": 7 } })).unwrap();
        assert_eq!(
            body,
            r#"{"type":"deal.won","timestamp":"2026-10-26T08:17:07Z","data":{"deal":{"id":7}}}"#
        );
    }

    #[test]
    fn message_ids_are_msg_and_32_hex_digits() {
        let id = new_message_id();
        assert!(id.starts_with("msg_"));
        assert_eq!(id.len(), 36);
        assert!(id[4..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn response_bodies_are_cut_at_a_character_boundary() {
        assert_eq!(truncate("héllo".to_owned(), 2), "h");
        assert_eq!(truncate("hello".to_owned(), 10), "hello");
        assert_eq!(truncate("x".repeat(5000), 4096).len(), 4096);
    }

    #[test]
    fn headers_are_checked_and_lower_cased() {
        let ok = check_headers(BTreeMap::from([(
            "X-Team".to_owned(),
            "revenue".to_owned(),
        )]))
        .unwrap();
        assert_eq!(ok.get("x-team").map(String::as_str), Some("revenue"));
        for name in ["Content-Type", "webhook-signature", "Host", "bad name"] {
            assert!(
                check_headers(BTreeMap::from([(name.to_owned(), "v".to_owned())])).is_err(),
                "{name}"
            );
        }
        assert!(
            check_headers(BTreeMap::from([(
                "X-A".to_owned(),
                "line\nbreak".to_owned()
            )]))
            .is_err()
        );
    }

    #[test]
    fn descriptions_are_trimmed_and_limited() {
        assert_eq!(check_description(Some("  ".into())).unwrap(), None);
        assert_eq!(
            check_description(Some(" Revenue ".into()))
                .unwrap()
                .as_deref(),
            Some("Revenue")
        );
        assert!(check_description(Some("x".repeat(501))).is_err());
    }
}
