//! [`Ai::rerank`]: candidates re-ordered by how well they answer a
//! question, scored by a model in one call.

use std::future::{Future, IntoFuture};
use std::pin::Pin;

use serde::Deserialize;
use utoipa::ToSchema;

use crate::ai::{Ai, Extra, Spec};
use crate::error::AiError;
use crate::message::Prompt;
use crate::provider::OutputFormat;
use crate::request::structured;
use crate::schema;
use crate::vectors::VectorMatch;

/// The longest passage the reranker reads, in characters.
const PASSAGE_CHARS: usize = 2000;

impl Ai {
    /// Re-orders `matches` by how well each answers `query`: one call to
    /// the model, which scores every candidate from 0 to 1; returns the
    /// best (`.top(n)`, default all) with `score` replaced by the
    /// reranker's. With the fake, the order and scores are kept and no
    /// model is called.
    pub fn rerank(&self, query: impl AsRef<str>, matches: Vec<VectorMatch>) -> RerankRequest {
        RerankRequest {
            ai: self.clone(),
            query: query.as_ref().to_owned(),
            matches,
            model: None,
            top: None,
            user: None,
        }
    }
}

/// A rerank being built ([`Ai::rerank`]); awaiting it returns the matches,
/// best first.
#[must_use = "a rerank does nothing until it is awaited"]
pub struct RerankRequest {
    ai: Ai,
    query: String,
    matches: Vec<VectorMatch>,
    model: Option<String>,
    top: Option<usize>,
    user: Option<uuid::Uuid>,
}

impl RerankRequest {
    /// The model that scores (default `ai.default`).
    pub fn model(mut self, name: impl Into<String>) -> Self {
        self.model = Some(name.into());
        self
    }

    /// Keeps the best `n`.
    pub fn top(mut self, n: usize) -> Self {
        self.top = Some(n);
        self
    }

    /// The user it is for (their budget, the log line).
    pub fn user(mut self, user: impl Into<Option<uuid::Uuid>>) -> Self {
        self.user = user.into();
        self
    }
}

/// The reranker's answer.
#[derive(Debug, Deserialize, ToSchema)]
struct Scores {
    /// One score per passage.
    scores: Vec<Score>,
}

/// A passage's score.
#[derive(Debug, Deserialize, ToSchema)]
struct Score {
    /// The passage number, as given.
    index: usize,
    /// How well it answers the question, from 0 (not at all) to 1 (fully).
    score: f32,
}

impl IntoFuture for RerankRequest {
    type Output = Result<Vec<VectorMatch>, AiError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let top = self.top.unwrap_or(self.matches.len());
            let mut matches = self.matches;
            if matches.is_empty() || top == 0 {
                return Ok(Vec::new());
            }
            if self.ai.is_fake() {
                matches.truncate(top);
                return Ok(matches);
            }
            let passages = matches
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    format!(
                        "[{}] {}",
                        i + 1,
                        m.text.chars().take(PASSAGE_CHARS).collect::<String>()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            let mut spec = Spec::new(Prompt::from(format!(
                "Question: {}\n\nPassages:\n{passages}",
                self.query
            )));
            spec.system = Some(
                "You rank passages by how well they answer a question. Score every passage \
                 from 0 (irrelevant) to 1 (answers it fully), by its number."
                    .to_owned(),
            );
            spec.model = self.model;
            spec.user = self.user;
            let extra = Extra {
                output: Some(OutputFormat {
                    name: "Scores".to_owned(),
                    schema: schema::json_schema::<Scores>(),
                }),
                ..Extra::default()
            };
            let scores: Scores = structured(&self.ai, spec, &extra, false, |value| {
                serde_json::from_value(value.clone()).map_err(|error| error.to_string())
            })
            .await?;
            let mut scored: Vec<(f32, usize)> = (0..matches.len()).map(|i| (0.0, i)).collect();
            for Score { index, score } in scores.scores {
                if let Some(slot) = index.checked_sub(1).and_then(|i| scored.get_mut(i)) {
                    slot.0 = score.clamp(0.0, 1.0);
                }
            }
            scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let mut slots: Vec<Option<VectorMatch>> = matches.drain(..).map(Some).collect();
            Ok(scored
                .into_iter()
                .take(top)
                .filter_map(|(score, i)| {
                    slots[i].take().map(|mut m| {
                        m.score = score;
                        m
                    })
                })
                .collect())
        })
    }
}
