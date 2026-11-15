//! AI provider drivers a module registers under a name: a provider table
//! selects one, `AiModule` builds it from that table, and its errors keep
//! the failover classes of the built-in drivers.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use navree_ai::{
    Ai, AiCapabilities, AiDriver, AiDriverAnswer, AiDriverChunk, AiDriverRequest, AiDriverStream,
    AiEmbedAnswer, AiEmbedRequest, AiError, AiModule, AiOptions, Embeddings, Finish, ModelConfig,
    ModuleBuilderAiProviderExt, Part, ProviderConfig, StreamEvent, TextStream, Usage,
};
use navree_kernel::{BoxError, BoxFuture, Module, ModuleBuilder};
use navree_testing::TestApp;
use serde_json::json;
use uuid::Uuid;

/// What the `vaani` driver's factory saw: each table's name and URL.
type Seen = Arc<Mutex<Vec<(Option<String>, Option<String>)>>>;

/// Answers "Namaste from {model}", whole and in two chunks, and embeds a
/// text as its length, its word count and 1.
struct Vaani;

impl AiDriver for Vaani {
    fn capabilities(&self) -> AiCapabilities {
        AiCapabilities::default().embeddings(true)
    }

    fn chat<'a>(
        &'a self,
        request: &'a AiDriverRequest,
    ) -> BoxFuture<'a, Result<AiDriverAnswer, AiError>> {
        Box::pin(async move {
            let text = format!("Namaste from {}", request.model());
            Ok(AiDriverAnswer::new(
                vec![Part::Text { text }],
                Usage::new(3, 1),
                Finish::Stop,
            ))
        })
    }

    fn stream<'a>(
        &'a self,
        request: &'a AiDriverRequest,
    ) -> BoxFuture<'a, Result<AiDriverStream, AiError>> {
        Box::pin(async move {
            let chunks: Vec<Result<AiDriverChunk, AiError>> = vec![
                Ok(AiDriverChunk::Text("Namaste".to_owned())),
                Ok(AiDriverChunk::Text(format!(" from {}", request.model()))),
                Ok(AiDriverChunk::Done {
                    usage: Usage::new(3, 2),
                    finish: Finish::Stop,
                }),
            ];
            Ok(Box::pin(futures_util::stream::iter(chunks)) as AiDriverStream)
        })
    }

    fn embed<'a>(
        &'a self,
        request: &'a AiEmbedRequest,
    ) -> BoxFuture<'a, Result<AiEmbedAnswer, AiError>> {
        Box::pin(async move {
            let vectors = request
                .texts()
                .iter()
                .map(|text| vec![text.len() as f32, text.split(' ').count() as f32, 1.0])
                .collect();
            Ok(AiEmbedAnswer::new(vectors, request.texts().len() as u64))
        })
    }
}

/// Registers the provider driver `vaani`, and records what its factory
/// read.
#[derive(Default)]
struct VaaniModule {
    seen: Seen,
}

impl Module for VaaniModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let seen = self.seen.clone();
        m.ai_provider("vaani", move |ctx| {
            let seen = seen.clone();
            async move {
                let table: ProviderConfig = ctx.settings()?;
                let instance = ctx.instance().map(str::to_owned);
                seen.lock().unwrap().push((instance, table.url));
                Ok::<_, BoxError>(Vaani)
            }
        });
    }
}

/// The text of a stream, and the model of its end.
async fn read(mut stream: TextStream) -> (String, String) {
    let mut text = String::new();
    let mut model = None;
    while let Some(event) = stream.next().await {
        match event.unwrap() {
            StreamEvent::Text(piece) => text.push_str(&piece),
            StreamEvent::Done(completion) => model = Some(completion.model),
            other => panic!("{other:?}"),
        }
    }
    (text, model.expect("the stream ends"))
}

#[tokio::test]
async fn a_registered_ai_driver_answers_streams_and_embeds() {
    let module = VaaniModule::default();
    let seen = module.seen.clone();
    let app = TestApp::builder()
        .config("ai.default", "smart")
        .config("ai.providers.vaani.url", "https://ai.ludhiana.example")
        .config(
            "ai.models",
            json!({
                "smart": { "provider": "vaani", "model": "vaani-large" },
                "embed": { "provider": "vaani", "model": "vaani-embed", "dimensions": 3 }
            }),
        )
        .config("ai.embeddings.model", "embed")
        .config("ai.vectors.dimensions", 3)
        .config("ai.budget.daily_tokens_per_user", 1000)
        .module(AiModule::from_config())
        .module(module)
        .build()
        .await;
    assert_eq!(
        *seen.lock().unwrap(),
        [(
            Some("vaani".to_owned()),
            Some("https://ai.ludhiana.example".to_owned())
        )]
    );

    let ai = app.get::<Ai>().await;
    let harpreet = Uuid::from_u128(7);
    let answer = ai.text("Namaste").user(harpreet).await.unwrap();
    assert_eq!(answer.text, "Namaste from vaani-large");
    assert_eq!(
        (answer.model.as_str(), answer.provider.as_str()),
        ("smart", "vaani")
    );
    assert_eq!(answer.usage, Usage::new(3, 1));
    assert_eq!(
        ai.tokens_used_today(harpreet).await,
        4,
        "the budget counts it"
    );

    let stream = ai.stream("Namaste").await.unwrap();
    assert_eq!(
        read(stream).await,
        ("Namaste from vaani-large".to_owned(), "smart".to_owned())
    );

    let embeddings = app.get::<Embeddings>().await;
    let vectors: Vec<Vec<f32>> = embeddings
        .embed_many(["Ludhiana", "Golden Temple Amritsar"])
        .await
        .unwrap()
        .into_iter()
        .map(|vector| vector.into_vec())
        .collect();
    assert_eq!(vectors, [vec![8.0, 1.0, 1.0], vec![22.0, 3.0, 1.0]]);

    // A table naming a driver no module registers fails the boot.
    let Err(error) = TestApp::builder()
        .config("ai.providers.backup.driver", "sarvagya")
        .module(AiModule::from_config())
        .module(VaaniModule::default())
        .try_build()
        .await
    else {
        panic!("the boot fails");
    };
    let message = format!("{error}");
    assert!(
        message.contains("ai.providers.backup.driver does not name a built-in AI provider driver or one a module registers with m.ai_provider (known: anthropic, openai, gemini, mistral, ollama, bedrock, vaani)"),
        "{message}"
    );
}

/// Registers the provider driver `vaani`, and records the URL and the key
/// its factory read.
#[derive(Default)]
struct KeyedVaaniModule {
    /// Each table's URL and key, as the factory read them.
    seen: Seen,
}

impl Module for KeyedVaaniModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let seen = self.seen.clone();
        m.ai_provider("vaani", move |ctx| {
            let seen = seen.clone();
            async move {
                let table: ProviderConfig = ctx.settings()?;
                let key = table.api_key.as_ref().map(|key| key.expose().clone());
                seen.lock().unwrap().push((table.url, key));
                Ok::<_, BoxError>(Vaani)
            }
        });
    }
}

#[tokio::test]
async fn a_provider_table_given_in_code_hands_its_driver_its_settings() {
    let module = KeyedVaaniModule::default();
    let seen = module.seen.clone();
    // The app's configuration has no `ai` section: the table is the one
    // given in code.
    let options = AiOptions {
        default: "smart".to_owned(),
        providers: [(
            "vaani".to_owned(),
            ProviderConfig::openai("sk-ludhiana-0042").url("https://ai.ludhiana.example"),
        )]
        .into(),
        models: [("smart".to_owned(), ModelConfig::new("vaani", "vaani-large"))].into(),
        ..AiOptions::default()
    };
    let app = TestApp::builder()
        .module(AiModule::for_root(options))
        .module(module)
        .build()
        .await;
    assert_eq!(
        *seen.lock().unwrap(),
        [(
            Some("https://ai.ludhiana.example".to_owned()),
            Some("sk-ludhiana-0042".to_owned())
        )]
    );
    let answer = app.get::<Ai>().await.text("Namaste").await.unwrap();
    assert_eq!(answer.text, "Namaste from vaani-large");
}

/// What a provider table does when it is called.
#[derive(Clone)]
enum Plan {
    /// Answers.
    Answer,
    /// Fails with the error the first time, then answers.
    FailOnce(AiError),
    /// Fails with the error every time.
    Fail(AiError),
    /// Starts a stream whose first chunk is the error, every time.
    FailFirstChunk(AiError),
}

/// What one call does.
enum Outcome {
    Answer,
    Error(AiError),
    FirstChunk(AiError),
}

/// What each provider table is set to do and how often it was called,
/// shared by the test and the drivers.
#[derive(Clone, Default)]
struct Script(Arc<Mutex<BTreeMap<String, (Plan, usize)>>>);

impl Script {
    fn set(&self, plans: &[(&str, Plan)]) {
        let mut tables = self.0.lock().unwrap();
        tables.clear();
        for (table, plan) in plans {
            tables.insert((*table).to_owned(), (plan.clone(), 0));
        }
    }

    fn calls(&self, table: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .get(table)
            .map_or(0, |(_, calls)| *calls)
    }

    /// Counts a call of `table` and says what it does.
    fn call(&self, table: &str) -> Outcome {
        let mut tables = self.0.lock().unwrap();
        let Some((plan, calls)) = tables.get_mut(table) else {
            return Outcome::Answer;
        };
        *calls += 1;
        match plan {
            Plan::Answer => Outcome::Answer,
            Plan::FailOnce(error) if *calls == 1 => Outcome::Error(error.clone()),
            Plan::FailOnce(_) => Outcome::Answer,
            Plan::Fail(error) => Outcome::Error(error.clone()),
            Plan::FailFirstChunk(error) => Outcome::FirstChunk(error.clone()),
        }
    }
}

/// A `vaani` driver built for one provider table, doing what the script
/// says for it.
struct Scripted {
    table: String,
    script: Script,
}

impl AiDriver for Scripted {
    fn capabilities(&self) -> AiCapabilities {
        AiCapabilities::default()
    }

    fn chat<'a>(
        &'a self,
        _request: &'a AiDriverRequest,
    ) -> BoxFuture<'a, Result<AiDriverAnswer, AiError>> {
        Box::pin(async move {
            match self.script.call(&self.table) {
                Outcome::Answer => Ok(AiDriverAnswer::new(
                    vec![Part::Text {
                        text: format!("{} answered", self.table),
                    }],
                    Usage::new(2, 2),
                    Finish::Stop,
                )),
                Outcome::Error(error) | Outcome::FirstChunk(error) => Err(error),
            }
        })
    }

    fn stream<'a>(
        &'a self,
        _request: &'a AiDriverRequest,
    ) -> BoxFuture<'a, Result<AiDriverStream, AiError>> {
        Box::pin(async move {
            let chunks: Vec<Result<AiDriverChunk, AiError>> = match self.script.call(&self.table) {
                Outcome::Answer => vec![
                    Ok(AiDriverChunk::Text(format!("{} answered", self.table))),
                    Ok(AiDriverChunk::Done {
                        usage: Usage::new(2, 2),
                        finish: Finish::Stop,
                    }),
                ],
                Outcome::Error(error) => return Err(error),
                Outcome::FirstChunk(error) => vec![Err(error)],
            };
            Ok(Box::pin(futures_util::stream::iter(chunks)) as AiDriverStream)
        })
    }
}

/// Registers `vaani` as a scripted driver: one per provider table, named
/// by the table it is built for.
struct ScriptedModule(Script);

impl Module for ScriptedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let script = self.0.clone();
        m.ai_provider("vaani", move |ctx| {
            let script = script.clone();
            async move {
                let table = ctx.instance().unwrap_or("").to_owned();
                Ok::<_, BoxError>(Scripted { table, script })
            }
        });
    }
}

#[tokio::test]
async fn a_registered_ai_drivers_errors_keep_their_failover_class() {
    let script = Script::default();
    let app = TestApp::builder()
        .config("ai.default", "smart")
        .config("ai.max_retries", 1)
        .config("ai.retry_base_ms", 1)
        .config(
            "ai.providers",
            json!({
                "vaani": { "url": "https://ai.ludhiana.example" },
                "vaani_backup": { "driver": "vaani", "url": "https://ai.amritsar.example" }
            }),
        )
        .config(
            "ai.models",
            json!({
                "smart": { "provider": "vaani", "model": "vaani-large", "fallbacks": ["backup"] },
                "backup": { "provider": "vaani_backup", "model": "vaani-small" }
            }),
        )
        .module(AiModule::from_config())
        .module(ScriptedModule(script.clone()))
        .build()
        .await;
    let ai = app.get::<Ai>().await;
    let backup = || ("vaani_backup", Plan::Answer);
    let who = |answer: &navree_ai::TextResponse| {
        (
            answer.text.clone(),
            answer.model.clone(),
            answer.provider.clone(),
        )
    };
    let by_backup = || {
        (
            "vaani_backup answered".to_owned(),
            "backup".to_owned(),
            "vaani_backup".to_owned(),
        )
    };

    // Rate limited: retried, and `smart` answers.
    script.set(&[
        (
            "vaani",
            Plan::FailOnce(AiError::rate_limited("vaani", None)),
        ),
        backup(),
    ]);
    let answer = ai.text("Namaste").await.unwrap();
    assert_eq!(
        who(&answer),
        (
            "vaani answered".to_owned(),
            "smart".to_owned(),
            "vaani".to_owned()
        )
    );
    assert_eq!(
        (script.calls("vaani"), script.calls("vaani_backup")),
        (2, 0)
    );

    // Unavailable every time: retried, then `backup` answers.
    script.set(&[
        ("vaani", Plan::Fail(AiError::unavailable("vaani"))),
        backup(),
    ]);
    let answer = ai.text("Namaste").await.unwrap();
    assert_eq!(who(&answer), by_backup());
    assert_eq!(
        (script.calls("vaani"), script.calls("vaani_backup")),
        (2, 1)
    );

    // Unsupported: the model is skipped at once.
    let unsupported = AiError::Unsupported {
        message: "no Punjabi voice".to_owned(),
    };
    script.set(&[("vaani", Plan::Fail(unsupported)), backup()]);
    let answer = ai.text("Namaste").await.unwrap();
    assert_eq!(who(&answer), by_backup());
    assert_eq!(
        (script.calls("vaani"), script.calls("vaani_backup")),
        (1, 1)
    );

    // A rejected request fails at once; `backup` is never called.
    script.set(&[
        ("vaani", Plan::Fail(AiError::request("vaani", "too long"))),
        backup(),
    ]);
    let error = ai.text("Namaste").await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "the AI provider vaani rejected the request: too long"
    );
    assert_eq!(
        (script.calls("vaani"), script.calls("vaani_backup")),
        (1, 0)
    );

    // A configuration error fails at once too, naming the provider.
    script.set(&[("vaani", Plan::Fail(AiError::config("no key"))), backup()]);
    let error = ai.text("Namaste").await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "AI configuration error: provider `vaani`: no key"
    );
    assert_eq!(
        (script.calls("vaani"), script.calls("vaani_backup")),
        (1, 0)
    );

    // A stream whose first chunk is an error falls over as `chat` does.
    script.set(&[
        ("vaani", Plan::FailFirstChunk(AiError::unavailable("vaani"))),
        backup(),
    ]);
    let stream = ai.stream("Namaste").await.unwrap();
    assert_eq!(
        read(stream).await,
        ("vaani_backup answered".to_owned(), "backup".to_owned())
    );
    assert_eq!(
        (script.calls("vaani"), script.calls("vaani_backup")),
        (2, 1)
    );
}
