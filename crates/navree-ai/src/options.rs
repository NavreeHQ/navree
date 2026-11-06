//! [`AiOptions`]: the `ai` configuration section.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use navree_config::{ConfigSection, ConfigTable};
use navree_contracts::Secret;
use serde::{Deserialize, Serialize};

/// The provider drivers Navree ships, which are also the built-in provider
/// names.
pub const DRIVERS: [&str; 6] = [
    "anthropic",
    "openai",
    "gemini",
    "mistral",
    "ollama",
    "bedrock",
];

/// Options of [`AiModule`](crate::AiModule) (configuration section `ai`).
///
/// ```toml
/// [ai]
/// default = "smart"
/// timeout_secs = 60
/// max_retries = 2
/// structured_retries = 2
/// log_content = false
///
/// [ai.models.smart]
/// provider = "anthropic"
/// model = "claude-opus-5-5"
/// fallbacks = ["fast"]
///
/// [ai.models.fast]
/// provider = "anthropic"
/// model = "claude-sonnet-5-5"
/// max_tokens = 1024
/// ```
///
/// Keys belong in the environment: `APP_AI__PROVIDERS__ANTHROPIC__API_KEY`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, ConfigSection)]
#[config(key = "ai", docs = "/docs/ai/ai-sdk/", validate = Self::check_values)]
#[serde(default)]
pub struct AiOptions {
    /// The model a request uses when it names none (default: the first
    /// model by name).
    pub default: String,
    /// The limit of one attempt, in seconds; for streams, the longest gap
    /// between two events (default 60).
    pub timeout_secs: u64,
    /// Retries of one model on a 429, a 5xx, a timeout or a connection
    /// failure, before its fallbacks (default 2).
    pub max_retries: u32,
    /// The first retry delay in milliseconds; each later one doubles
    /// (default 500).
    pub retry_base_ms: u64,
    /// How often structured output that doesn't match its type is sent back
    /// for repair (default 2).
    pub structured_retries: u32,
    /// Per-user token budgets.
    #[config(table)]
    pub budget: BudgetOptions,
    /// Log prompts and outputs at `debug` (default false).
    pub log_content: bool,
    /// Answer every request from an [`AiFake`](crate::AiFake) (tests).
    pub fake: bool,
    /// Provider accounts by name.
    pub providers: BTreeMap<String, ProviderConfig>,
    /// Models by the name code uses (`smart`, `fast`, `embed`).
    pub models: BTreeMap<String, ModelConfig>,
    /// [`Embeddings`](crate::Embeddings).
    #[config(table)]
    pub embeddings: EmbeddingsOptions,
    /// [`VectorStore`](crate::VectorStore).
    #[config(table)]
    pub vectors: VectorsOptions,
    /// Agent conversation memory.
    #[config(table)]
    pub memory: MemoryOptions,
}

impl Default for AiOptions {
    fn default() -> Self {
        Self {
            default: String::new(),
            timeout_secs: 60,
            max_retries: 2,
            retry_base_ms: 500,
            structured_retries: 2,
            budget: BudgetOptions::default(),
            log_content: false,
            fake: false,
            providers: BTreeMap::new(),
            models: BTreeMap::new(),
            embeddings: EmbeddingsOptions::default(),
            vectors: VectorsOptions::default(),
            memory: MemoryOptions::default(),
        }
    }
}

/// Per-user token budgets (`ai.budget`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, ConfigTable)]
#[serde(default)]
pub struct BudgetOptions {
    /// Input plus output tokens one user may spend per UTC day; `0` turns
    /// budgets off (default).
    pub daily_tokens_per_user: u64,
}

/// One provider account (`ai.providers.<name>`). Set the keys you need; the
/// rest have defaults.
#[derive(Clone, Default, PartialEq, Deserialize, Serialize, ConfigTable)]
#[serde(default)]
pub struct ProviderConfig {
    /// The driver: `anthropic`, `openai`, `gemini`, `mistral`, `ollama` or
    /// `bedrock`. Defaults to the table's name, so `[ai.providers.openai]`
    /// needs none; a second account names it (`driver = "openai"`).
    pub driver: Option<String>,
    /// The API key (Anthropic, OpenAI, Gemini, Mistral). Never serialized.
    #[serde(skip_serializing)]
    pub api_key: Option<Secret<String>>,
    /// Replaces the vendor's API address (a proxy, a gateway, an
    /// OpenAI-compatible server, Ollama's address).
    pub url: Option<String>,
    /// OpenAI: the organization header.
    pub organization: Option<String>,
    /// Bedrock: the AWS region (`us-east-1`; else `AWS_REGION`, else
    /// `AWS_DEFAULT_REGION`).
    #[config(env_fallback = "AWS_REGION")]
    pub region: Option<String>,
    /// Bedrock: the access key id (else `AWS_ACCESS_KEY_ID`, else the
    /// shared credentials file).
    #[config(env_fallback = "AWS_ACCESS_KEY_ID")]
    pub access_key_id: Option<String>,
    /// Bedrock: the secret access key (else `AWS_SECRET_ACCESS_KEY`). Never
    /// serialized.
    #[serde(skip_serializing)]
    #[config(env_fallback = "AWS_SECRET_ACCESS_KEY")]
    pub secret_access_key: Option<Secret<String>>,
    /// Bedrock: a session token for temporary credentials (else
    /// `AWS_SESSION_TOKEN`). Never serialized.
    #[serde(skip_serializing)]
    #[config(env_fallback = "AWS_SESSION_TOKEN")]
    pub session_token: Option<Secret<String>>,
    /// Bedrock: the profile of the shared credentials file (default
    /// `AWS_PROFILE`, else `default`).
    #[config(env_fallback = "AWS_PROFILE")]
    pub profile: Option<String>,
}

impl ProviderConfig {
    /// Anthropic with `api_key`.
    pub fn anthropic(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(Secret::new(api_key.into())),
            ..Self::default()
        }
    }

    /// OpenAI with `api_key`.
    pub fn openai(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(Secret::new(api_key.into())),
            ..Self::default()
        }
    }

    /// A server that speaks the OpenAI chat completions API at `url`
    /// (`http://127.0.0.1:8000/v1`).
    pub fn openai_compatible(url: impl Into<String>) -> Self {
        Self {
            driver: Some("openai".to_owned()),
            url: Some(url.into()),
            ..Self::default()
        }
    }

    /// Google Gemini with `api_key`.
    pub fn gemini(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(Secret::new(api_key.into())),
            ..Self::default()
        }
    }

    /// Mistral with `api_key`.
    pub fn mistral(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(Secret::new(api_key.into())),
            ..Self::default()
        }
    }

    /// Ollama at `url` (`http://127.0.0.1:11434`).
    pub fn ollama(url: impl Into<String>) -> Self {
        Self {
            driver: Some("ollama".to_owned()),
            url: Some(url.into()),
            ..Self::default()
        }
    }

    /// AWS Bedrock in `region`, with the standard credential sources.
    pub fn bedrock(region: impl Into<String>) -> Self {
        Self {
            driver: Some("bedrock".to_owned()),
            region: Some(region.into()),
            ..Self::default()
        }
    }

    /// Sets the driver.
    pub fn driver(mut self, driver: impl Into<String>) -> Self {
        self.driver = Some(driver.into());
        self
    }

    /// Sets the API address.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// The driver of the provider named `name`.
    pub(crate) fn driver_for<'a>(&'a self, name: &'a str) -> &'a str {
        self.driver.as_deref().unwrap_or(name)
    }
}

/// Secrets are never printed, and neither is the access key id.
impl fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn mask<T>(value: &Option<T>) -> Option<&'static str> {
            value.as_ref().map(|_| "[redacted]")
        }
        f.debug_struct("ProviderConfig")
            .field("driver", &self.driver)
            .field("api_key", &mask(&self.api_key))
            .field("url", &self.url)
            .field("organization", &self.organization)
            .field("region", &self.region)
            .field("access_key_id", &mask(&self.access_key_id))
            .field("secret_access_key", &mask(&self.secret_access_key))
            .field("session_token", &mask(&self.session_token))
            .field("profile", &self.profile)
            .finish()
    }
}

/// One model (`ai.models.<name>`): a provider plus the vendor's model id,
/// under the name code uses.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ModelConfig {
    /// A provider name from `ai.providers`, or a built-in one.
    pub provider: String,
    /// The vendor's model id, sent as it is.
    pub model: String,
    /// The output limit of a request that sets none (default 4096).
    pub max_tokens: u32,
    /// Sampling temperature (default: the vendor's).
    pub temperature: Option<f32>,
    /// Models tried in order when this one is unavailable. A list, or a
    /// comma-separated string (from an environment variable).
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub fallbacks: Vec<String>,
    /// This model's attempt limit (default `ai.timeout_secs`).
    pub timeout_secs: Option<u64>,
    /// For embedding models: the vector size.
    pub dimensions: Option<usize>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            model: String::new(),
            max_tokens: 4096,
            temperature: None,
            fallbacks: Vec::new(),
            timeout_secs: None,
            dimensions: None,
        }
    }
}

impl ModelConfig {
    /// Model `model` of provider `provider`, with the defaults.
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            ..Self::default()
        }
    }

    /// Sets the fallbacks.
    pub fn fallbacks<I, S>(mut self, fallbacks: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.fallbacks = fallbacks.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the output limit.
    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Sets the vector size (embedding models).
    pub fn dimensions(mut self, dimensions: usize) -> Self {
        self.dimensions = Some(dimensions);
        self
    }
}

/// [`Embeddings`](crate::Embeddings) (`ai.embeddings`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ConfigTable)]
#[serde(default)]
pub struct EmbeddingsOptions {
    /// The model name in `ai.models` that embeds.
    pub model: Option<String>,
    /// Texts per provider call in `embed_many` (default 96).
    pub batch_size: usize,
    /// How long a text's vector is cached, in days; `0` turns the cache off
    /// (default 30).
    pub cache_ttl_days: u64,
}

impl Default for EmbeddingsOptions {
    fn default() -> Self {
        Self {
            model: None,
            batch_size: 96,
            cache_ttl_days: 30,
        }
    }
}

/// Where vectors live.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorStoreKind {
    /// The process's memory, searched exhaustively (tests, development).
    #[default]
    Memory,
    /// Tables in the app's PostgreSQL database, with an HNSW index.
    Pgvector,
    /// A Qdrant server, over its REST API.
    Qdrant,
}

/// How vectors are compared. Scores are always "higher is closer".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Cosine similarity (score from -1 to 1).
    #[default]
    Cosine,
    /// Dot product.
    Dot,
    /// Euclidean distance `d`, scored `1 / (1 + d)`.
    Euclidean,
}

impl Metric {
    /// `cosine`, `dot` or `euclidean`.
    pub fn as_str(self) -> &'static str {
        match self {
            Metric::Cosine => "cosine",
            Metric::Dot => "dot",
            Metric::Euclidean => "euclidean",
        }
    }
}

/// [`VectorStore`](crate::VectorStore) (`ai.vectors`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ConfigTable)]
#[serde(default)]
pub struct VectorsOptions {
    /// `memory` (default), `pgvector` or `qdrant`.
    #[config(one_of = "memory, pgvector, qdrant")]
    pub store: VectorStoreKind,
    /// The vector size of every collection (default 1536).
    pub dimensions: usize,
    /// `cosine` (default), `dot` or `euclidean`.
    #[config(one_of = "cosine, dot, euclidean")]
    pub metric: Metric,
    /// The Qdrant server (`http://127.0.0.1:6333`).
    pub url: Option<String>,
    /// The Qdrant API key. Never serialized.
    #[serde(skip_serializing)]
    pub key: Option<Secret<String>>,
}

impl Default for VectorsOptions {
    fn default() -> Self {
        Self {
            store: VectorStoreKind::Memory,
            dimensions: 1536,
            metric: Metric::Cosine,
            url: None,
            key: None,
        }
    }
}

/// Agent conversation memory (`ai.memory`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ConfigTable)]
#[serde(default)]
pub struct MemoryOptions {
    /// Messages the model sees before older ones are summarized (default
    /// 40).
    pub max_messages: usize,
    /// The model that writes summaries (default `ai.default`).
    pub summary_model: Option<String>,
    /// Age in days at which `ai:prune` deletes an untouched conversation
    /// (default 90).
    pub retention_days: u64,
}

impl Default for MemoryOptions {
    fn default() -> Self {
        Self {
            max_messages: 40,
            summary_model: None,
            retention_days: 90,
        }
    }
}

impl AiOptions {
    /// Checks the values (the boot calls this).
    fn check_values(&self) -> Result<(), String> {
        let errors = self.check();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Every problem with these options, each naming its key.
    pub fn check(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.timeout_secs == 0 {
            errors.push("ai.timeout_secs must be at least 1".to_owned());
        }
        for (name, provider) in &self.providers {
            let driver = provider.driver_for(name);
            if !DRIVERS.contains(&driver) {
                errors.push(format!(
                    "ai.providers.{name}.driver: unknown driver `{driver}` (expected one of {})",
                    DRIVERS.join(", ")
                ));
            }
        }
        for (name, model) in &self.models {
            if model.provider.is_empty() {
                errors.push(format!("ai.models.{name}.provider is required"));
            } else if !self.knows_provider(&model.provider) {
                errors.push(format!(
                    "ai.models.{name}.provider: unknown provider `{}`",
                    model.provider
                ));
            }
            if model.model.is_empty() {
                errors.push(format!("ai.models.{name}.model is required"));
            }
            if model.max_tokens == 0 {
                errors.push(format!("ai.models.{name}.max_tokens must be at least 1"));
            }
            if model.timeout_secs == Some(0) {
                errors.push(format!("ai.models.{name}.timeout_secs must be at least 1"));
            }
            for fallback in &model.fallbacks {
                if !self.models.contains_key(fallback) {
                    errors.push(format!(
                        "ai.models.{name}.fallbacks: unknown model `{fallback}`"
                    ));
                }
            }
        }
        if let Some(cycle) = self.fallback_cycle() {
            errors.push(format!(
                "ai.models: the fallbacks form a cycle: {}",
                cycle.join(" -> ")
            ));
        }
        if !self.default.is_empty() && !self.models.contains_key(&self.default) && !self.fake {
            errors.push(format!("ai.default: unknown model `{}`", self.default));
        }
        if let Some(model) = &self.embeddings.model
            && !self.models.contains_key(model)
            && !self.fake
        {
            errors.push(format!("ai.embeddings.model: unknown model `{model}`"));
        }
        if let Some(model) = &self.memory.summary_model
            && !self.models.contains_key(model)
            && !self.fake
        {
            errors.push(format!("ai.memory.summary_model: unknown model `{model}`"));
        }
        if self.embeddings.batch_size == 0 {
            errors.push("ai.embeddings.batch_size must be at least 1".to_owned());
        }
        if self.vectors.dimensions == 0 {
            errors.push("ai.vectors.dimensions must be at least 1".to_owned());
        }
        if self.vectors.store == VectorStoreKind::Qdrant && self.vectors.url.is_none() {
            errors.push("ai.vectors.url is required for store = \"qdrant\"".to_owned());
        }
        if self.memory.max_messages < 2 {
            errors.push("ai.memory.max_messages must be at least 2".to_owned());
        }
        errors.extend(self.embedding_fallback_sizes());
        errors
    }

    /// Whether `name` is a configured or built-in provider.
    pub(crate) fn knows_provider(&self, name: &str) -> bool {
        self.providers.contains_key(name) || DRIVERS.contains(&name)
    }

    /// The model a request without `.model(..)` uses.
    pub(crate) fn default_model(&self) -> Option<&str> {
        if !self.default.is_empty() {
            return Some(&self.default);
        }
        self.models.keys().next().map(String::as_str)
    }

    /// `name` and its fallbacks, depth first, each model once.
    pub(crate) fn chain(&self, name: &str) -> Vec<String> {
        fn visit(options: &AiOptions, name: &str, chain: &mut Vec<String>) {
            if chain.iter().any(|seen| seen == name) {
                return;
            }
            chain.push(name.to_owned());
            if let Some(model) = options.models.get(name) {
                for fallback in &model.fallbacks {
                    visit(options, fallback, chain);
                }
            }
        }
        let mut chain = Vec::new();
        visit(self, name, &mut chain);
        chain
    }

    /// A cycle in the fallbacks, if there is one.
    fn fallback_cycle(&self) -> Option<Vec<String>> {
        fn visit(
            options: &AiOptions,
            name: &str,
            path: &mut Vec<String>,
            done: &mut BTreeSet<String>,
        ) -> Option<Vec<String>> {
            if let Some(start) = path.iter().position(|seen| seen == name) {
                let mut cycle = path[start..].to_vec();
                cycle.push(name.to_owned());
                return Some(cycle);
            }
            if done.contains(name) {
                return None;
            }
            path.push(name.to_owned());
            if let Some(model) = options.models.get(name) {
                for fallback in &model.fallbacks {
                    if let Some(cycle) = visit(options, fallback, path, done) {
                        return Some(cycle);
                    }
                }
            }
            path.pop();
            done.insert(name.to_owned());
            None
        }
        let mut done = BTreeSet::new();
        for name in self.models.keys() {
            if let Some(cycle) = visit(self, name, &mut Vec::new(), &mut done) {
                return Some(cycle);
            }
        }
        None
    }

    /// Embedding fallbacks must have the embedding model's size.
    fn embedding_fallback_sizes(&self) -> Vec<String> {
        let Some(name) = &self.embeddings.model else {
            return Vec::new();
        };
        let Some(model) = self.models.get(name) else {
            return Vec::new();
        };
        let mut errors = Vec::new();
        for fallback in self.chain(name).iter().skip(1) {
            let Some(other) = self.models.get(fallback) else {
                continue;
            };
            if let (Some(a), Some(b)) = (model.dimensions, other.dimensions)
                && a != b
            {
                errors.push(format!(
                    "ai.models.{fallback}: an embedding fallback of `{name}` must have its size ({a} dimensions, not {b})"
                ));
            }
        }
        errors
    }

    /// The hosts of every configured provider URL (and the Qdrant URL):
    /// the operator chose them, so the SSRF guard lets them through.
    pub(crate) fn configured_hosts(&self) -> Vec<String> {
        let mut hosts: BTreeSet<String> = self
            .providers
            .values()
            .filter_map(|provider| provider.url.as_deref())
            .chain(self.vectors.url.as_deref())
            .filter_map(|url| url::Url::parse(url).ok())
            .filter_map(|url| url.host_str().map(str::to_owned))
            .map(|host| {
                host.trim_start_matches('[')
                    .trim_end_matches(']')
                    .to_owned()
            })
            .collect();
        // Ollama's default address, when Ollama is used without a URL.
        let uses_default_ollama = self.models.values().any(|model| {
            let provider = self.providers.get(&model.provider);
            let driver =
                provider.map_or(model.provider.as_str(), |p| p.driver_for(&model.provider));
            driver == "ollama" && provider.and_then(|p| p.url.as_ref()).is_none()
        });
        if uses_default_ollama {
            hosts.insert("127.0.0.1".to_owned());
        }
        hosts.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn options(value: serde_json::Value) -> AiOptions {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn defaults() {
        let o = AiOptions::default();
        assert_eq!(o.timeout_secs, 60);
        assert_eq!(o.max_retries, 2);
        assert_eq!(o.structured_retries, 2);
        assert_eq!(o.budget.daily_tokens_per_user, 0);
        assert!(!o.log_content && !o.fake);
        assert_eq!(o.embeddings.batch_size, 96);
        assert_eq!(o.embeddings.cache_ttl_days, 30);
        assert_eq!(o.vectors.store, VectorStoreKind::Memory);
        assert_eq!(o.vectors.dimensions, 1536);
        assert_eq!(o.vectors.metric, Metric::Cosine);
        assert_eq!(o.memory.max_messages, 40);
        assert_eq!(o.memory.retention_days, 90);
        assert_eq!(ModelConfig::default().max_tokens, 4096);
        assert!(o.check().is_empty());
    }

    #[test]
    fn fallbacks_accept_a_comma_separated_string() {
        let o = options(json!({
            "models": {
                "smart": { "provider": "anthropic", "model": "claude-opus-5-5", "fallbacks": "fast, backup" },
                "fast": { "provider": "anthropic", "model": "claude-sonnet-5-5" },
                "backup": { "provider": "openai", "model": "gpt-5" }
            }
        }));
        assert_eq!(o.models["smart"].fallbacks, ["fast", "backup"]);
        assert!(o.check().is_empty(), "{:?}", o.check());
    }

    #[test]
    fn chains_follow_fallbacks_depth_first_once_each() {
        let o = options(json!({
            "models": {
                "smart": { "provider": "anthropic", "model": "a", "fallbacks": ["fast", "backup"] },
                "fast": { "provider": "anthropic", "model": "b", "fallbacks": ["backup"] },
                "backup": { "provider": "openai", "model": "c" }
            }
        }));
        assert_eq!(o.chain("smart"), ["smart", "fast", "backup"]);
        assert_eq!(o.chain("backup"), ["backup"]);
    }

    #[test]
    fn invalid_options_name_the_key() {
        let o = options(json!({
            "default": "nope",
            "timeout_secs": 0,
            "providers": { "weird": { "driver": "cohere" } },
            "models": {
                "a": { "provider": "unknown", "model": "x", "fallbacks": ["b", "ghost"] },
                "b": { "provider": "openai", "model": "", "fallbacks": ["a"] }
            },
            "embeddings": { "model": "nothing" },
            "vectors": { "store": "qdrant" }
        }));
        let errors = o.check().join("\n");
        for expected in [
            "ai.timeout_secs",
            "ai.providers.weird.driver",
            "ai.models.a.provider: unknown provider `unknown`",
            "ai.models.a.fallbacks: unknown model `ghost`",
            "ai.models.b.model is required",
            "the fallbacks form a cycle",
            "ai.default: unknown model `nope`",
            "ai.embeddings.model: unknown model `nothing`",
            "ai.vectors.url is required",
        ] {
            assert!(
                errors.contains(expected),
                "missing `{expected}` in:\n{errors}"
            );
        }
    }

    #[test]
    fn embedding_fallbacks_must_have_the_same_size() {
        let o = options(json!({
            "models": {
                "embed": { "provider": "openai", "model": "text-embedding-3-small", "dimensions": 1536, "fallbacks": ["local"] },
                "local": { "provider": "ollama", "model": "nomic-embed-text", "dimensions": 768 }
            },
            "embeddings": { "model": "embed" }
        }));
        assert!(
            o.check().join(" ").contains("ai.models.local"),
            "{:?}",
            o.check()
        );
    }

    #[test]
    fn the_default_model_is_the_first_by_name_unless_set() {
        let mut o = options(json!({
            "models": {
                "b": { "provider": "openai", "model": "x" },
                "a": { "provider": "openai", "model": "y" }
            }
        }));
        assert_eq!(o.default_model(), Some("a"));
        o.default = "b".into();
        assert_eq!(o.default_model(), Some("b"));
    }

    #[test]
    fn configured_hosts_are_allowed() {
        let o = AiOptions {
            providers: [
                (
                    "local".to_owned(),
                    ProviderConfig::openai_compatible("http://10.0.0.5:8000/v1"),
                ),
                ("anthropic".to_owned(), ProviderConfig::anthropic("k")),
            ]
            .into(),
            models: [("m".to_owned(), ModelConfig::new("ollama", "llama3.3"))].into(),
            ..AiOptions::default()
        };
        assert_eq!(o.configured_hosts(), ["10.0.0.5", "127.0.0.1"]);
    }

    #[test]
    fn debug_never_prints_secrets() {
        let config = ProviderConfig {
            secret_access_key: Some(Secret::new("s3cr3t".to_owned())),
            ..ProviderConfig::anthropic("sk-ant-123")
        };
        let printed = format!("{config:?}");
        assert!(
            !printed.contains("sk-ant-123") && !printed.contains("s3cr3t"),
            "{printed}"
        );
        let vectors = VectorsOptions {
            key: Some(Secret::new("qdrant-key".to_owned())),
            ..VectorsOptions::default()
        };
        assert!(!format!("{vectors:?}").contains("qdrant-key"));
    }

    #[test]
    fn serializing_never_writes_a_credential() {
        let bedrock = ProviderConfig {
            access_key_id: Some("AKIA4EXAMPLE7ID".into()),
            secret_access_key: Some(Secret::new("aws-s3cr3t-access-key".to_owned())),
            session_token: Some(Secret::new("aws-s3ssion-t0ken".to_owned())),
            ..ProviderConfig::bedrock("eu-west-1")
        };
        let options = AiOptions {
            providers: [
                (
                    "anthropic".to_owned(),
                    ProviderConfig::anthropic("sk-ant-api-s3cr3t"),
                ),
                ("bedrock".to_owned(), bedrock),
            ]
            .into(),
            vectors: VectorsOptions {
                key: Some(Secret::new("qdrant-api-s3cr3t".to_owned())),
                ..VectorsOptions::default()
            },
            ..AiOptions::default()
        };
        let json = serde_json::to_string(&options).unwrap();
        for secret in [
            "sk-ant-api-s3cr3t",
            "aws-s3cr3t-access-key",
            "aws-s3ssion-t0ken",
            "qdrant-api-s3cr3t",
        ] {
            assert!(!json.contains(secret), "{secret} in {json}");
        }
        assert!(json.contains("eu-west-1"), "{json}");
    }

    #[test]
    fn credentials_are_read_from_a_file_as_plain_text() {
        let o = options(json!({
            "providers": {
                "anthropic": { "api_key": "sk-ant-1" },
                "bedrock": { "secret_access_key": "aws-1", "session_token": "tok-1" }
            },
            "vectors": { "store": "qdrant", "url": "http://127.0.0.1:6333", "key": "q-1" }
        }));
        let key = o.providers["anthropic"].api_key.as_ref().unwrap();
        assert_eq!(key.expose(), "sk-ant-1");
        let bedrock = &o.providers["bedrock"];
        assert_eq!(
            bedrock.secret_access_key.as_ref().unwrap().expose(),
            "aws-1"
        );
        assert_eq!(bedrock.session_token.as_ref().unwrap().expose(), "tok-1");
        assert_eq!(o.vectors.key.as_ref().unwrap().expose(), "q-1");
    }

    #[test]
    fn a_provider_describes_its_credentials_and_fallbacks() {
        let fields = <ProviderConfig as ConfigTable>::fields();
        let field = |name: &str| {
            fields
                .iter()
                .find(|field| field.name == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        for name in ["api_key", "secret_access_key", "session_token"] {
            assert!(field(name).secret, "{name}");
        }
        assert!(!field("access_key_id").secret);
        assert_eq!(
            field("secret_access_key").env_fallback.as_deref(),
            Some("AWS_SECRET_ACCESS_KEY")
        );
        assert_eq!(field("region").env_fallback.as_deref(), Some("AWS_REGION"));
        let value = ConfigTable::to_json(&ProviderConfig::anthropic("sk-ant-api-s3cr3t"));
        assert_eq!(value["api_key"], serde_json::Value::Null);
        assert!(!value.to_string().contains("sk-ant-api-s3cr3t"), "{value}");
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = AiOptions::schema().expect("schema");
        assert_eq!(schema.key, "ai");
        assert_eq!(schema.docs.as_deref(), Some("/docs/ai/ai-sdk/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        for name in [
            "default",
            "timeout_secs",
            "providers",
            "models",
            "budget",
            "embeddings",
            "vectors",
            "memory",
        ] {
            assert!(schema.fields.iter().any(|f| f.name == name), "{name}");
        }
        assert_eq!(schema.secret_paths(), ["ai.vectors.key"]);
        let store = schema
            .leaves()
            .into_iter()
            .find(|(path, _)| path == "ai.vectors.store")
            .map(|(_, field)| field.ty.clone());
        assert_eq!(
            store,
            Some(navree_config::FieldType::choice(&[
                "memory", "pgvector", "qdrant"
            ]))
        );
        let defaults = AiOptions::defaults().expect("defaults");
        assert_eq!(defaults["timeout_secs"], 60);
        assert_eq!(defaults["vectors"]["key"], serde_json::Value::Null);
    }

    #[test]
    fn the_section_never_shows_a_credential_in_debug_output() {
        let bedrock = ProviderConfig {
            access_key_id: Some("AKIA4EXAMPLE7ID".into()),
            secret_access_key: Some("aws-s3cr3t-access-key".into()),
            session_token: Some("aws-s3ssion-t0ken".into()),
            ..ProviderConfig::bedrock("eu-west-1")
        };
        let options = AiOptions {
            providers: [
                (
                    "anthropic".to_owned(),
                    ProviderConfig::anthropic("sk-ant-api-s3cr3t"),
                ),
                ("bedrock".to_owned(), bedrock),
            ]
            .into(),
            vectors: VectorsOptions {
                key: Some("qdrant-api-s3cr3t".into()),
                ..VectorsOptions::default()
            },
            ..AiOptions::default()
        };
        let printed = format!("{options:?}");
        for secret in [
            "sk-ant-api-s3cr3t",
            "AKIA4EXAMPLE7ID",
            "aws-s3cr3t-access-key",
            "aws-s3ssion-t0ken",
            "qdrant-api-s3cr3t",
        ] {
            assert!(!printed.contains(secret), "{secret} in {printed}");
        }
        assert!(printed.contains("eu-west-1"), "{printed}");
    }
}
