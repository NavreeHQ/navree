//! [`FlagsFake`]: flags that answer what a test tells them.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::service::Change;
use crate::{Flag, FlagDefinition, FlagScope, FlagState, Flags};

/// Flags for tests that answer what you tell them, whatever the scope,
/// and record every check. Flags the fake was not told about resolve to
/// their `default`; no registration, storage or cache is involved.
///
/// ```
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// use navree_flags::{Flag, FlagScope, FlagsFake};
///
/// struct NewPipeline;
/// impl Flag for NewPipeline {
///     const NAME: &'static str = "new-pipeline";
/// }
/// struct DealExport;
/// impl Flag for DealExport {
///     const NAME: &'static str = "deal-export";
///     const DEFAULT: bool = true;
/// }
///
/// let fake = FlagsFake::new().on::<NewPipeline>();
/// let flags = fake.flags(); // install with TestApp::builder().override_value(Arc::new(flags))
/// assert!(flags.active::<NewPipeline>(FlagScope::everyone()).await.unwrap());
/// fake.assert_checked::<NewPipeline>();
/// fake.assert_not_checked::<DealExport>();
/// # });
/// ```
///
/// Writes through the fake's `Flags` change its answers: `activate` turns
/// a flag on and `deactivate` and `kill` turn it off (for every scope),
/// `forget` and `revive` return it to its default; rollouts are ignored.
#[derive(Clone, Debug, Default)]
pub struct FlagsFake {
    state: Arc<FakeState>,
}

#[derive(Debug, Default)]
pub(crate) struct FakeState {
    inner: Mutex<FakeInner>,
}

#[derive(Debug, Default)]
struct FakeInner {
    /// Told (or written) answers, by name.
    answers: BTreeMap<String, bool>,
    /// Every flag told, written or checked, by name.
    known: BTreeMap<String, FlagDefinition>,
    /// Every check, in order.
    checks: Vec<(String, FlagScope)>,
}

impl FlagsFake {
    /// A fake that was told nothing: every flag is its default.
    pub fn new() -> Self {
        Self::default()
    }

    /// Answers `true` for `F`.
    pub fn on<F: Flag>(self) -> Self {
        self.set::<F>(true);
        self
    }

    /// Answers `false` for `F`.
    pub fn off<F: Flag>(self) -> Self {
        self.set::<F>(false);
        self
    }

    /// Answers `value` for `F` from now on (also while the app runs).
    pub fn set<F: Flag>(&self, value: bool) {
        let definition = FlagDefinition::of::<F>();
        let mut inner = self.state.lock();
        inner.answers.insert(definition.name.to_owned(), value);
        inner.known.insert(definition.name.to_owned(), definition);
    }

    /// The `Flags` service backed by this fake. Install it with
    /// `TestApp::builder().override_value(Arc::new(fake.flags()))`; the
    /// fake keeps recording through every clone.
    pub fn flags(&self) -> Flags {
        Flags::fake(self.state.clone())
    }

    /// How many times `F` was checked.
    pub fn checked<F: Flag>(&self) -> usize {
        self.state
            .lock()
            .checks
            .iter()
            .filter(|(name, _)| name == F::NAME)
            .count()
    }

    /// The scopes `F` was checked for, in order.
    pub fn checks_of<F: Flag>(&self) -> Vec<FlagScope> {
        self.state
            .lock()
            .checks
            .iter()
            .filter(|(name, _)| name == F::NAME)
            .map(|(_, scope)| *scope)
            .collect()
    }

    /// Panics unless `F` was checked.
    #[track_caller]
    pub fn assert_checked<F: Flag>(&self) {
        let checked: Vec<String> = self.checked_names();
        assert!(
            checked.iter().any(|name| name == F::NAME),
            "the flag `{}` was not checked; checked: {checked:?}",
            F::NAME
        );
    }

    /// Panics if `F` was checked.
    #[track_caller]
    pub fn assert_not_checked<F: Flag>(&self) {
        let times = self.checked::<F>();
        assert_eq!(
            times,
            0,
            "the flag `{}` was checked {times} time(s)",
            F::NAME
        );
    }

    fn checked_names(&self) -> Vec<String> {
        self.state
            .lock()
            .checks
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }
}

impl FakeState {
    fn lock(&self) -> MutexGuard<'_, FakeInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn check(&self, definition: &FlagDefinition, scope: &FlagScope) -> bool {
        let mut inner = self.lock();
        inner.checks.push((definition.name.to_owned(), *scope));
        inner
            .known
            .entry(definition.name.to_owned())
            .or_insert(*definition);
        inner
            .answers
            .get(definition.name)
            .copied()
            .unwrap_or(definition.default)
    }

    pub(crate) fn all(&self, scope: &FlagScope) -> BTreeMap<String, bool> {
        let known: Vec<FlagDefinition> = self.lock().known.values().copied().collect();
        known
            .iter()
            .map(|definition| (definition.name.to_owned(), self.check(definition, scope)))
            .collect()
    }

    pub(crate) fn change(&self, definition: &FlagDefinition, change: &Change) {
        let mut inner = self.lock();
        inner
            .known
            .entry(definition.name.to_owned())
            .or_insert(*definition);
        let name = definition.name.to_owned();
        match change {
            Change::Everyone(Some(value)) | Change::Override(_, _, Some(value)) => {
                inner.answers.insert(name, *value);
            }
            Change::Killed(true) => {
                inner.answers.insert(name, false);
            }
            Change::Everyone(None)
            | Change::Override(_, _, None)
            | Change::Killed(false)
            | Change::Purge => {
                inner.answers.remove(&name);
            }
            Change::Rollout(_) => {}
        }
    }

    pub(crate) fn state(&self, definition: &FlagDefinition) -> FlagState {
        FlagState {
            everyone: self.lock().answers.get(definition.name).copied(),
            ..FlagState::unset(definition.name)
        }
    }

    pub(crate) fn states(&self) -> BTreeMap<String, FlagState> {
        self.lock()
            .answers
            .iter()
            .map(|(name, value)| {
                let state = FlagState {
                    everyone: Some(*value),
                    ..FlagState::unset(name)
                };
                (name.clone(), state)
            })
            .collect()
    }

    pub(crate) fn known(&self, name: &str) -> Option<FlagDefinition> {
        self.lock().known.get(name).copied()
    }
}
