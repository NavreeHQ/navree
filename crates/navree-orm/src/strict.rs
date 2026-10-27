//! Strict modes: mistakes that are errors in development and tests, and
//! tolerated (or only reported) in production.
//!
//! | Mode | The mistake | Strict | Lenient |
//! |---|---|---|---|
//! | missing attributes | a partial select left out a model column | [`OrmError::MissingAttribute`] | the default of the field's type (an error when it has none) |
//! | discarding attributes | mass assignment names a column that is not fillable | [`OrmError::MassAssignment`] | the column is skipped |
//! | lazy loading | a relation is loaded one record at a time for records read together | [`OrmError::LazyLoading`] | the relation loads |
//!
//! `[orm] strict` turns them on together; each `prevent_*` key overrides
//! it (see [`OrmOptions`](crate::OrmOptions)). Handlers registered on
//! `OrmModule` (`on_missing_attribute`, `on_discarded_attributes`,
//! `on_lazy_loading`) take over from the error in strict mode, to log a
//! violation instead of failing: returning `Ok(())` tolerates it.
//!
//! Where a strictness comes from, first match wins: a
//! [`scope`] around the running task; the app of the executor an
//! operation runs on (its `OrmModule`); the app that booted last in this
//! process with `OrmModule` (for code without an executor, such as filling
//! a record); [`Strictness::default`] (missing and discarded attributes are
//! errors, lazy loading is allowed).

use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, PoisonError, RwLock};

use crate::error::{OrmError, Result};
use crate::exec::ExecContext;

/// A strict-mode violation, as handlers see it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Violation {
    /// A row lacked a model column.
    MissingAttribute {
        /// The model.
        model: &'static str,
        /// The missing column.
        column: String,
    },
    /// Mass assignment named columns that are not fillable.
    DiscardedAttributes {
        /// The model.
        model: &'static str,
        /// The columns.
        columns: Vec<String>,
    },
    /// A relation was loaded lazily for a record read with others.
    LazyLoading {
        /// The model.
        model: &'static str,
        /// The relation.
        relation: Cow<'static, str>,
    },
}

impl Violation {
    /// The error strict mode answers the violation with.
    pub fn into_error(self) -> OrmError {
        match self {
            Violation::MissingAttribute { model, column } => OrmError::MissingAttribute {
                model: Cow::Borrowed(model),
                column,
                expected: Cow::Borrowed("its field"),
                selected: Vec::new(),
            },
            Violation::DiscardedAttributes { model, columns } => OrmError::MassAssignment {
                model: Cow::Borrowed(model),
                columns,
            },
            Violation::LazyLoading { model, relation } => OrmError::LazyLoading {
                model: Cow::Borrowed(model),
                relation,
            },
        }
    }

    fn mode(&self) -> Mode {
        match self {
            Violation::MissingAttribute { .. } => Mode::MissingAttributes,
            Violation::DiscardedAttributes { .. } => Mode::DiscardingAttributes,
            Violation::LazyLoading { .. } => Mode::LazyLoading,
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::MissingAttribute { model, column } => {
                write!(f, "column `{column}` of {model} is missing from the row")
            }
            Violation::DiscardedAttributes { model, columns } => write!(
                f,
                "columns {} of {model} are not fillable and were not filled",
                columns.join(", ")
            ),
            Violation::LazyLoading { model, relation } => {
                write!(f, "relation `{relation}` of {model} was loaded lazily")
            }
        }
    }
}

/// What a handler does with a violation: `Ok(())` tolerates it, an error
/// is returned to the operation.
pub type ViolationHandler = Arc<dyn Fn(&Violation) -> Result<()> + Send + Sync>;

#[derive(Clone, Copy)]
enum Mode {
    MissingAttributes,
    DiscardingAttributes,
    LazyLoading,
}

/// Which strict modes are on, and the handlers that take over from their
/// errors.
///
/// ```
/// use navree_orm::strict::{Strictness, Violation};
///
/// let strict = Strictness::strict();
/// let violation = Violation::DiscardedAttributes { model: "User", columns: vec!["role".into()] };
/// assert!(strict.check(violation.clone()).is_err());
/// assert!(Strictness::lenient().check(violation.clone()).is_ok());
///
/// // A handler that only logs tolerates it.
/// let logging = Strictness::strict().on_discarded_attributes(|violation| {
///     eprintln!("{violation}");
///     Ok(())
/// });
/// assert!(logging.check(violation).is_ok());
/// ```
#[derive(Clone)]
pub struct Strictness {
    missing_attributes: bool,
    discarding_attributes: bool,
    lazy_loading: bool,
    auto_eager_load: bool,
    on_missing_attribute: Option<ViolationHandler>,
    on_discarded_attributes: Option<ViolationHandler>,
    on_lazy_loading: Option<ViolationHandler>,
}

/// Missing and discarded attributes are errors; lazy loading is allowed.
impl Default for Strictness {
    fn default() -> Self {
        Strictness {
            missing_attributes: true,
            discarding_attributes: true,
            lazy_loading: false,
            auto_eager_load: false,
            on_missing_attribute: None,
            on_discarded_attributes: None,
            on_lazy_loading: None,
        }
    }
}

impl Strictness {
    /// Every strict mode on.
    pub fn strict() -> Self {
        Strictness {
            lazy_loading: true,
            ..Strictness::default()
        }
    }

    /// Every strict mode off.
    pub fn lenient() -> Self {
        Strictness {
            missing_attributes: false,
            discarding_attributes: false,
            lazy_loading: false,
            ..Strictness::default()
        }
    }

    /// The strictness `options` describe in environment `env`.
    pub fn from_options(options: &crate::OrmOptions, env: Option<&str>) -> Self {
        let strict = options.is_strict_in(env);
        Strictness {
            missing_attributes: options.prevent_missing_attributes.unwrap_or(strict),
            discarding_attributes: options.prevent_discarding_attributes.unwrap_or(strict),
            lazy_loading: options.prevent_lazy_loading.unwrap_or(strict),
            auto_eager_load: options.auto_eager_load,
            ..Strictness::default()
        }
    }

    /// Whether a missing model column is an error.
    pub fn prevents_missing_attributes(&self) -> bool {
        self.missing_attributes
    }

    /// Whether filling a column that is not fillable is an error.
    pub fn prevents_discarding_attributes(&self) -> bool {
        self.discarding_attributes
    }

    /// Whether loading a relation lazily for a record read with others is
    /// an error.
    pub fn prevents_lazy_loading(&self) -> bool {
        self.lazy_loading
    }

    /// Whether a lazy load loads the relation for the whole result.
    pub fn auto_eager_loads(&self) -> bool {
        self.auto_eager_load
    }

    /// The same, with missing attributes an error or not.
    pub fn prevent_missing_attributes(self, on: bool) -> Self {
        Strictness {
            missing_attributes: on,
            ..self
        }
    }

    /// The same, with discarded attributes an error or not.
    pub fn prevent_discarding_attributes(self, on: bool) -> Self {
        Strictness {
            discarding_attributes: on,
            ..self
        }
    }

    /// The same, with lazy loading an error or not.
    pub fn prevent_lazy_loading(self, on: bool) -> Self {
        Strictness {
            lazy_loading: on,
            ..self
        }
    }

    /// The same, with `handler` deciding about missing attributes.
    pub fn on_missing_attribute(
        self,
        handler: impl Fn(&Violation) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        Strictness {
            on_missing_attribute: Some(Arc::new(handler)),
            ..self
        }
    }

    /// The same, with `handler` deciding about discarded attributes.
    pub fn on_discarded_attributes(
        self,
        handler: impl Fn(&Violation) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        Strictness {
            on_discarded_attributes: Some(Arc::new(handler)),
            ..self
        }
    }

    /// The same, with `handler` deciding about lazy loading.
    pub fn on_lazy_loading(
        self,
        handler: impl Fn(&Violation) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        Strictness {
            on_lazy_loading: Some(Arc::new(handler)),
            ..self
        }
    }

    /// Answers `violation`: tolerated when its mode is off; else its
    /// handler decides; else the violation's error.
    pub fn check(&self, violation: Violation) -> Result<()> {
        self.check_with(violation, Violation::into_error)
    }

    /// Like [`check`](Strictness::check), with `error` building the error
    /// (a more precise one than [`Violation::into_error`]).
    pub fn check_with(
        &self,
        violation: Violation,
        error: impl FnOnce(Violation) -> OrmError,
    ) -> Result<()> {
        let (on, handler) = match violation.mode() {
            Mode::MissingAttributes => (self.missing_attributes, &self.on_missing_attribute),
            Mode::DiscardingAttributes => {
                (self.discarding_attributes, &self.on_discarded_attributes)
            }
            Mode::LazyLoading => (self.lazy_loading, &self.on_lazy_loading),
        };
        if !on {
            return Ok(());
        }
        match handler {
            Some(handler) => handler(&violation),
            None => Err(error(violation)),
        }
    }
}

impl fmt::Debug for Strictness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Strictness")
            .field("missing_attributes", &self.missing_attributes)
            .field("discarding_attributes", &self.discarding_attributes)
            .field("lazy_loading", &self.lazy_loading)
            .field("auto_eager_load", &self.auto_eager_load)
            .field("on_missing_attribute", &self.on_missing_attribute.is_some())
            .field(
                "on_discarded_attributes",
                &self.on_discarded_attributes.is_some(),
            )
            .field("on_lazy_loading", &self.on_lazy_loading.is_some())
            .finish()
    }
}

tokio::task_local! {
    /// The strictness [`scope`] set for the running task.
    static SCOPED: Strictness;
}

/// The strictness of the app that booted last with `OrmModule`.
static PROCESS: RwLock<Option<Strictness>> = RwLock::new(None);

/// Runs `future` with `strictness`, whatever the app says (tests that need
/// a mode on or off, a data import that tolerates partial rows). The
/// choice belongs to the task running `future`.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db) -> Result<()> {
/// use navree_orm::strict::{self, Strictness};
///
/// // Read three columns; the others take their defaults.
/// let slim = strict::scope(
///     Strictness::strict().prevent_missing_attributes(false),
///     Deal::query().select(["id", "title", "stage"]).get(&db),
/// )
/// .await?;
/// # Ok(()) }
/// ```
pub async fn scope<F: Future>(strictness: Strictness, future: F) -> F::Output {
    SCOPED.scope(strictness, future).await
}

/// Runs `f` with `strictness` (decoding rows, which has no executor).
pub(crate) fn sync_scope<R>(strictness: Strictness, f: impl FnOnce() -> R) -> R {
    SCOPED.sync_scope(strictness, f)
}

/// The strictness for code without an executor: the task's [`scope`],
/// else the app that booted last with `OrmModule`, else
/// [`Strictness::default`].
pub fn current() -> Strictness {
    if let Ok(scoped) = SCOPED.try_with(Strictness::clone) {
        return scoped;
    }
    PROCESS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

/// Makes `strictness` the process's (what `OrmModule` does at boot).
pub(crate) fn install(strictness: Strictness) {
    *PROCESS.write().unwrap_or_else(PoisonError::into_inner) = Some(strictness);
}

impl ExecContext {
    /// The strictness of this operation: the task's [`scope`], else the
    /// app's `OrmModule`, else [`current`].
    pub async fn strictness(&self) -> Strictness {
        if let Ok(scoped) = SCOPED.try_with(Strictness::clone) {
            return scoped;
        }
        if let Some(app) = self.app()
            && let Some(runtime) = app.try_get::<crate::OrmRuntime>().await
        {
            return runtime.strictness().clone();
        }
        current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OrmOptions;

    fn discarded() -> Violation {
        Violation::DiscardedAttributes {
            model: "User",
            columns: vec!["role".into(), "admin".into()],
        }
    }

    #[test]
    fn options_choose_the_modes() {
        let production = Strictness::from_options(&OrmOptions::default(), Some("production"));
        assert!(!production.prevents_missing_attributes());
        assert!(!production.prevents_lazy_loading());
        let development = Strictness::from_options(&OrmOptions::default(), None);
        assert!(development.prevents_missing_attributes());
        assert!(development.prevents_discarding_attributes());
        assert!(development.prevents_lazy_loading());
        let mixed = Strictness::from_options(
            &OrmOptions {
                strict: Some(true),
                prevent_lazy_loading: Some(false),
                auto_eager_load: true,
                ..OrmOptions::default()
            },
            None,
        );
        assert!(mixed.prevents_discarding_attributes());
        assert!(!mixed.prevents_lazy_loading());
        assert!(mixed.auto_eager_loads());
    }

    #[test]
    fn handlers_take_over_from_the_error() {
        let error = Strictness::strict().check(discarded()).unwrap_err();
        assert!(matches!(error, OrmError::MassAssignment { .. }));
        let refusing = Strictness::strict().on_discarded_attributes(|violation| {
            Err(OrmError::invalid(format!("refused: {violation}")))
        });
        assert_eq!(
            refusing.check(discarded()).unwrap_err().to_string(),
            "invalid query: refused: columns role, admin of User are not fillable and were not filled"
        );
        let lenient = Strictness::lenient().on_discarded_attributes(|_| panic!("not asked"));
        assert!(lenient.check(discarded()).is_ok(), "off: no handler runs");
    }

    #[tokio::test]
    async fn scopes_win_over_everything() {
        assert!(current().prevents_missing_attributes());
        scope(Strictness::lenient(), async {
            assert!(!current().prevents_missing_attributes());
            assert!(
                !ExecContext::default()
                    .strictness()
                    .await
                    .prevents_missing_attributes()
            );
        })
        .await;
        sync_scope(Strictness::lenient(), || {
            assert!(!current().prevents_discarding_attributes());
        });
    }
}
