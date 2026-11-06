//! [`MicroRuntime`]: the transport, the client, the message router and the
//! stores of one app, shared by the listener, the relay and the commands.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use navree_contracts::{HealthCheck, HealthStatus};
use navree_db::Db;
use navree_kernel::{AppContext, BoxError, BoxFuture, Lifecycle, WeakAppContext};
use navree_telemetry::{Telemetry, TracePolicy};
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::client::{ClientCore, Outbox};
use crate::options::{ClientOptions, DeadLetterMode};
use crate::pipeline::{MessageRouter, PipelineSettings};
use crate::store::Store;
use crate::transport::TransportKind;
use crate::{
    MicroError, MicroOptions, MicroTransport, ServiceClient, TcpTransport, Transport, listener,
};

/// Everything `MicroModule` builds for one app.
pub(crate) struct MicroRuntime {
    pub(crate) options: MicroOptions,
    pub(crate) service: String,
    pub(crate) instance: String,
    pub(crate) transport: Arc<dyn MicroTransport>,
    pub(crate) client: Arc<ServiceClient>,
    pub(crate) store: Option<Store>,
    pub(crate) dead_letter: DeadLetterMode,
    /// `observe.tracing`: how a handled message's trace starts.
    pub(crate) trace: TracePolicy,
    /// The app's telemetry, for the context handlers run in.
    pub(crate) telemetry: Telemetry,
    app: WeakAppContext,
    router: OnceCell<Arc<MessageRouter>>,
    listening: AtomicBool,
    /// True while a started listener that has subscriptions is not subscribed: the broker has
    /// not accepted them yet, or the listen failed and is waiting to start again.
    unsubscribed: AtomicBool,
}

impl MicroRuntime {
    /// Builds the runtime: resolves the service name and the transport,
    /// and finds the database (for the outbox, the inbox and dead letters).
    pub(crate) async fn new(ctx: AppContext, mut options: MicroOptions) -> Result<Self, BoxError> {
        options
            .check()
            .map_err(|errors| format!("invalid micro options: {}", errors.join("; ")))?;
        let service = resolve_service(&ctx, &options.service).await;
        options.service.clone_from(&service);
        let instance = format!("{service}-{}", &Uuid::new_v4().simple().to_string()[..8]);
        let transport = connect(
            &options.transport,
            options.transport_url(),
            &options,
            &service,
            &instance,
        )?;
        let db = ctx.try_get::<Db>().await;
        let store = db.map(|db| Store::new((*db).clone()));
        let dead_letter = options.events.dead_letter.unwrap_or(if store.is_some() {
            DeadLetterMode::Database
        } else {
            DeadLetterMode::Transport
        });
        if dead_letter == DeadLetterMode::Database && store.is_none() {
            return Err(
                "micro.events.dead_letter = \"database\" needs DatabaseModule in the app".into(),
            );
        }
        let outbox = store.clone().filter(|_| options.outbox).map(Outbox::new);
        let trace = trace_policy(&ctx).await?;
        let telemetry = Telemetry::from_app(&ctx).await;
        let client = Arc::new(ServiceClient::new(ClientCore {
            transport: transport.clone(),
            service: service.clone(),
            codec: options.codec,
            request_timeout: Duration::from_millis(options.request_timeout_ms),
            max_message_bytes: options.max_message_bytes,
            forward_auth: options.forward_auth,
            outbox,
            local: None,
            trace,
        }));
        Ok(Self {
            options,
            service,
            instance,
            transport,
            client,
            store,
            dead_letter,
            trace,
            telemetry,
            app: ctx.downgrade(),
            router: OnceCell::new(),
            listening: AtomicBool::new(false),
            unsubscribed: AtomicBool::new(false),
        })
    }

    /// The app, while it is alive.
    pub(crate) fn app(&self) -> Result<AppContext, MicroError> {
        self.app.upgrade().ok_or(MicroError::ShutDown)
    }

    /// The message router, built on first use (at boot, so mistakes fail
    /// it).
    pub(crate) async fn router(&self, ctx: &AppContext) -> Result<Arc<MessageRouter>, BoxError> {
        self.router
            .get_or_try_init(|| async {
                let settings = PipelineSettings {
                    service: self.service.clone(),
                    inbox: self.store.clone().filter(|_| self.options.inbox),
                    event_timeout: Duration::from_secs(self.options.events.timeout_secs),
                    wildcards: self.transport.supports_wildcards(),
                    transport: self.transport.name(),
                    trace: self.trace,
                    telemetry: self.telemetry.clone(),
                };
                MessageRouter::build(ctx, &settings).await.map(Arc::new)
            })
            .await
            .cloned()
    }

    /// A client for `micro.clients.<name>`.
    pub(crate) fn named_client(&self, name: &str) -> Result<ServiceClient, BoxError> {
        let client = self.options.clients.get(name).ok_or_else(|| {
            format!(
                "no client named `{name}`: add a [micro.clients.{name}] section with its transport"
            )
        })?;
        let merged = merge_client(&self.options, client);
        let transport = connect(
            &client.transport,
            client
                .transport
                .url()
                .map(str::to_owned)
                .or_else(|| client.url.clone()),
            &merged,
            &self.service,
            &self.instance,
        )?;
        Ok(ServiceClient::new(ClientCore {
            transport,
            service: self.service.clone(),
            codec: merged.codec,
            request_timeout: Duration::from_millis(merged.request_timeout_ms),
            max_message_bytes: merged.max_message_bytes,
            forward_auth: merged.forward_auth,
            outbox: None,
            local: None,
            trace: self.trace,
        }))
    }

    /// Starts the listener and the outbox relay in the background (once);
    /// they stop when the app shuts down. Returns false when they already
    /// run.
    pub(crate) async fn start(self: &Arc<Self>, ctx: &AppContext) -> Result<bool, BoxError> {
        if self.listening.swap(true, Ordering::SeqCst) {
            return Ok(false);
        }
        let router = self.router(ctx).await?;
        let token = ctx.shutdown_token();
        let runtime = self.clone();
        let task_ctx = ctx.clone();
        let stop = token.clone();
        let (started, is_started) = tokio::sync::oneshot::channel();
        ctx.tracker().spawn(async move {
            if let Err(error) = listener::run(runtime, task_ctx, router, stop, started).await {
                tracing::error!(target: "navree::micro", %error, "the message listener stopped");
            }
        });
        // Subscribed (or done) before returning, so a message sent right
        // after boot finds its handler. A boot that is stopped does not wait for it.
        tokio::select! {
            _ = is_started => {}
            () = token.cancelled() => {}
        }
        self.start_relay(ctx, token);
        Ok(true)
    }

    /// The outbox relay and the inbox pruning, with a database.
    fn start_relay(self: &Arc<Self>, ctx: &AppContext, token: CancellationToken) {
        if self.store.is_none() {
            return;
        }
        let runtime = self.clone();
        ctx.tracker().spawn(async move {
            listener::relay(runtime, token).await;
        });
    }

    /// Says whether the listener has its subscriptions (see `unsubscribed`).
    pub(crate) fn set_subscribed(&self, subscribed: bool) {
        self.unsubscribed.store(!subscribed, Ordering::SeqCst);
    }

    /// The `micro` readiness check.
    pub(crate) fn health(self: &Arc<Self>) -> Arc<dyn HealthCheck> {
        Arc::new(MicroHealth(self.clone()))
    }
}

impl std::fmt::Debug for MicroRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MicroRuntime")
            .field("service", &self.service)
            .field("instance", &self.instance)
            .field("transport", &self.transport.name())
            .finish_non_exhaustive()
    }
}

/// `observe.tracing.enabled`, `trust_incoming` and `sample_ratio` (the
/// ratio falls back to `OTEL_TRACES_SAMPLER_ARG`), as navree-http reads
/// them; the defaults without configuration.
async fn trace_policy(ctx: &AppContext) -> Result<TracePolicy, BoxError> {
    match ctx.try_get::<navree_config::Config>().await {
        Some(config) => Ok(TracePolicy::from_settings(
            |key| config.get_string(key),
            |name| std::env::var(name).ok(),
        )?),
        None => Ok(TracePolicy::default()),
    }
}

/// `micro.service`, else `app.name`, else the binary's name.
async fn resolve_service(ctx: &AppContext, configured: &str) -> String {
    if !configured.is_empty() {
        return configured.to_owned();
    }
    if let Some(config) = ctx.try_get::<navree_config::Config>().await
        && let Some(name) = config.app_name()
    {
        let name = sanitize(&name);
        if !name.is_empty() {
            return name;
        }
    }
    let binary = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let name = sanitize(&binary);
    if name.is_empty() {
        "app".to_owned()
    } else {
        name
    }
}

/// A service name from free text: letters, digits, `_` and `-`.
fn sanitize(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

/// The app's options with a named client's keys on top.
fn merge_client(options: &MicroOptions, client: &ClientOptions) -> MicroOptions {
    let mut merged = options.clone();
    merged.transport = client.transport.clone();
    merged.url.clone_from(&client.url);
    if let Some(codec) = client.codec {
        merged.codec = codec;
    }
    if let Some(timeout) = client.request_timeout_ms {
        merged.request_timeout_ms = timeout;
    }
    merged.tcp = client.tcp.clone().unwrap_or_default();
    if let Some(nats) = &client.nats {
        merged.nats = nats.clone();
    }
    if let Some(kafka) = &client.kafka {
        merged.kafka = kafka.clone();
    }
    if let Some(rabbitmq) = &client.rabbitmq {
        merged.rabbitmq = rabbitmq.clone();
    }
    if let Some(redis) = &client.redis {
        merged.redis = redis.clone();
    }
    if let Some(mqtt) = &client.mqtt {
        merged.mqtt = mqtt.clone();
    }
    merged
}

/// The transport instance for `transport`.
pub(crate) fn connect(
    transport: &Transport,
    url: Option<String>,
    options: &MicroOptions,
    service: &str,
    instance: &str,
) -> Result<Arc<dyn MicroTransport>, BoxError> {
    if let Some(instance) = transport.instance() {
        return Ok(instance.clone());
    }
    let connect_timeout = Duration::from_millis(options.connect_timeout_ms);
    let kind = transport
        .kind()
        .ok_or("micro.transport is required: tcp, nats, kafka, rabbitmq, redis, mqtt or memory")?;
    let _ = (service, instance);
    match kind {
        TransportKind::Tcp => {
            let mut tcp = options.tcp.clone();
            // `Transport::tcp(addr)` is where this service listens; a
            // configured `micro.url` is the peer of unrouted patterns.
            let peer = if let Some(addr) = transport.url() {
                tcp.addr = addr.to_owned();
                options.url.clone()
            } else {
                url
            };
            Ok(Arc::new(TcpTransport::new(
                &tcp,
                peer,
                options.max_message_bytes,
                connect_timeout,
            )?))
        }
        #[cfg(feature = "nats")]
        TransportKind::Nats => Ok(Arc::new(crate::transport::nats::NatsTransport::new(
            url.unwrap_or_else(|| "nats://127.0.0.1:4222".to_owned()),
            &options.nats,
            service,
            instance,
            connect_timeout,
        )?)),
        #[cfg(feature = "redis")]
        TransportKind::Redis => Ok(Arc::new(crate::transport::redis::RedisTransport::new(
            url.unwrap_or_else(|| "redis://127.0.0.1:6379".to_owned()),
            &options.redis,
            service,
            instance,
            connect_timeout,
        )?)),
        #[cfg(feature = "rabbitmq")]
        TransportKind::Rabbitmq => Ok(Arc::new(
            crate::transport::rabbitmq::RabbitmqTransport::new(
                url.unwrap_or_else(|| "amqp://guest:guest@127.0.0.1:5672/%2f".to_owned()),
                &options.rabbitmq,
                service,
                instance,
                connect_timeout,
            )?,
        )),
        #[cfg(feature = "mqtt")]
        TransportKind::Mqtt => Ok(Arc::new(crate::transport::mqtt::MqttTransport::new(
            url.unwrap_or_else(|| "mqtt://127.0.0.1:1883".to_owned()),
            &options.mqtt,
            service,
            instance,
            connect_timeout,
        )?)),
        #[cfg(feature = "kafka")]
        TransportKind::Kafka => {
            let kafka = &options.kafka;
            let mut transport = crate::transport::kafka::KafkaTransport::new(
                url.unwrap_or_else(|| "127.0.0.1:9092".to_owned()),
                kafka,
                service,
                instance,
                connect_timeout,
            )?;
            if kafka.tls_files.ca.is_some() || kafka.tls_files.cert.is_some() {
                transport = transport.with_tls_files(&kafka.tls_files)?;
            }
            Ok(Arc::new(transport))
        }
        TransportKind::Memory => Ok(Arc::new(crate::MemoryTransport::new())),
        other => Err(format!(
            "the {other} transport needs the `{}` feature of navree",
            other.feature()
        )
        .into()),
    }
}

/// Starts the listener with `serve` (or at boot for the in-memory
/// transport), closes the transport at shutdown.
pub(crate) struct Starter(pub(crate) Arc<MicroRuntime>);

impl Lifecycle for Starter {
    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            let runtime = &self.0;
            // Mistakes in the handlers fail the boot.
            runtime.router(&ctx).await?;
            if runtime.transport.listens_at_boot() {
                runtime.start(&ctx).await?;
                return Ok(());
            }
            if let Some(server) = ctx.try_get::<navree_http::HttpServer>().await {
                let runtime = runtime.clone();
                let token = ctx.shutdown_token();
                let task_ctx = ctx.clone();
                ctx.tracker().spawn(async move {
                    tokio::select! {
                        _ = server.wait_bound() => {
                            let started = if runtime.options.listen_with_serve {
                                runtime.start(&task_ctx).await.map(|_| ())
                            } else {
                                runtime.start_relay(&task_ctx, task_ctx.shutdown_token());
                                Ok(())
                            };
                            if let Err(error) = started {
                                tracing::error!(target: "navree::micro", %error, "the message listener did not start");
                            }
                        }
                        () = token.cancelled() => {}
                    }
                });
            }
            Ok(())
        })
    }

    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.0.transport.close().await;
            Ok(())
        })
    }
}

/// The `micro` readiness check: down while the transport is disconnected, and while a started
/// listener has not got its subscriptions.
struct MicroHealth(Arc<MicroRuntime>);

impl HealthCheck for MicroHealth {
    fn name(&self) -> &str {
        "micro"
    }

    fn check(&self) -> BoxFuture<'_, HealthStatus> {
        Box::pin(async move {
            let transport = &self.0.transport;
            if !transport.is_connected() {
                HealthStatus::Down(format!(
                    "the {} transport is disconnected",
                    transport.name()
                ))
            } else if self.0.unsubscribed.load(Ordering::SeqCst) {
                HealthStatus::Down(format!(
                    "the {} listener is not subscribed to its messages",
                    transport.name()
                ))
            } else {
                HealthStatus::Up
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_names_are_sanitized() {
        assert_eq!(sanitize("CRM API"), "CRM-API");
        assert_eq!(sanitize(" deals "), "deals");
        assert_eq!(sanitize("..."), "");
    }
}
