//! [`Ai`]: the service, and the engine behind every request (model chains,
//! retries, failover, budgets, metrics and the log line).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Days, Utc};
use futures_util::StreamExt;
use navree_cache::{Cache, Ttl};
use navree_contracts::{Clock, SystemClock};
use navree_http_client::{HttpClient, HttpClientOptions, SsrfPolicy};
use navree_kernel::{AppContext, BoxFuture, WeakAppContext};
use serde_json::Value;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::error::{AiError, ProviderError};
use crate::fake::AiFake;
use crate::message::{Attachment, AttachmentKind, Message, Part, Prompt, Role};
use crate::options::{AiOptions, ModelConfig};
use crate::provider::{
    self, ChatRequest, ChatResponse, ChunkStream, Driver, OutputFormat, ToolChoice, ToolDefinition,
};
use crate::request::{StreamRequest, StructuredRequest, TextRequest};
use crate::response::{TextResponse, Usage};

/// The marker of the schema instruction added to the system prompt for
/// providers without native structured output.
pub(crate) const SCHEMA_INSTRUCTION: &str = "Respond with only a JSON value";

/// The longest wait between two attempts.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Language models behind one service. Inject `Arc<Ai>`.
///
/// ```no_run
/// # async fn run(ai: std::sync::Arc<navree_ai::Ai>) -> Result<(), navree_ai::AiError> {
/// let summary = ai
///     .text("Summarize this deal's activity: ...")
///     .system("You write two-sentence updates for sales managers.")
///     .model("fast")
///     .await?;
/// println!("{} (by {} on {})", summary.text, summary.model, summary.provider);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Ai {
    pub(crate) inner: Arc<Inner>,
}

/// The shared state of an [`Ai`] and its clones.
pub(crate) struct Inner {
    pub(crate) options: AiOptions,
    drivers: BTreeMap<String, Arc<dyn Driver>>,
    fake: Option<AiFake>,
    fake_driver: Option<Arc<dyn Driver>>,
    app: Option<WeakAppContext>,
    cache: OnceCell<Cache>,
    clock: OnceCell<Arc<dyn Clock>>,
    pub(crate) http: HttpClient,
}

/// Everything a request asks, before a model is chosen.
#[derive(Clone, Debug, Default)]
pub(crate) struct Spec {
    pub(crate) messages: Vec<Message>,
    pub(crate) system: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) max_tokens: Option<u32>,
    pub(crate) temperature: Option<f32>,
    pub(crate) stop: Vec<String>,
    pub(crate) attachments: Vec<Attachment>,
    pub(crate) user: Option<Uuid>,
    pub(crate) timeout: Option<Duration>,
}

impl Spec {
    /// A spec for `prompt`.
    pub(crate) fn new(prompt: Prompt) -> Self {
        Self {
            messages: prompt.0,
            ..Self::default()
        }
    }

    /// The messages with the attachments put first in the last user
    /// message (a new user message when there is none).
    pub(crate) fn messages_with_attachments(&self) -> Vec<Message> {
        let mut messages = self.messages.clone();
        if self.attachments.is_empty() {
            return messages;
        }
        let attachments: Vec<Part> = self
            .attachments
            .iter()
            .cloned()
            .map(Part::Attachment)
            .collect();
        match messages.iter_mut().rev().find(|m| m.role == Role::User) {
            Some(message) => {
                let mut parts = attachments;
                parts.append(&mut message.parts);
                message.parts = parts;
            }
            None => messages.push(Message::new(Role::User, attachments)),
        }
        messages
    }
}

/// What a request adds for agents and structured output.
#[derive(Clone, Debug, Default)]
pub(crate) struct Extra {
    pub(crate) tools: Vec<ToolDefinition>,
    pub(crate) tool_choice: ToolChoice,
    pub(crate) output: Option<OutputFormat>,
    /// Appended to the system prompt.
    pub(crate) system_suffix: Option<String>,
}

/// The model a request went to.
#[derive(Clone)]
pub(crate) struct Resolved {
    pub(crate) name: String,
    pub(crate) provider: String,
    pub(crate) config: ModelConfig,
    pub(crate) driver: Arc<dyn Driver>,
}

/// A successful attempt.
pub(crate) struct Success<T> {
    pub(crate) value: T,
    pub(crate) model: Resolved,
    /// The model's position in the chain (0: no failover).
    pub(crate) position: usize,
    pub(crate) tried: Vec<String>,
    pub(crate) started: Instant,
}

/// An answer, with who gave it.
pub(crate) struct Answer {
    pub(crate) response: ChatResponse,
    pub(crate) model: String,
    pub(crate) provider: String,
}

/// An open stream, with who answers, and the bookkeeping to do at its end.
pub(crate) struct OpenStream {
    pub(crate) chunks: ChunkStream,
    pub(crate) model: String,
    pub(crate) provider: String,
    pub(crate) gap: Duration,
    pub(crate) finisher: Finisher,
}

/// Records usage, metrics and the log line once a stream ends.
pub(crate) struct Finisher {
    ai: Ai,
    user: Option<Uuid>,
    model: String,
    provider: String,
    position: usize,
    tried: Vec<String>,
    started: Instant,
}

impl Finisher {
    /// The stream finished with `usage`.
    pub(crate) async fn finish(self, usage: Usage) {
        self.ai.spend(self.user, usage).await;
        self.ai.record(
            &self.model,
            &self.provider,
            if self.position == 0 { "ok" } else { "failover" },
            Some(usage),
            self.user,
            &self.tried,
            self.started,
        );
    }
}

impl Ai {
    /// An `Ai` with `options` and no app (no shared cache: budgets and the
    /// embeddings cache live in this value's memory). Invalid options fail.
    pub fn new(options: AiOptions) -> Result<Ai, AiError> {
        Self::build(options, None)
    }

    /// The `Ai` of [`AiModule`](crate::AiModule).
    pub(crate) fn build(options: AiOptions, app: Option<WeakAppContext>) -> Result<Ai, AiError> {
        let errors = options.check();
        if !errors.is_empty() {
            return Err(AiError::config(errors.join("; ")));
        }
        if options.fake {
            return Ok(Self::with_fake(options, AiFake::new(), app));
        }
        let http = http_client(&options)?;
        let mut drivers = BTreeMap::new();
        let mut names: Vec<String> = options.providers.keys().cloned().collect();
        for model in options.models.values() {
            if !names.contains(&model.provider) {
                names.push(model.provider.clone());
            }
        }
        for name in names {
            let config = options.providers.get(&name).cloned().unwrap_or_default();
            if provider::missing_key(&name, &config) {
                tracing::warn!(
                    target: "navree::ai",
                    provider = %name,
                    "the AI provider `{name}` has no API key; its requests will fail \
                     (set APP_AI__PROVIDERS__{}__API_KEY)",
                    name.to_uppercase()
                );
            }
            drivers.insert(name.clone(), provider::build_driver(&name, &config, &http)?);
        }
        Ok(Ai {
            inner: Arc::new(Inner {
                options,
                drivers,
                fake: None,
                fake_driver: None,
                app,
                cache: OnceCell::new(),
                clock: OnceCell::new(),
                http,
            }),
        })
    }

    /// An `Ai` whose every model answers from `fake`.
    pub(crate) fn with_fake(options: AiOptions, fake: AiFake, app: Option<WeakAppContext>) -> Ai {
        let http = http_client(&options).unwrap_or_else(|_| HttpClient::fake().0);
        Ai {
            inner: Arc::new(Inner {
                options,
                drivers: BTreeMap::new(),
                fake_driver: Some(fake.driver()),
                fake: Some(fake),
                app,
                cache: OnceCell::new(),
                clock: OnceCell::new(),
                http,
            }),
        }
    }

    /// The options.
    pub fn options(&self) -> &AiOptions {
        &self.inner.options
    }

    /// The fake, when `ai.fake` is on (or this `Ai` came from
    /// [`AiFake::ai`]).
    pub fn fake(&self) -> Option<AiFake> {
        self.inner.fake.clone()
    }

    /// A text request; awaiting it returns a [`TextResponse`].
    pub fn text(&self, prompt: impl Into<Prompt>) -> TextRequest {
        TextRequest::new(self.clone(), prompt.into())
    }

    /// A structured request; awaiting it returns a `T`, deserialized from
    /// JSON that matches `T`'s schema.
    pub fn structured<T>(&self, prompt: impl Into<Prompt>) -> StructuredRequest<T>
    where
        T: serde::de::DeserializeOwned + navree_openapi::ToSchema,
    {
        StructuredRequest::new(self.clone(), prompt.into())
    }

    /// A streamed request; awaiting it returns a
    /// [`TextStream`](crate::TextStream) once the provider starts
    /// answering.
    pub fn stream(&self, prompt: impl Into<Prompt>) -> StreamRequest {
        StreamRequest::new(self.clone(), prompt.into())
    }

    /// The app, while it runs.
    pub(crate) fn app(&self) -> Option<AppContext> {
        self.inner.app.as_ref().and_then(WeakAppContext::upgrade)
    }

    /// The app's `dyn Clock`, or the system clock.
    pub(crate) async fn clock(&self) -> Arc<dyn Clock> {
        self.inner
            .clock
            .get_or_init(|| async {
                match self.app() {
                    Some(app) => match app.try_get::<dyn Clock>().await {
                        Some(clock) => clock,
                        None => Arc::new(SystemClock) as Arc<dyn Clock>,
                    },
                    None => Arc::new(SystemClock) as Arc<dyn Clock>,
                }
            })
            .await
            .clone()
    }

    /// The app's cache, or one in this value's memory.
    pub(crate) async fn cache(&self) -> Cache {
        self.inner
            .cache
            .get_or_init(|| async {
                match self.app() {
                    Some(app) => match app.try_get::<Cache>().await {
                        Some(cache) => (*cache).clone(),
                        None => Cache::memory(),
                    },
                    None => Cache::memory(),
                }
            })
            .await
            .clone()
    }

    /// The model `name` resolves to: a configured one, or with the fake any
    /// name.
    pub(crate) fn resolve(&self, name: &str) -> Result<Resolved, AiError> {
        let options = &self.inner.options;
        if let Some(driver) = &self.inner.fake_driver {
            let config = options
                .models
                .get(name)
                .cloned()
                .unwrap_or_else(|| ModelConfig::new("fake", name));
            return Ok(Resolved {
                name: name.to_owned(),
                provider: if config.provider.is_empty() {
                    "fake".to_owned()
                } else {
                    config.provider.clone()
                },
                config,
                driver: driver.clone(),
            });
        }
        let config =
            options.models.get(name).cloned().ok_or_else(|| {
                AiError::config(format!("unknown model `{name}` (not in ai.models)"))
            })?;
        let driver = self
            .inner
            .drivers
            .get(&config.provider)
            .cloned()
            .ok_or_else(|| AiError::config(format!("unknown provider `{}`", config.provider)))?;
        Ok(Resolved {
            name: name.to_owned(),
            provider: config.provider.clone(),
            config,
            driver,
        })
    }

    /// The model a request goes to first.
    pub(crate) fn first_model(&self, requested: Option<&str>) -> Result<String, AiError> {
        if let Some(name) = requested {
            return Ok(name.to_owned());
        }
        match self.inner.options.default_model() {
            Some(name) => Ok(name.to_owned()),
            None if self.inner.fake.is_some() => Ok("default".to_owned()),
            None => Err(AiError::config(
                "no model is configured (ai.models is empty)",
            )),
        }
    }

    /// The models a request tries, in order.
    pub(crate) fn chain(&self, first: &str) -> Vec<String> {
        self.inner.options.chain(first)
    }

    /// The provider request for `model`.
    pub(crate) fn chat_request(&self, model: &Resolved, spec: &Spec, extra: &Extra) -> ChatRequest {
        let options = &self.inner.options;
        let messages = spec.messages_with_attachments();
        let native = model.driver.capabilities().native_structured;
        let mut system = spec.system.clone();
        let mut add_system = |text: String| {
            system = Some(match system.take() {
                Some(existing) if !existing.is_empty() => format!("{existing}\n\n{text}"),
                _ => text,
            });
        };
        if let Some(suffix) = &extra.system_suffix {
            add_system(suffix.clone());
        }
        let mut output = extra.output.clone();
        if let Some(format) = &output
            && !native
        {
            add_system(format!(
                "{SCHEMA_INSTRUCTION} that matches this JSON Schema, without code fences or \
                 commentary:\n{}",
                format.schema
            ));
            output = None;
        }
        ChatRequest {
            model_name: model.name.clone(),
            model: model.config.model.clone(),
            system,
            messages,
            max_tokens: spec.max_tokens.unwrap_or(model.config.max_tokens),
            temperature: spec.temperature.or(model.config.temperature),
            stop: spec.stop.clone(),
            tools: extra.tools.clone(),
            tool_choice: extra.tool_choice,
            output,
            user: spec.user,
            timeout: spec.timeout.unwrap_or_else(|| {
                Duration::from_secs(model.config.timeout_secs.unwrap_or(options.timeout_secs))
            }),
        }
    }

    /// Why `model` can't take this request, if it can't.
    fn unsupported(&self, model: &Resolved, request: &ChatRequest) -> Option<String> {
        let capabilities = model.driver.capabilities();
        for attachment in request.messages.iter().flat_map(Message::attachments) {
            let supported = match attachment.kind() {
                AttachmentKind::Image => capabilities.images,
                AttachmentKind::Pdf => capabilities.pdfs,
                AttachmentKind::Other => false,
            };
            if !supported {
                return Some(format!(
                    "model `{}` ({}) can't read {} attachments",
                    model.name,
                    model.provider,
                    attachment.media_type()
                ));
            }
        }
        None
    }

    /// Runs `attempt` on each model of the chain, with retries, until one
    /// succeeds. Budgets are checked first; failures are logged and
    /// counted here, successes by the caller (once the tokens are known).
    pub(crate) async fn failover<T, F>(
        &self,
        spec: &Spec,
        extra: &Extra,
        attempt: F,
    ) -> Result<Success<T>, AiError>
    where
        F: Fn(Arc<dyn Driver>, ChatRequest) -> BoxFuture<'static, Result<T, ProviderError>>,
    {
        self.check_budget(spec.user).await?;
        let first = self.first_model(spec.model.as_deref())?;
        let started = Instant::now();
        let options = &self.inner.options;
        let mut tried = Vec::new();
        let mut last: Option<(Resolved, ProviderError)> = None;
        let mut unsupported: Option<String> = None;
        for (position, name) in self.chain(&first).iter().enumerate() {
            let model = match self.resolve(name) {
                Ok(model) => model,
                Err(error) => {
                    self.record(name, "", "error", None, spec.user, &tried, started);
                    return Err(error);
                }
            };
            let request = self.chat_request(&model, spec, extra);
            if let Some(reason) = self.unsupported(&model, &request) {
                unsupported = Some(reason);
                tried.push(model.name.clone());
                continue;
            }
            tried.push(model.name.clone());
            let mut retry = 0;
            loop {
                let timeout = request.timeout;
                let outcome =
                    tokio::time::timeout(timeout, attempt(model.driver.clone(), request.clone()))
                        .await
                        .unwrap_or_else(|_| {
                            Err(ProviderError::unavailable(format!(
                                "no answer within {} s",
                                timeout.as_secs_f64()
                            )))
                        });
                match outcome {
                    Ok(value) => {
                        return Ok(Success {
                            value,
                            model,
                            position,
                            tried,
                            started,
                        });
                    }
                    Err(ProviderError::Unsupported { message }) => {
                        unsupported = Some(message);
                        break;
                    }
                    Err(error) if error.is_transient() => {
                        tracing::warn!(
                            target: "navree::ai",
                            model = %model.name,
                            provider = %model.provider,
                            attempt = retry + 1,
                            error = %error.describe(),
                            "AI request attempt failed"
                        );
                        let wait = backoff(options.retry_base_ms, retry)
                            .max(error.retry_after().unwrap_or_default());
                        last = Some((model.clone(), error));
                        if retry >= options.max_retries || wait > timeout {
                            break;
                        }
                        if self.inner.fake.is_none() {
                            tokio::time::sleep(wait).await;
                        }
                        retry += 1;
                    }
                    Err(error) => {
                        let ai_error = error.clone().into_ai_error(&model.provider);
                        if matches!(ai_error, AiError::Config { .. }) {
                            tracing::error!(
                                target: "navree::ai",
                                model = %model.name,
                                provider = %model.provider,
                                error = %error.describe(),
                                "AI provider configuration error"
                            );
                        } else {
                            tracing::warn!(
                                target: "navree::ai",
                                model = %model.name,
                                provider = %model.provider,
                                error = %error.describe(),
                                "AI request failed"
                            );
                        }
                        self.record(
                            &model.name,
                            &model.provider,
                            "error",
                            None,
                            spec.user,
                            &tried,
                            started,
                        );
                        return Err(ai_error);
                    }
                }
            }
        }
        let error = match last {
            Some((model, error)) if error.is_rate_limit() => AiError::RateLimited {
                provider: model.provider.clone(),
                retry_after: error.retry_after(),
            },
            Some((model, ProviderError::Scripted(error))) => {
                self.record(
                    &model.name,
                    &model.provider,
                    "error",
                    None,
                    spec.user,
                    &tried,
                    started,
                );
                return Err(error);
            }
            Some((model, error)) => AiError::Unavailable {
                provider: model.provider.clone(),
                message: format!(
                    "every model failed (tried {}): {}",
                    tried.join(", "),
                    error.describe()
                ),
            },
            None => match unsupported {
                Some(message) => AiError::Unsupported { message },
                None => AiError::config("no model to try"),
            },
        };
        let (name, provider) = tried
            .last()
            .and_then(|name| self.resolve(name).ok())
            .map(|m| (m.name, m.provider))
            .unwrap_or_default();
        self.record(&name, &provider, "error", None, spec.user, &tried, started);
        Err(error)
    }

    /// One whole answer, with failover; usage is recorded.
    pub(crate) async fn complete(&self, spec: &Spec, extra: &Extra) -> Result<Answer, AiError> {
        let success = self
            .failover(spec, extra, |driver, request| {
                Box::pin(async move { driver.chat(&request).await })
            })
            .await?;
        let response = success.value;
        self.spend(spec.user, response.usage).await;
        self.record(
            &success.model.name,
            &success.model.provider,
            if success.position == 0 {
                "ok"
            } else {
                "failover"
            },
            Some(response.usage),
            spec.user,
            &success.tried,
            success.started,
        );
        if self.inner.options.log_content {
            tracing::debug!(
                target: "navree::ai",
                model = %success.model.name,
                prompt = ?spec.messages.last().map(Message::text),
                output = %response.text(),
                "AI content"
            );
        }
        Ok(Answer {
            response,
            model: success.model.name,
            provider: success.model.provider,
        })
    }

    /// A text answer.
    pub(crate) async fn text_response(
        &self,
        spec: &Spec,
        extra: &Extra,
    ) -> Result<TextResponse, AiError> {
        let answer = self.complete(spec, extra).await?;
        Ok(TextResponse {
            text: answer.response.text(),
            model: answer.model,
            provider: answer.provider,
            usage: answer.response.usage,
            finish: answer.response.finish,
        })
    }

    /// A streamed answer, failing over until the first chunk arrives.
    pub(crate) async fn open_stream(
        &self,
        spec: &Spec,
        extra: &Extra,
    ) -> Result<OpenStream, AiError> {
        let success = self
            .failover(spec, extra, |driver, request| {
                Box::pin(async move {
                    let mut chunks = driver.stream(&request).await?;
                    // The first chunk decides whether this model answers.
                    match chunks.next().await {
                        Some(Ok(first)) => {
                            let rest = futures_util::stream::iter([Ok(first)]).chain(chunks);
                            Ok(Box::pin(rest) as ChunkStream)
                        }
                        Some(Err(error)) => Err(error),
                        None => Err(ProviderError::unavailable("an empty stream")),
                    }
                })
            })
            .await?;
        let gap = spec.timeout.unwrap_or_else(|| {
            Duration::from_secs(
                success
                    .model
                    .config
                    .timeout_secs
                    .unwrap_or(self.inner.options.timeout_secs),
            )
        });
        Ok(OpenStream {
            chunks: success.value,
            model: success.model.name.clone(),
            provider: success.model.provider.clone(),
            gap,
            finisher: Finisher {
                ai: self.clone(),
                user: spec.user,
                model: success.model.name,
                provider: success.model.provider,
                position: success.position,
                tried: success.tried,
                started: success.started,
            },
        })
    }

    /// Fails with [`AiError::BudgetExceeded`] when `user` has spent their
    /// tokens for the UTC day.
    pub(crate) async fn check_budget(&self, user: Option<Uuid>) -> Result<(), AiError> {
        let limit = self.inner.options.budget.daily_tokens_per_user;
        let Some(user) = user.filter(|_| limit > 0) else {
            return Ok(());
        };
        let now = self.clock().await.now();
        let used = self
            .cache()
            .await
            .get::<i64>(&budget_key(user, now))
            .await
            .ok()
            .flatten()
            .unwrap_or(0);
        if used >= limit as i64 {
            return Err(AiError::BudgetExceeded {
                retry_after: until_midnight(now),
            });
        }
        Ok(())
    }

    /// Counts `usage` against `user`'s budget.
    pub(crate) async fn spend(&self, user: Option<Uuid>, usage: Usage) {
        if self.inner.options.budget.daily_tokens_per_user == 0 || usage.total() == 0 {
            return;
        }
        let Some(user) = user else { return };
        let now = self.clock().await.now();
        let key = budget_key(user, now);
        let cache = self.cache().await;
        // A new counter lives two days, past the end of its day.
        let created = cache
            .add(&key, &0i64, Ttl::from(Duration::from_secs(2 * 24 * 3600)))
            .await;
        let counted = cache.increment(&key, usage.total() as i64).await;
        if let Err(error) = created.and(counted.map(|_| true)) {
            tracing::warn!(target: "navree::ai", %error, "counting AI tokens against the budget failed");
        }
    }

    /// The tokens `user` has used today.
    pub async fn tokens_used_today(&self, user: Uuid) -> u64 {
        let now = self.clock().await.now();
        self.cache()
            .await
            .get::<i64>(&budget_key(user, now))
            .await
            .ok()
            .flatten()
            .unwrap_or(0)
            .max(0) as u64
    }

    /// The metrics and the log line of one request.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record(
        &self,
        model: &str,
        provider: &str,
        outcome: &'static str,
        usage: Option<Usage>,
        user: Option<Uuid>,
        tried: &[String],
        started: Instant,
    ) {
        let elapsed = started.elapsed();
        let labels = [
            ("model", model.to_owned()),
            ("provider", provider.to_owned()),
        ];
        let mut with_outcome = labels.to_vec();
        with_outcome.push(("outcome", outcome.to_owned()));
        metrics::counter!("ai_requests_total", &with_outcome).increment(1);
        metrics::histogram!("ai_request_duration_seconds", &labels).record(elapsed.as_secs_f64());
        if let Some(usage) = usage {
            for (kind, count) in [
                ("input", usage.input_tokens),
                ("output", usage.output_tokens),
                ("cached_input", usage.cached_input_tokens),
            ] {
                let mut with_type = labels.to_vec();
                with_type.push(("type", kind.to_owned()));
                metrics::counter!("ai_tokens_total", &with_type).increment(count);
            }
        }
        let usage = usage.unwrap_or_default();
        let request_id =
            navree_db::Uow::current().and_then(|uow| uow.request_id().map(str::to_owned));
        tracing::info!(
            target: "navree::ai",
            model,
            provider,
            user_id = user.map(tracing::field::display),
            input_tokens = usage.input_tokens,
            output_tokens = usage.output_tokens,
            cached_input_tokens = usage.cached_input_tokens,
            duration_ms = elapsed.as_millis() as u64,
            outcome,
            tried = %tried.join(","),
            request_id = request_id.as_deref(),
            "AI request"
        );
    }

    /// Whether answers come from the fake.
    pub(crate) fn is_fake(&self) -> bool {
        self.inner.fake.is_some()
    }
}

impl fmt::Debug for Ai {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ai")
            .field(
                "models",
                &self.inner.options.models.keys().collect::<Vec<_>>(),
            )
            .field("fake", &self.inner.fake.is_some())
            .finish_non_exhaustive()
    }
}

/// The HTTP client providers use: navree-http-client's stack with its SSRF
/// guard, which lets through the hosts configured in `ai.providers.*.url`
/// and `ai.vectors.url` (the operator chose them) and nothing else
/// internal. Retries are the engine's job, so the client makes none.
fn http_client(options: &AiOptions) -> Result<HttpClient, AiError> {
    let longest = options
        .models
        .values()
        .filter_map(|m| m.timeout_secs)
        .chain([options.timeout_secs])
        .max()
        .unwrap_or(60);
    HttpClient::new(HttpClientOptions {
        timeout_secs: longest.max(1),
        retries: 0,
        max_response_bytes: 64 * 1024 * 1024,
        ssrf: SsrfPolicy {
            block_private: true,
            allow_hosts: options.configured_hosts(),
        },
        user_agent: concat!("navree-ai/", env!("CARGO_PKG_VERSION")).to_owned(),
        ..HttpClientOptions::default()
    })
    .map_err(|error| AiError::config(format!("cannot build the AI HTTP client: {error}")))
}

/// Exponential backoff from `base_ms`.
fn backoff(base_ms: u64, retry: u32) -> Duration {
    let factor = 2u64.saturating_pow(retry);
    Duration::from_millis(base_ms.saturating_mul(factor)).min(MAX_BACKOFF)
}

fn budget_key(user: Uuid, now: DateTime<Utc>) -> String {
    format!("ai:budget:{user}:{}", now.format("%Y-%m-%d"))
}

/// The time to the next midnight UTC (at least a second).
pub(crate) fn until_midnight(now: DateTime<Utc>) -> Duration {
    let midnight = now
        .date_naive()
        .checked_add_days(Days::new(1))
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .map(|naive| naive.and_utc())
        .unwrap_or(now);
    (midnight - now)
        .to_std()
        .unwrap_or_default()
        .max(Duration::from_secs(1))
}

/// The JSON in a model's text: the whole text, or the inside of a code
/// fence, or from the first `{`/`[` to the last `}`/`]`.
pub(crate) fn extract_json(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }
    let unfenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|rest| rest.strip_suffix("```"))
        .map(str::trim);
    if let Some(inner) = unfenced
        && let Ok(value) = serde_json::from_str(inner)
    {
        return Ok(value);
    }
    let start = trimmed.find(['{', '[']);
    let end = trimmed.rfind(['}', ']']);
    if let (Some(start), Some(end)) = (start, end)
        && start < end
        && let Ok(value) = serde_json::from_str(&trimmed[start..=end])
    {
        return Ok(value);
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => Ok(value),
        Err(error) => Err(format!("the answer is not valid JSON ({error})")),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn backoff_doubles_from_the_base() {
        assert_eq!(backoff(500, 0), Duration::from_millis(500));
        assert_eq!(backoff(500, 1), Duration::from_millis(1000));
        assert_eq!(backoff(500, 2), Duration::from_millis(2000));
        assert_eq!(backoff(500, 40), MAX_BACKOFF);
    }

    #[test]
    fn midnight_utc() {
        let now = Utc.with_ymd_and_hms(2026, 10, 26, 23, 0, 0).unwrap();
        assert_eq!(until_midnight(now), Duration::from_secs(3600));
    }

    #[test]
    fn json_is_found_in_fences_and_prose() {
        assert_eq!(extract_json("{\"a\":1}").unwrap()["a"], 1);
        assert_eq!(extract_json("```json\n{\"a\":2}\n```").unwrap()["a"], 2);
        assert_eq!(
            extract_json("Here you go: {\"a\":3} Thanks").unwrap()["a"],
            3
        );
        assert!(extract_json("no json here").is_err());
    }
}
