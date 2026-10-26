//! [`TextResponse`], [`Usage`], [`Finish`] and [`Completion`].

use std::fmt;
use std::ops::{Add, AddAssign};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// The tokens a request used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Usage {
    /// Input tokens, cached ones included.
    pub input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
    /// The part of `input_tokens` read from the provider's prompt cache.
    #[serde(default)]
    pub cached_input_tokens: u64,
}

impl Usage {
    /// A usage of `input` and `output` tokens.
    pub fn new(input: u64, output: u64) -> Self {
        Self {
            input_tokens: input,
            output_tokens: output,
            cached_input_tokens: 0,
        }
    }

    /// Input plus output tokens (what budgets count).
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

impl Add for Usage {
    type Output = Usage;

    fn add(self, other: Usage) -> Usage {
        Usage {
            input_tokens: self.input_tokens + other.input_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
            cached_input_tokens: self.cached_input_tokens + other.cached_input_tokens,
        }
    }
}

impl AddAssign for Usage {
    fn add_assign(&mut self, other: Usage) {
        *self = *self + other;
    }
}

/// Why the model stopped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Finish {
    /// It finished its answer (or reached a stop sequence).
    #[default]
    Stop,
    /// It was cut off by the output limit: the text ends mid-thought.
    MaxTokens,
    /// It declined to answer.
    Refused,
    /// It asked to call tools (agents).
    ToolCalls,
}

impl Finish {
    /// `stop`, `max_tokens`, `refused` or `tool_calls`.
    pub fn as_str(self) -> &'static str {
        match self {
            Finish::Stop => "stop",
            Finish::MaxTokens => "max_tokens",
            Finish::Refused => "refused",
            Finish::ToolCalls => "tool_calls",
        }
    }
}

/// A text answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextResponse {
    /// The generated text.
    pub text: String,
    /// The model name that answered, after any failover (`fast`, `backup`).
    pub model: String,
    /// Its provider.
    pub provider: String,
    /// The tokens used.
    pub usage: Usage,
    /// Why the model stopped.
    pub finish: Finish,
}

impl fmt::Display for TextResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// The end of a stream: who answered, the tokens and why it stopped.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Completion {
    /// The model name that answered.
    pub model: String,
    /// Its provider.
    pub provider: String,
    /// The tokens of the whole answer (of the whole run, for agents).
    pub usage: Usage,
    /// Why the model stopped.
    pub finish: Finish,
    /// The agent's conversation, when the run has memory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_adds_up() {
        let mut usage = Usage::new(10, 2);
        usage += Usage {
            input_tokens: 5,
            output_tokens: 1,
            cached_input_tokens: 4,
        };
        assert_eq!(usage.total(), 18);
        assert_eq!(usage.cached_input_tokens, 4);
    }

    #[test]
    fn finish_serializes_in_snake_case() {
        assert_eq!(
            serde_json::to_value(Finish::MaxTokens).unwrap(),
            "max_tokens"
        );
        assert_eq!(Finish::Refused.as_str(), "refused");
    }

    #[test]
    fn text_response_displays_its_text() {
        let response = TextResponse {
            text: "Hi".into(),
            model: "fast".into(),
            provider: "anthropic".into(),
            usage: Usage::default(),
            finish: Finish::Stop,
        };
        assert_eq!(response.to_string(), "Hi");
    }
}
