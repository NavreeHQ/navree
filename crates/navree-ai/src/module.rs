//! [`AiModule`] and [`ModuleBuilderAiExt`].

use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, ConfiguredModule, Factory, Lifecycle, Module, ModuleBuilder,
    ProviderKey, Resolver,
};

use crate::agent::Agent;
use crate::commands::prune_command;
use crate::error::map_ai_error;
use crate::memory::{self, Conversations};
use crate::options::VectorStoreKind;
use crate::vectors::{VectorCollection, VectorStore};
use crate::{Ai, AiError, AiOptions, Embeddings};

/// Language models for the app (configuration section `ai`).
///
/// Global; provides and exports [`Ai`], [`Embeddings`], [`VectorStore`],
/// [`Conversations`] and [`AiOptions`]; contributes the `ai` migrations
/// (`ai_conversations`, `ai_messages`), the command `ai:prune [--days N]`
/// and the mapping of [`AiError`]s returned from handlers to their
/// statuses. With pgvector or Qdrant, the collections declared with
/// [`ModuleBuilderAiExt::vector_collection`] are created at boot.
///
/// It needs only `ConfigModule`. `DatabaseModule` is needed for
/// conversation memory and pgvector (conversation memory runs on
/// PostgreSQL, MySQL or MariaDB, and SQLite, whichever driver the app
/// turns on; pgvector needs PostgreSQL); `CacheModule` shares budgets and
/// the embeddings cache between processes (without it both live in the
/// process's memory). Agents also need `AuthModule` (their user) and
/// `AccessModule` (their abilities).
///
/// ```no_run
/// use navree_ai::{AiModule, AiOptions, ModelConfig, ProviderConfig};
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(AiModule::for_root(AiOptions {
///         default: "local".into(),
///         providers: [("ollama".to_owned(), ProviderConfig::ollama("http://127.0.0.1:11434"))].into(),
///         models: [("local".to_owned(), ModelConfig::new("ollama", "llama3.3"))].into(),
///         ..AiOptions::default()
///     }))
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct AiModule;

impl AiModule {
    /// The module with explicit options, without reading configuration.
    /// Invalid options fail the boot, naming the key.
    pub fn for_root(options: AiOptions) -> impl Module {
        ConfiguredModule::<AiModule, _>::for_root(options, register)
    }

    /// The module configured from the `ai` section at boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<AiModule, AiOptions>::from_config(register)
    }
}

impl Module for AiModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<AiOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<AiOptions>::new(move |_| {
            let options = options.clone();
            async move {
                let errors = options.check();
                if !errors.is_empty() {
                    return Err(format!("invalid ai options: {}", errors.join("; ")).into());
                }
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<AiOptions>(),
    };
    m.exports::<AiOptions>()
        .factory(
            Factory::<Ai>::new(|r: Resolver| async move {
                let options = r.get::<AiOptions>().await?;
                Ok(Arc::new(Ai::build(
                    (*options).clone(),
                    Some(r.app().downgrade()),
                )?))
            })
            .deps(vec![ProviderKey::of::<AiOptions>()]),
        )
        .exports::<Ai>()
        .factory(
            Factory::<Embeddings>::new(|r: Resolver| async move {
                Ok(Arc::new(Embeddings::new((*r.get::<Ai>().await?).clone())))
            })
            .deps(vec![ProviderKey::of::<Ai>()]),
        )
        .exports::<Embeddings>()
        .factory(
            Factory::<VectorStore>::new(|r: Resolver| async move {
                let ai = r.get::<Ai>().await?;
                let embeddings = r.get::<Embeddings>().await?;
                let db = r.app().try_get::<Db>().await;
                Ok(Arc::new(VectorStore::from_options(
                    &ai.options().vectors,
                    (*embeddings).clone(),
                    db,
                    ai.inner.http.clone(),
                )?))
            })
            .deps(vec![
                ProviderKey::of::<Ai>(),
                ProviderKey::of::<Embeddings>(),
            ]),
        )
        .exports::<VectorStore>()
        .lifecycle::<VectorStore>()
        .factory(Factory::<Conversations>::new(|r: Resolver| async move {
            Ok(Arc::new(Conversations::new(r.app().try_get::<Db>().await)))
        }))
        .exports::<Conversations>()
        .migrations(memory::migrations())
        .map_error::<AiError>(map_ai_error)
        .command(prune_command());
}

/// Creates the declared collections at boot (pgvector, Qdrant).
impl Lifecycle for VectorStore {
    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            if self.kind() == VectorStoreKind::Memory {
                return Ok(());
            }
            for declared in ctx.contributions::<VectorCollection>() {
                self.ensure_collection(declared.value.name())
                    .await
                    .map_err(|error| {
                        format!("vector collection `{}`: {error}", declared.value.name())
                    })?;
            }
            Ok(())
        })
    }
}

/// AI registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderAiExt {
    /// Registers agent `A` as a provider of this module, so
    /// `ai.agent::<A>()` can run it.
    fn agent<A: Agent>(&mut self) -> &mut Self;

    /// Declares a vector collection: with pgvector or Qdrant it is created
    /// at boot (an existing one of another size fails the boot).
    fn vector_collection(&mut self, collection: VectorCollection) -> &mut Self;
}

impl ModuleBuilderAiExt for ModuleBuilder {
    fn agent<A: Agent>(&mut self) -> &mut Self {
        self.provider::<A>()
    }

    fn vector_collection(&mut self, collection: VectorCollection) -> &mut Self {
        self.contribute(collection)
    }
}
