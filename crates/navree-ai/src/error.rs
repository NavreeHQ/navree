//! [`AiError`] and its HTTP mapping, and the drivers' [`ProviderError`].

use std::time::Duration;

use http::StatusCode;
use navree_http::AppError;

/// Every failure of the AI SDK. Returned from handlers with `?`, it becomes
/// an [`AppError`] in Navree's JSON error shape; the provider's own error
/// text stays in the log and never reaches the client.
///
/// | Error | Status |
/// |---|---|
/// | `Unavailable` | 503, "The AI provider is unavailable" |
/// | `RateLimited` | 429, with `Retry-After` |
/// | `BudgetExceeded` | 429, with `Retry-After` (midnight UTC) |
/// | `InvalidOutput` | 502 |
/// | `Refused` | 502 |
/// | `NotFound` | 404 |
/// | `Unauthenticated` | 401 |
/// | `Request`, `Unsupported`, `Config`, `NoFakeReply`, `Internal` | 500 |
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AiError {
    /// 5xx answers, timeouts and connection failures outlasted the retries
    /// and fallbacks.
    #[error("the AI provider is unavailable ({provider}): {message}")]
    Unavailable {
        /// The provider of the last model tried.
        provider: String,
        /// What went wrong, for the log.
        message: String,
    },
    /// The provider's rate limit outlasted the retries and fallbacks.
    #[error("the AI provider {provider} is rate limiting requests")]
    RateLimited {
        /// The provider of the last model tried.
        provider: String,
        /// The provider's `retry-after`, when it sent one.
        retry_after: Option<Duration>,
    },
    /// The user's daily token budget is spent.
    #[error("the daily AI token budget of the user is spent")]
    BudgetExceeded {
        /// When the budget renews (midnight UTC).
        retry_after: Duration,
    },
    /// Structured output still didn't match its type after the repairs.
    #[error("the model's answer does not match the expected type: {message}")]
    InvalidOutput {
        /// The last deserialization or validation error.
        message: String,
    },
    /// The model declined to answer a structured request.
    #[error("the model {model} declined to answer")]
    Refused {
        /// The model name that declined.
        model: String,
    },
    /// The provider rejected the request as invalid (a prompt longer than
    /// the context window, an unreadable image).
    #[error("the AI provider {provider} rejected the request: {message}")]
    Request {
        /// The provider.
        provider: String,
        /// The provider's message.
        message: String,
    },
    /// No model in the chain supports an attachment or a feature.
    #[error("unsupported: {message}")]
    Unsupported {
        /// What is not supported, by which models.
        message: String,
    },
    /// An unknown model, a provider without credentials, or a rejected key.
    #[error("AI configuration error: {message}")]
    Config {
        /// What is wrong, naming the key or the provider.
        message: String,
    },
    /// An unknown conversation (or one of another user or agent), or an
    /// unknown or already decided tool call.
    #[error("not found: {message}")]
    NotFound {
        /// What was not found.
        message: String,
    },
    /// An agent run without a signed-in user.
    #[error("an agent run needs a signed-in user")]
    Unauthenticated,
    /// [`AiFake`](crate::AiFake) has no reply for a request.
    #[error("AiFake has no reply for the request; prompt: {prompt}")]
    NoFakeReply {
        /// The request's last user message.
        prompt: String,
    },
    /// Anything else (a database error in conversation memory, a failed
    /// vector store call).
    #[error("AI internal error: {message}")]
    Internal {
        /// What went wrong.
        message: String,
    },
}

impl AiError {
    /// [`AiError::Unavailable`] for `provider`, as a failure of every model
    /// would report it (handy with [`FakeReply::error`](crate::FakeReply::error)).
    pub fn unavailable(provider: impl Into<String>) -> Self {
        AiError::Unavailable {
            provider: provider.into(),
            message: "the provider is unavailable".to_owned(),
        }
    }

    /// [`AiError::RateLimited`] for `provider`.
    pub fn rate_limited(provider: impl Into<String>, retry_after: Option<Duration>) -> Self {
        AiError::RateLimited {
            provider: provider.into(),
            retry_after,
        }
    }

    /// [`AiError::Request`] for `provider`.
    pub fn request(provider: impl Into<String>, message: impl Into<String>) -> Self {
        AiError::Request {
            provider: provider.into(),
            message: message.into(),
        }
    }

    /// [`AiError::Config`].
    pub fn config(message: impl Into<String>) -> Self {
        AiError::Config {
            message: message.into(),
        }
    }

    /// [`AiError::Internal`].
    pub fn internal(message: impl Into<String>) -> Self {
        AiError::Internal {
            message: message.into(),
        }
    }

    /// [`AiError::NotFound`].
    pub fn not_found(message: impl Into<String>) -> Self {
        AiError::NotFound {
            message: message.into(),
        }
    }

    /// The HTTP status this error is answered with.
    pub fn status(&self) -> StatusCode {
        match self {
            AiError::Unavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            AiError::RateLimited { .. } | AiError::BudgetExceeded { .. } => {
                StatusCode::TOO_MANY_REQUESTS
            }
            AiError::InvalidOutput { .. } | AiError::Refused { .. } => StatusCode::BAD_GATEWAY,
            AiError::NotFound { .. } => StatusCode::NOT_FOUND,
            AiError::Unauthenticated => StatusCode::UNAUTHORIZED,
            AiError::Request { .. }
            | AiError::Unsupported { .. }
            | AiError::Config { .. }
            | AiError::NoFakeReply { .. }
            | AiError::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The error response.
    pub fn to_app_error(&self) -> AppError {
        match self {
            AiError::Unavailable { .. } => {
                AppError::service_unavailable(UNAVAILABLE_MESSAGE).with_source(self.clone())
            }
            AiError::RateLimited { retry_after, .. } => {
                AppError::too_many_requests(retry_after.unwrap_or(Duration::from_secs(1)))
                    .with_detail("The AI provider is busy; try again shortly.")
            }
            AiError::BudgetExceeded { retry_after } => {
                AppError::too_many_requests(*retry_after)
                    .with_detail("The daily AI usage limit is reached; it renews at midnight UTC.")
            }
            AiError::InvalidOutput { .. } => AppError::from_status(StatusCode::BAD_GATEWAY)
                .with_detail("The AI model returned an answer in the wrong format.")
                .with_source(self.clone()),
            AiError::Refused { .. } => AppError::from_status(StatusCode::BAD_GATEWAY)
                .with_detail("The AI model declined to answer."),
            AiError::NotFound { .. } => AppError::not_found(),
            AiError::Unauthenticated => AppError::unauthenticated(),
            _ => AppError::internal(self.clone()),
        }
    }

    /// Whether trying again (the same model later, or another model) can
    /// help.
    pub(crate) fn is_transient(&self) -> bool {
        matches!(
            self,
            AiError::Unavailable { .. } | AiError::RateLimited { .. }
        )
    }
}

/// The message of a 503 [`AiError::Unavailable`].
pub(crate) const UNAVAILABLE_MESSAGE: &str = "The AI provider is unavailable";

/// The error mapping `AiModule` registers: `?` turns an `AiError` into a
/// 500 `AppError` carrying it, and the response pipeline answers with its
/// own status (a 500 stays a 500 and is logged).
pub(crate) fn map_ai_error(error: &AiError) -> Option<AppError> {
    if error.status() == StatusCode::INTERNAL_SERVER_ERROR {
        None
    } else {
        Some(error.to_app_error())
    }
}

/// A failure of one attempt with one provider, classified for failover.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ProviderError {
    /// 429: retried after `retry_after` (or the backoff), then fails over.
    RateLimited { retry_after: Option<Duration> },
    /// 5xx, a timeout, a connection failure or an unreadable answer: retried,
    /// then fails over.
    Unavailable { message: String },
    /// The request is invalid (4xx): fails at once.
    Request { message: String },
    /// The key is missing or rejected (401, 403): fails at once.
    Auth { message: String },
    /// The provider can't do this (an attachment type, embeddings): the
    /// model is skipped.
    Unsupported { message: String },
    /// A scripted failure of [`AiFake`](crate::AiFake), replayed as is.
    Scripted(AiError),
}

impl ProviderError {
    /// [`ProviderError::Unavailable`].
    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        ProviderError::Unavailable {
            message: message.into(),
        }
    }

    /// [`ProviderError::Request`].
    pub(crate) fn request(message: impl Into<String>) -> Self {
        ProviderError::Request {
            message: message.into(),
        }
    }

    /// [`ProviderError::Unsupported`].
    pub(crate) fn unsupported(message: impl Into<String>) -> Self {
        ProviderError::Unsupported {
            message: message.into(),
        }
    }

    /// Whether another attempt can help.
    pub(crate) fn is_transient(&self) -> bool {
        match self {
            ProviderError::RateLimited { .. } | ProviderError::Unavailable { .. } => true,
            ProviderError::Scripted(error) => error.is_transient(),
            _ => false,
        }
    }

    /// The provider's `retry-after`, if any.
    pub(crate) fn retry_after(&self) -> Option<Duration> {
        match self {
            ProviderError::RateLimited { retry_after } => *retry_after,
            ProviderError::Scripted(AiError::RateLimited { retry_after, .. }) => *retry_after,
            _ => None,
        }
    }

    /// Whether this is a rate limit.
    pub(crate) fn is_rate_limit(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimited { .. } | ProviderError::Scripted(AiError::RateLimited { .. })
        )
    }

    /// The [`AiError`] of a failure that ends the request at once (not
    /// transient), for `provider`.
    pub(crate) fn into_ai_error(self, provider: &str) -> AiError {
        match self {
            ProviderError::RateLimited { retry_after } => AiError::RateLimited {
                provider: provider.to_owned(),
                retry_after,
            },
            ProviderError::Unavailable { message } => AiError::Unavailable {
                provider: provider.to_owned(),
                message,
            },
            ProviderError::Request { message } => AiError::Request {
                provider: provider.to_owned(),
                message,
            },
            ProviderError::Auth { message } => AiError::Config {
                message: format!("provider `{provider}`: {message}"),
            },
            ProviderError::Unsupported { message } => AiError::Unsupported { message },
            ProviderError::Scripted(error) => error,
        }
    }

    /// A short description for the log.
    pub(crate) fn describe(&self) -> String {
        match self {
            ProviderError::RateLimited { retry_after } => match retry_after {
                Some(after) => format!("rate limited (retry after {} ms)", after.as_millis()),
                None => "rate limited".to_owned(),
            },
            ProviderError::Unavailable { message }
            | ProviderError::Request { message }
            | ProviderError::Auth { message }
            | ProviderError::Unsupported { message } => message.clone(),
            ProviderError::Scripted(error) => error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_statuses() {
        let cases = [
            (AiError::unavailable("anthropic"), 503),
            (AiError::rate_limited("openai", None), 429),
            (
                AiError::BudgetExceeded {
                    retry_after: Duration::from_secs(60),
                },
                429,
            ),
            (
                AiError::InvalidOutput {
                    message: "x".into(),
                },
                502,
            ),
            (AiError::Refused { model: "x".into() }, 502),
            (AiError::request("openai", "too long"), 500),
            (
                AiError::Unsupported {
                    message: "pdf".into(),
                },
                500,
            ),
            (AiError::config("no key"), 500),
            (AiError::not_found("conversation"), 404),
            (AiError::Unauthenticated, 401),
            (
                AiError::NoFakeReply {
                    prompt: "hi".into(),
                },
                500,
            ),
            (AiError::internal("db"), 500),
        ];
        for (error, status) in cases {
            assert_eq!(error.status().as_u16(), status, "{error}");
            assert_eq!(error.to_app_error().status.as_u16(), status, "{error}");
            let mapped = map_ai_error(&error).map(|e| e.status.as_u16());
            assert_eq!(mapped, (status != 500).then_some(status), "{error}");
        }
    }

    #[test]
    fn unavailable_has_the_documented_body() {
        let body = AiError::unavailable("anthropic").to_app_error().body();
        assert_eq!(body["statusCode"], 503);
        assert_eq!(body["message"], "The AI provider is unavailable");
        assert_eq!(body["error"], "Service Unavailable");
    }

    #[test]
    fn rate_limits_carry_retry_after() {
        let error = AiError::BudgetExceeded {
            retry_after: Duration::from_secs(3600),
        }
        .to_app_error();
        assert_eq!(error.headers.get("retry-after").unwrap(), "3600");
        let error = AiError::rate_limited("openai", Some(Duration::from_secs(7))).to_app_error();
        assert_eq!(error.headers.get("retry-after").unwrap(), "7");
    }

    #[test]
    fn provider_text_never_reaches_the_client() {
        let body = AiError::request("openai", "secret prompt details").to_app_error().body();
        assert!(!body.to_string().contains("secret"), "{body}");
    }

    #[test]
    fn provider_errors_classify() {
        assert!(ProviderError::RateLimited { retry_after: None }.is_transient());
        assert!(ProviderError::unavailable("503").is_transient());
        assert!(!ProviderError::request("bad").is_transient());
        assert!(
            !ProviderError::Auth {
                message: "401".into()
            }
            .is_transient()
        );
        assert!(ProviderError::Scripted(AiError::unavailable("x")).is_transient());
        assert!(!ProviderError::Scripted(AiError::config("x")).is_transient());
        assert!(matches!(
            ProviderError::Auth {
                message: "invalid x-api-key".into()
            }
            .into_ai_error("anthropic"),
            AiError::Config { .. }
        ));
    }
}
