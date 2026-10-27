//! The registered importers, by name, with their row types erased, and the
//! row checker (serde, then garde, in the app's validation wording).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use garde::Validate;
use navree_contracts::BoxError;
use navree_db::Uow;
use navree_kernel::{AppContext, BoxFuture, Injectable, ModuleId};
use navree_validation::{MessageTranslator, Messages, ValidationErrors, validate_in};
use serde::de::DeserializeOwned;

use super::de::read_row;
use super::{Import, ImportColumn, ImportContext, ImportRecord, ImportRow, RowRejected};
use crate::export::registry::check_name;
use crate::{Cell, SheetsError};

/// Opens a registered importer.
type OpenFn =
    fn(AppContext, ModuleId) -> BoxFuture<'static, Result<Arc<dyn ErasedImport>, SheetsError>>;

/// An importer registered by a module (a contribution of
/// `m.importer::<I>()`).
#[derive(Clone)]
pub(crate) struct ImporterRegistration {
    pub(crate) name: &'static str,
    pub(crate) module: ModuleId,
    pub(crate) permission: Option<String>,
    pub(crate) columns: fn() -> Vec<ImportColumn>,
    open: OpenFn,
}

impl ImporterRegistration {
    pub(crate) fn of<I: Import + Injectable>(module: ModuleId) -> Self {
        ImporterRegistration {
            name: I::NAME,
            module,
            permission: I::permission(),
            columns: <I::Row as ImportRow>::columns,
            open: open::<I>,
        }
    }

    pub(crate) async fn open(&self, app: AppContext) -> Result<Arc<dyn ErasedImport>, SheetsError> {
        (self.open)(app, self.module).await
    }
}

/// Why saving a row failed.
#[derive(Debug)]
pub(crate) enum SaveError {
    /// The stored values no longer read or validate.
    Invalid(ValidationErrors),
    /// The import refused it ([`RowRejected`]).
    Rejected(String),
    /// Anything else (logged; the row is reported as failed).
    Failed(BoxError),
}

/// An importer with its row type erased.
pub(crate) trait ErasedImport: Send + Sync {
    /// Reads and checks a row; its duplicate key when it is valid.
    fn check<'a>(
        &'a self,
        checker: &'a RowChecker,
        fields: Vec<(String, Cell)>,
    ) -> BoxFuture<'a, Result<Option<String>, ValidationErrors>>;

    /// Which of `keys` the app already has.
    fn existing<'a>(
        &'a self,
        ctx: &'a ImportContext,
        keys: &'a [String],
    ) -> BoxFuture<'a, Result<HashSet<String>, BoxError>>;

    /// Reads a row again and saves it (`update`: as an update of the
    /// record it duplicates).
    fn save<'a>(
        &'a self,
        ctx: &'a ImportContext,
        uow: &'a Uow,
        checker: &'a RowChecker,
        fields: Vec<(String, Cell)>,
        update: bool,
    ) -> BoxFuture<'a, Result<(), SaveError>>;

    /// The import was applied.
    fn completed<'a>(
        &'a self,
        ctx: &'a ImportContext,
        record: &'a ImportRecord,
    ) -> BoxFuture<'a, Result<(), BoxError>>;
}

struct Typed<I>(Arc<I>);

impl<I: Import> ErasedImport for Typed<I> {
    fn check<'a>(
        &'a self,
        checker: &'a RowChecker,
        fields: Vec<(String, Cell)>,
    ) -> BoxFuture<'a, Result<Option<String>, ValidationErrors>> {
        Box::pin(async move {
            let row: I::Row = checker.check(fields).await?;
            Ok(I::key(&row))
        })
    }

    fn existing<'a>(
        &'a self,
        ctx: &'a ImportContext,
        keys: &'a [String],
    ) -> BoxFuture<'a, Result<HashSet<String>, BoxError>> {
        Box::pin(self.0.existing(ctx, keys))
    }

    fn save<'a>(
        &'a self,
        ctx: &'a ImportContext,
        uow: &'a Uow,
        checker: &'a RowChecker,
        fields: Vec<(String, Cell)>,
        update: bool,
    ) -> BoxFuture<'a, Result<(), SaveError>> {
        Box::pin(async move {
            let row: I::Row = checker.check(fields).await.map_err(SaveError::Invalid)?;
            let saved = if update {
                self.0.update(ctx, uow, row).await
            } else {
                self.0.insert(ctx, uow, row).await
            };
            saved.map_err(|error| match error.downcast::<RowRejected>() {
                Ok(rejected) => SaveError::Rejected(rejected.message().to_owned()),
                Err(error) => SaveError::Failed(error),
            })
        })
    }

    fn completed<'a>(
        &'a self,
        ctx: &'a ImportContext,
        record: &'a ImportRecord,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(self.0.completed(ctx, record))
    }
}

fn open<I: Import + Injectable>(
    app: AppContext,
    module: ModuleId,
) -> BoxFuture<'static, Result<Arc<dyn ErasedImport>, SheetsError>> {
    Box::pin(async move {
        let import = app
            .resolver_for(module)
            .get::<I>()
            .await
            .map_err(|error| SheetsError::App(error.into()))?;
        Ok(Arc::new(Typed(import)) as Arc<dyn ErasedImport>)
    })
}

/// Reads rows into their type and runs their rules, with the app's
/// validation messages (and its message translator: navree-i18n's, in
/// the locale the import runs in).
pub(crate) struct RowChecker {
    app: AppContext,
    messages: Arc<Messages>,
    translator: Option<Arc<dyn MessageTranslator>>,
}

impl RowChecker {
    pub(crate) async fn new(app: &AppContext) -> RowChecker {
        RowChecker {
            messages: app
                .try_get::<Messages>()
                .await
                .unwrap_or_else(|| Arc::new(Messages::default())),
            translator: app.try_get::<dyn MessageTranslator>().await,
            app: app.clone(),
        }
    }

    /// `fields` read into `R`, then `R`'s garde rules; errors by field,
    /// rules on the whole row under `row`.
    pub(crate) async fn check<R>(&self, fields: Vec<(String, Cell)>) -> Result<R, ValidationErrors>
    where
        R: DeserializeOwned + Validate<Context = ()>,
    {
        let row: R = read_row(fields).map_err(|error| {
            let mut errors = ValidationErrors::new();
            let (field, message) = error.render_with(&self.messages, self.translator.as_deref());
            errors.add(field, message);
            errors
        })?;
        validate_in(&row, Some(&self.app)).await.map_err(|errors| {
            let mut renamed = ValidationErrors::new();
            for (field, messages) in errors.iter() {
                let field = if field == "input" { "row" } else { field };
                for message in messages {
                    renamed.add(field, message.clone());
                }
            }
            renamed
        })?;
        Ok(row)
    }
}

/// The importers of the app, by name.
#[derive(Default)]
pub(crate) struct ImporterRegistry {
    by_name: BTreeMap<&'static str, ImporterRegistration>,
}

impl ImporterRegistry {
    /// Every importer the app's modules registered; two with one name are
    /// an error.
    pub(crate) fn build(app: &AppContext) -> Result<Self, String> {
        let mut by_name = BTreeMap::new();
        for contributed in app.contributions::<ImporterRegistration>() {
            let registration = (*contributed.value).clone();
            check_name("importer", registration.name)?;
            if by_name.insert(registration.name, registration).is_some() {
                return Err(format!(
                    "two importers are named `{}` (m.importer::<I>())",
                    contributed.value.name
                ));
            }
        }
        Ok(ImporterRegistry { by_name })
    }

    pub(crate) fn get(&self, name: &str) -> Result<&ImporterRegistration, SheetsError> {
        self.by_name
            .get(name)
            .ok_or_else(|| SheetsError::UnknownImporter(name.to_owned()))
    }

    pub(crate) fn all(&self) -> impl Iterator<Item = &ImporterRegistration> {
        self.by_name.values()
    }
}
