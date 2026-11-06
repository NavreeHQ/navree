//! [`Embeddings`] and [`Embedding`]: vectors for texts.

use std::ops::Deref;
use std::time::{Duration, Instant};

use futures_util::future::try_join_all;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ai::{Ai, Extra, Spec};
use crate::error::AiError;
use crate::provider::EmbedRequest;

/// A vector computed from a text. Derefs to `[f32]`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Embedding(Vec<f32>);

impl Embedding {
    /// An embedding from its values.
    pub fn new(values: Vec<f32>) -> Self {
        Self(values)
    }

    /// The vector size.
    pub fn dimensions(&self) -> usize {
        self.0.len()
    }

    /// The cosine similarity with `other` (from -1 to 1; 0 when either is
    /// all zeros or the sizes differ).
    pub fn cosine(&self, other: &Embedding) -> f32 {
        cosine(&self.0, &other.0)
    }

    /// The values.
    pub fn into_vec(self) -> Vec<f32> {
        self.0
    }
}

impl Deref for Embedding {
    type Target = [f32];

    fn deref(&self) -> &[f32] {
        &self.0
    }
}

impl From<Vec<f32>> for Embedding {
    fn from(values: Vec<f32>) -> Self {
        Self(values)
    }
}

/// Cosine similarity of two vectors.
pub(crate) fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Computes embeddings with the model of `ai.embeddings.model`, in batches
/// of `ai.embeddings.batch_size` sent in parallel, through the model's
/// failover chain, cached by model and text in the app's cache for
/// `ai.embeddings.cache_ttl_days`. Inject `Arc<Embeddings>`.
///
/// ```
/// use navree_ai::{AiFake, Embeddings};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_ai::AiError> {
/// let embeddings = Embeddings::new(AiFake::new().ai());
/// let one = embeddings.embed("Dana asked whether onboarding takes more than a month").await?;
/// let many = embeddings.embed_many(["first passage", "second passage"]).await?;
/// assert_eq!(one.dimensions(), 1536);
/// assert_eq!(many.len(), 2);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct Embeddings {
    ai: Ai,
}

impl Embeddings {
    /// Embeddings through `ai`.
    pub fn new(ai: Ai) -> Self {
        Self { ai }
    }

    /// The embedding model's name (with the fake and no
    /// `ai.embeddings.model`, `embed`).
    pub fn model(&self) -> Result<String, AiError> {
        match &self.ai.options().embeddings.model {
            Some(model) => Ok(model.clone()),
            None if self.ai.is_fake() => Ok("embed".to_owned()),
            None => Err(AiError::config(
                "ai.embeddings.model is not set: name the model that embeds",
            )),
        }
    }

    /// The vector size every embedding must have (`ai.vectors.dimensions`).
    pub fn dimensions(&self) -> usize {
        self.ai.options().vectors.dimensions
    }

    /// How vectors are compared (`ai.vectors.metric`).
    pub(crate) fn metric(&self) -> crate::options::Metric {
        self.ai.options().vectors.metric
    }

    /// The vector of one text.
    pub async fn embed(&self, text: impl AsRef<str>) -> Result<Embedding, AiError> {
        let mut vectors = self.embed_many([text.as_ref()]).await?;
        vectors
            .pop()
            .ok_or_else(|| AiError::internal("the provider returned no embedding"))
    }

    /// The vectors of `texts`, in input order.
    pub async fn embed_many<I, S>(&self, texts: I) -> Result<Vec<Embedding>, AiError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let texts: Vec<String> = texts.into_iter().map(|t| t.as_ref().to_owned()).collect();
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let model = self.model()?;
        let options = &self.ai.options().embeddings;
        let ttl_days = options.cache_ttl_days;
        let cache = self.ai.cache().await;
        let mut out: Vec<Option<Embedding>> = vec![None; texts.len()];
        if ttl_days > 0 {
            for (slot, text) in out.iter_mut().zip(&texts) {
                if let Ok(Some(values)) = cache.get::<Vec<f32>>(&cache_key(&model, text)).await {
                    *slot = Some(Embedding(values));
                }
            }
        }
        let missing: Vec<usize> = (0..texts.len()).filter(|i| out[*i].is_none()).collect();
        let batches: Vec<Vec<usize>> = missing
            .chunks(options.batch_size.max(1))
            .map(<[usize]>::to_vec)
            .collect();
        let results = try_join_all(batches.iter().map(|batch| {
            let batch_texts: Vec<String> = batch.iter().map(|i| texts[*i].clone()).collect();
            self.batch(&model, batch_texts)
        }))
        .await?;
        for (batch, vectors) in batches.iter().zip(results) {
            for (index, vector) in batch.iter().zip(vectors) {
                if ttl_days > 0 {
                    let ttl = Duration::from_secs(ttl_days * 24 * 3600);
                    if let Err(error) = cache
                        .put(&cache_key(&model, &texts[*index]), &vector.0, ttl)
                        .await
                    {
                        tracing::warn!(target: "navree::ai", %error, "caching an embedding failed");
                    }
                }
                out[*index] = Some(vector);
            }
        }
        Ok(out.into_iter().map(Option::unwrap_or_default).collect())
    }

    /// One provider call for `texts`, through the model's failover chain.
    async fn batch(&self, model: &str, texts: Vec<String>) -> Result<Vec<Embedding>, AiError> {
        let expected = self.dimensions();
        let spec = Spec {
            model: Some(model.to_owned()),
            ..Spec::default()
        };
        let ai = self.ai.clone();
        let started = Instant::now();
        let success = self
            .ai
            .failover("embed", &spec, &Extra::default(), move |driver, request| {
                let texts = texts.clone();
                let dimensions = ai
                    .resolve(&request.model_name)
                    .ok()
                    .and_then(|m| m.config.dimensions)
                    .or(if ai.is_fake() { Some(expected) } else { None });
                Box::pin(async move {
                    driver
                        .embed(&EmbedRequest {
                            model: request.model.clone(),
                            texts,
                            dimensions,
                            timeout: request.timeout,
                        })
                        .await
                })
            })
            .await?;
        let response = success.value;
        self.ai.record(
            "embed",
            &success.model.name,
            &success.model.provider,
            if success.position == 0 {
                "ok"
            } else {
                "failover"
            },
            Some(crate::Usage::new(response.input_tokens, 0)),
            None,
            &success.tried,
            started,
        );
        let mut vectors = Vec::with_capacity(response.vectors.len());
        for vector in response.vectors {
            if vector.len() != expected {
                return Err(AiError::config(format!(
                    "the embedding model `{}` returns vectors of {} dimensions, but ai.vectors.dimensions is {expected}",
                    success.model.name,
                    vector.len()
                )));
            }
            vectors.push(Embedding(vector));
        }
        Ok(vectors)
    }
}

/// The cache key of `text` embedded by `model`.
fn cache_key(model: &str, text: &str) -> String {
    format!(
        "ai:embed:{model}:{}",
        hex::encode(Sha256::digest(text.as_bytes()))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity() {
        let a = Embedding::new(vec![1.0, 0.0]);
        let b = Embedding::new(vec![0.0, 2.0]);
        let c = Embedding::new(vec![3.0, 0.0]);
        assert_eq!(a.cosine(&b), 0.0);
        assert!((a.cosine(&c) - 1.0).abs() < 1e-6);
        assert_eq!(a.cosine(&Embedding::new(vec![1.0])), 0.0);
        assert_eq!(a.dimensions(), 2);
        assert_eq!(&a[..], &[1.0, 0.0]);
    }

    #[test]
    fn embeddings_serialize_as_arrays() {
        let json = serde_json::to_value(Embedding::new(vec![0.5, 1.0])).unwrap();
        assert_eq!(json, serde_json::json!([0.5, 1.0]));
    }
}
