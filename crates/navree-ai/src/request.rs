//! The request builders: [`TextRequest`], [`StructuredRequest`] and
//! [`StreamRequest`]. Each takes the same options; awaiting it sends it.

use std::future::{Future, IntoFuture};
use std::marker::PhantomData;
use std::pin::Pin;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::ai::{Ai, Extra, Spec, extract_json};
use crate::message::{Attachment, Message, Prompt};
use crate::provider::OutputFormat;
use crate::response::{Finish, TextResponse};
use crate::schema;
use crate::stream::TextStream;
use crate::AiError;

/// The options every request builder takes.
macro_rules! request_options {
    () => {
        /// The system prompt: who the model is and the rules it follows.
        pub fn system(mut self, text: impl Into<String>) -> Self {
            self.spec.system = Some(text.into());
            self
        }

        /// A model name from `ai.models` (default `ai.default`).
        pub fn model(mut self, name: impl Into<String>) -> Self {
            self.spec.model = Some(name.into());
            self
        }

        /// The output limit of this request (default: the model's
        /// `max_tokens`).
        pub fn max_tokens(mut self, max_tokens: u32) -> Self {
            self.spec.max_tokens = Some(max_tokens);
            self
        }

        /// The sampling temperature (lower is more predictable).
        pub fn temperature(mut self, temperature: f32) -> Self {
            self.spec.temperature = Some(temperature);
            self
        }

        /// Stop sequences.
        pub fn stop<I, S>(mut self, sequences: I) -> Self
        where
            I: IntoIterator<Item = S>,
            S: Into<String>,
        {
            self.spec.stop = sequences.into_iter().map(Into::into).collect();
            self
        }

        /// Attaches an image or a document to the (last) user message.
        pub fn attach(mut self, attachment: Attachment) -> Self {
            self.spec.attachments.push(attachment);
            self
        }

        /// The user the request is for: it counts against their budget and
        /// labels the log line.
        pub fn user(mut self, user: impl Into<Option<Uuid>>) -> Self {
            self.spec.user = user.into();
            self
        }

        /// The attempt limit of this request (default: the model's).
        pub fn timeout(mut self, timeout: Duration) -> Self {
            self.spec.timeout = Some(timeout);
            self
        }
    };
}

/// A text request ([`Ai::text`]); awaiting it returns a [`TextResponse`].
#[must_use = "a request does nothing until it is awaited"]
pub struct TextRequest {
    ai: Ai,
    spec: Spec,
}

impl TextRequest {
    pub(crate) fn new(ai: Ai, prompt: Prompt) -> Self {
        Self {
            ai,
            spec: Spec::new(prompt),
        }
    }

    request_options!();
}

impl IntoFuture for TextRequest {
    type Output = Result<TextResponse, AiError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { self.ai.text_response(&self.spec, &Extra::default()).await })
    }
}

/// Parses an answer into `T`.
type Parser<T> = fn(&Value) -> Result<T, String>;

/// A structured request ([`Ai::structured`]); awaiting it returns a `T`.
///
/// The JSON Schema comes from `T`'s `ToSchema`; the provider's own
/// structured-output mode constrains the answer where it has one. An
/// answer that doesn't deserialize (or, with [`validated`](Self::validated),
/// breaks `T`'s garde rules) goes back to the model with the errors, up to
/// `ai.structured_retries` times, then fails with
/// [`AiError::InvalidOutput`].
#[must_use = "a request does nothing until it is awaited"]
pub struct StructuredRequest<T> {
    ai: Ai,
    spec: Spec,
    parse: Parser<T>,
    _type: PhantomData<fn() -> T>,
}

impl<T> StructuredRequest<T>
where
    T: DeserializeOwned + ToSchema,
{
    pub(crate) fn new(ai: Ai, prompt: Prompt) -> Self {
        Self {
            ai,
            spec: Spec::new(prompt),
            parse: plain::<T>,
            _type: PhantomData,
        }
    }

    request_options!();

    /// Also checks the value against `T`'s garde rules, the same rules as
    /// a validated request body.
    pub fn validated(mut self) -> Self
    where
        T: garde::Validate<Context = ()>,
    {
        self.parse = validated::<T>;
        self
    }
}

/// Deserializes `value`.
fn plain<T: DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

/// Deserializes and validates `value`, with the API's messages.
fn validated<T: DeserializeOwned + garde::Validate<Context = ()>>(value: &Value) -> Result<T, String> {
    navree_validation::validate_json::<T>(value, &navree_validation::Messages::default()).map_err(|errors| {
        errors
            .iter()
            .flat_map(|(_, messages)| messages.iter().cloned())
            .collect::<Vec<_>>()
            .join("; ")
    })
}

impl<T> IntoFuture for StructuredRequest<T>
where
    T: DeserializeOwned + ToSchema + Send + 'static,
{
    type Output = Result<T, AiError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let (schema, wrapped) = schema::wrap_non_object(schema::json_schema::<T>());
            let extra = Extra {
                output: Some(OutputFormat {
                    name: T::name().into_owned(),
                    schema,
                }),
                ..Extra::default()
            };
            structured(&self.ai, self.spec, &extra, wrapped, self.parse).await
        })
    }
}

/// Asks for structured output and repairs it: the shared loop of
/// [`StructuredRequest`] and agents' structured answers.
pub(crate) async fn structured<T>(
    ai: &Ai,
    mut spec: Spec,
    extra: &Extra,
    wrapped: bool,
    parse: Parser<T>,
) -> Result<T, AiError> {
    // The attachments belong to the question, not to a repair request.
    spec.messages = spec.messages_with_attachments();
    spec.attachments.clear();
    let mut last_error = String::new();
    for _ in 0..=ai.options().structured_retries {
        let answer = ai.complete(&spec, extra).await?;
        if answer.response.finish == Finish::Refused {
            return Err(AiError::Refused { model: answer.model });
        }
        let text = answer.response.text();
        let parsed = extract_json(&text).and_then(|value| {
            let value = if wrapped { schema::unwrap_value(value) } else { value };
            parse(&value)
        });
        match parsed {
            Ok(value) => return Ok(value),
            Err(error) => {
                tracing::debug!(target: "navree::ai", model = %answer.model, %error, "structured output needs repair");
                last_error = error;
                spec.messages.push(Message::assistant(text));
                spec.messages.push(Message::user(format!(
                    "Your answer does not match the required JSON schema: {last_error}. \
                     Answer again with only the corrected JSON."
                )));
            }
        }
    }
    Err(AiError::InvalidOutput { message: last_error })
}

/// A streamed request ([`Ai::stream`]); awaiting it returns a
/// [`TextStream`] as soon as the provider starts answering.
#[must_use = "a request does nothing until it is awaited"]
pub struct StreamRequest {
    ai: Ai,
    spec: Spec,
}

impl StreamRequest {
    pub(crate) fn new(ai: Ai, prompt: Prompt) -> Self {
        Self {
            ai,
            spec: Spec::new(prompt),
        }
    }

    request_options!();
}

impl IntoFuture for StreamRequest {
    type Output = Result<TextStream, AiError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let open = self.ai.open_stream(&self.spec, &Extra::default()).await?;
            Ok(TextStream::from_open(open, self.ai.options().log_content))
        })
    }
}
