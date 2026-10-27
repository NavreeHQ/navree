//! The [`Workflow`] trait, its registration and the [`WorkflowRegistry`].

use std::any::{TypeId, type_name};
use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use navree_kernel::{BoxError, BoxFuture};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use uuid::Uuid;

use crate::{WorkflowCtx, WorkflowsError};

/// The longest workflow name.
pub const MAX_NAME_LEN: usize = 100;

/// A durable workflow: its input type, its name and version, and the code
/// that runs it (what `#[workflow]` implements on the input type).
///
/// ```
/// use navree_kernel::BoxError;
/// use navree_workflows::{Workflow, WorkflowCtx};
/// use serde::{Deserialize, Serialize};
/// use std::time::Duration;
///
/// #[derive(Serialize, Deserialize)]
/// struct Onboarding {
///     user_id: u64,
/// }
///
/// // #[workflow] async fn onboarding(ctx: WorkflowCtx, input: Onboarding) -> ..
/// // writes this impl.
/// impl Workflow for Onboarding {
///     const NAME: &'static str = "onboarding";
///     type Output = ();
///
///     async fn run(self, ctx: WorkflowCtx) -> Result<(), BoxError> {
///         ctx.step("send-welcome", || async { Ok::<_, BoxError>(()) }).await?;
///         ctx.sleep(Duration::from_secs(3 * 86_400)).await;
///         Ok(())
///     }
/// }
/// ```
///
/// **Replay.** A run is executed from the top every time it resumes; each
/// [`step`](WorkflowCtx::step) that completed before returns its stored
/// result instead of running again. Code between steps must therefore make
/// the same decisions on every execution: read the time with
/// [`ctx.now()`](WorkflowCtx::now), and do anything with side effects or
/// varying results (database reads, HTTP calls, random numbers) inside a
/// step.
pub trait Workflow: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The stable name stored with runs (`onboarding`): 1 to 100 lower-case
    /// letters, digits, `-`, `_` and `.`.
    const NAME: &'static str;

    /// The version new runs start on (default 1). A run always resumes on
    /// its own version: keep the old implementation registered (on another
    /// input type with the same name) until its runs finish, or let this one
    /// resume older runs with [`MIN_VERSION`](Workflow::MIN_VERSION) and
    /// branch on [`ctx.version()`](WorkflowCtx::version).
    const VERSION: u32 = 1;

    /// The oldest version whose runs this implementation resumes (default
    /// [`VERSION`](Workflow::VERSION): only its own).
    const MIN_VERSION: u32 = Self::VERSION;

    /// A description for people (the doc comment, with `#[workflow]`).
    const DESCRIPTION: &'static str = "";

    /// What the workflow returns, stored as the run's output.
    type Output: Serialize + DeserializeOwned + Send + 'static;

    /// Runs (or resumes) the workflow. An error fails the run.
    fn run(self, ctx: WorkflowCtx) -> impl Future<Output = Result<Self::Output, BoxError>> + Send;
}

/// How [`Workflows::start_with`](crate::Workflows::start_with) starts a
/// run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartOptions {
    /// The workspace the run belongs to (listed and authorized with it).
    pub workspace_id: Option<Uuid>,
    /// A deduplication key: while an unfinished run of the same workflow
    /// has this key, starting again returns that run's id instead.
    pub key: Option<String>,
    /// Start no earlier than this from now.
    pub delay: Option<Duration>,
}

impl StartOptions {
    /// No workspace, key or delay.
    pub fn new() -> Self {
        Self::default()
    }

    /// The run belongs to `workspace`.
    pub fn workspace(mut self, workspace: Uuid) -> Self {
        self.workspace_id = Some(workspace);
        self
    }

    /// At most one unfinished run with `key` at a time.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Start after `delay`.
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }
}

/// Runs a type-erased workflow: the input as JSON in, the output as JSON
/// out.
pub(crate) type RunFn = fn(Value, WorkflowCtx) -> BoxFuture<'static, Result<Value, BoxError>>;

/// A registered workflow: what the registry knows about one
/// implementation.
#[derive(Clone, Debug)]
pub struct WorkflowDefinition {
    /// [`Workflow::NAME`].
    pub name: &'static str,
    /// [`Workflow::VERSION`].
    pub version: u32,
    /// [`Workflow::MIN_VERSION`].
    pub min_version: u32,
    /// [`Workflow::DESCRIPTION`].
    pub description: &'static str,
    /// The input type's name.
    pub type_name: &'static str,
    /// The input type.
    pub(crate) type_id: TypeId,
    /// Runs it.
    pub(crate) run: RunFn,
}

impl WorkflowDefinition {
    /// The definition of workflow `W`.
    pub fn of<W: Workflow>() -> Self {
        Self {
            name: W::NAME,
            version: W::VERSION,
            min_version: W::MIN_VERSION,
            description: W::DESCRIPTION,
            type_name: type_name::<W>(),
            type_id: TypeId::of::<W>(),
            run: run_erased::<W>,
        }
    }

    /// Whether this implementation resumes runs started on `version`.
    pub fn resumes(&self, version: u32) -> bool {
        (self.min_version..=self.version).contains(&version)
    }
}

fn run_erased<W: Workflow>(
    input: Value,
    ctx: WorkflowCtx,
) -> BoxFuture<'static, Result<Value, BoxError>> {
    Box::pin(async move {
        let input: W = serde_json::from_value(input).map_err(|error| {
            WorkflowsError::payload(&format!("invalid input for workflow `{}`", W::NAME), error)
        })?;
        let output = input.run(ctx).await?;
        Ok(serde_json::to_value(output).map_err(|error| {
            WorkflowsError::payload(
                &format!("the output of workflow `{}` cannot be stored", W::NAME),
                error,
            )
        })?)
    })
}

/// `WorkflowsModule::for_feature::<W>()` contributes one.
pub(crate) struct WorkflowRegistration(pub(crate) WorkflowDefinition);

/// Every registered workflow, by type and by name and version.
#[derive(Debug, Default)]
pub struct WorkflowRegistry {
    by_type: BTreeMap<String, WorkflowDefinition>,
    /// Definitions by name, newest version first.
    by_name: BTreeMap<&'static str, Vec<WorkflowDefinition>>,
}

impl WorkflowRegistry {
    /// A registry of `definitions`: fails on an invalid name, an invalid
    /// version range or two types with the same name and version.
    pub fn new(
        definitions: impl IntoIterator<Item = WorkflowDefinition>,
    ) -> Result<Self, WorkflowsError> {
        let mut registry = WorkflowRegistry::default();
        for definition in definitions {
            check_name(definition.name).map_err(WorkflowsError::Other)?;
            if definition.version == 0 || definition.min_version > definition.version {
                return Err(WorkflowsError::Other(format!(
                    "workflow `{}` ({}): VERSION must be at least 1 and MIN_VERSION at most \
                     VERSION (got {} and {})",
                    definition.name,
                    definition.type_name,
                    definition.version,
                    definition.min_version
                )));
            }
            if registry.by_type.contains_key(definition.type_name) {
                // The same type registered by two modules: one definition.
                continue;
            }
            let versions = registry.by_name.entry(definition.name).or_default();
            if let Some(other) = versions.iter().find(|d| d.version == definition.version) {
                return Err(WorkflowsError::Other(format!(
                    "two workflows are registered as `{}` version {}: {} and {}",
                    definition.name, definition.version, other.type_name, definition.type_name
                )));
            }
            versions.push(definition.clone());
            versions.sort_by_key(|d| std::cmp::Reverse(d.version));
            registry
                .by_type
                .insert(definition.type_name.to_owned(), definition);
        }
        Ok(registry)
    }

    /// The definition of type `W`, if registered.
    pub fn of<W: Workflow>(&self) -> Option<&WorkflowDefinition> {
        self.by_type
            .get(type_name::<W>())
            .filter(|d| d.type_id == TypeId::of::<W>())
    }

    /// The implementation that resumes a run of `name` started on
    /// `version`: the one registered with that version, else the newest
    /// whose range covers it.
    pub fn for_run(&self, name: &str, version: u32) -> Option<&WorkflowDefinition> {
        let versions = self.by_name.get(name)?;
        versions
            .iter()
            .find(|d| d.version == version)
            .or_else(|| versions.iter().find(|d| d.resumes(version)))
    }

    /// The newest registered version of `name`.
    pub fn latest(&self, name: &str) -> Option<&WorkflowDefinition> {
        self.by_name.get(name).and_then(|versions| versions.first())
    }

    /// Every definition, by name, newest version first.
    pub fn iter(&self) -> impl Iterator<Item = &WorkflowDefinition> {
        self.by_name.values().flatten()
    }

    /// Whether no workflow is registered.
    pub fn is_empty(&self) -> bool {
        self.by_type.is_empty()
    }
}

/// The name rule: 1 to 100 characters, lower-case letters, digits, `-`,
/// `_` and `.`, starting with a letter or a digit.
pub fn check_name(name: &str) -> Result<(), String> {
    let valid =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.');
    let starts_well = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if name.is_empty() || name.len() > MAX_NAME_LEN || !starts_well || !name.chars().all(valid) {
        return Err(format!(
            "invalid workflow name `{name}`: use 1 to {MAX_NAME_LEN} lower-case letters, digits, \
             `-`, `_` and `.`, starting with a letter or a digit (`onboarding`)"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Serialize, Deserialize)]
    struct V1;
    impl Workflow for V1 {
        const NAME: &'static str = "onboarding";
        type Output = ();
        async fn run(self, _ctx: WorkflowCtx) -> Result<(), BoxError> {
            Ok(())
        }
    }

    #[derive(Serialize, Deserialize)]
    struct V3;
    impl Workflow for V3 {
        const NAME: &'static str = "onboarding";
        const VERSION: u32 = 3;
        const MIN_VERSION: u32 = 2;
        type Output = ();
        async fn run(self, _ctx: WorkflowCtx) -> Result<(), BoxError> {
            Ok(())
        }
    }

    #[derive(Serialize, Deserialize)]
    struct Clash;
    impl Workflow for Clash {
        const NAME: &'static str = "onboarding";
        type Output = ();
        async fn run(self, _ctx: WorkflowCtx) -> Result<(), BoxError> {
            Ok(())
        }
    }

    #[test]
    fn runs_resume_on_their_version() {
        let registry = WorkflowRegistry::new([
            WorkflowDefinition::of::<V1>(),
            WorkflowDefinition::of::<V3>(),
        ])
        .unwrap();
        assert_eq!(registry.latest("onboarding").unwrap().version, 3);
        assert_eq!(registry.for_run("onboarding", 1).unwrap().version, 1);
        assert_eq!(registry.for_run("onboarding", 2).unwrap().version, 3);
        assert_eq!(registry.for_run("onboarding", 3).unwrap().version, 3);
        assert!(registry.for_run("onboarding", 4).is_none());
        assert!(registry.for_run("billing", 1).is_none());
        assert_eq!(registry.of::<V1>().unwrap().version, 1);
        assert_eq!(registry.iter().count(), 2);
    }

    #[test]
    fn duplicates_and_bad_names_fail() {
        let error = WorkflowRegistry::new([
            WorkflowDefinition::of::<V1>(),
            WorkflowDefinition::of::<Clash>(),
        ])
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("two workflows are registered as `onboarding` version 1")
        );
        // The same type twice is one registration.
        assert!(
            WorkflowRegistry::new([
                WorkflowDefinition::of::<V1>(),
                WorkflowDefinition::of::<V1>()
            ])
            .is_ok()
        );
        assert!(check_name("Onboarding").is_err());
        assert!(check_name("").is_err());
        assert!(check_name("deal.follow-up_2").is_ok());
    }
}
