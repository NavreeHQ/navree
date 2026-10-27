//! [`MorphMap`]: which model a name in a polymorphic type column means.

use std::any::TypeId;
use std::borrow::Cow;
use std::future::Future;
use std::sync::{Arc, OnceLock, RwLock};

use crate::error::{OrmError, Result};
use crate::model::Model;
use crate::relations::morph::{MorphTarget, MorphTargetOf};

/// One model in a morph map.
#[derive(Clone)]
pub(crate) struct MorphEntry {
    pub(crate) name: Cow<'static, str>,
    pub(crate) type_id: TypeId,
    pub(crate) model: &'static str,
    pub(crate) target: Arc<dyn MorphTarget>,
}

/// The models polymorphic relations can load, by the name their type
/// columns store.
///
/// A polymorphic relation writes a model's
/// [`morph_name`](crate::Model::morph_name) into its type column (a
/// comment on a deal stores `deal`). Reading it back, a `morph_to`
/// relation needs the model behind the name: the models it lists itself
/// (`.target::<Deal>()`), else this map. Rust cannot find a type by name
/// at run time, so models are registered here, once, at boot (a module's
/// `register`, or the app's `main`):
///
/// ```
/// # use navree_orm::__doc_models::*;
/// use navree_orm::relations::MorphMap;
///
/// MorphMap::register::<Deal>();
/// MorphMap::register::<Company>();
/// assert_eq!(MorphMap::model_for("deal"), Some("Deal"));
/// assert_eq!(MorphMap::name_for::<Company>().as_deref(), Some("company"));
/// ```
///
/// With [`require`](MorphMap::require) on, writing the type of a model
/// that is not registered is an error ([`OrmError::MorphNotMapped`]), so
/// every stored name can be read back. Names are part of the data: renaming
/// a model's `morph_name` needs a data migration rewriting the stored
/// names (`UPDATE comments SET commentable_type = 'opportunity' WHERE
/// commentable_type = 'deal'`).
///
/// Tests that need a map of their own run under
/// [`scope`](MorphMap::scope), which replaces the process-wide map for one
/// future.
#[derive(Clone, Default)]
pub struct MorphMap {
    entries: Vec<MorphEntry>,
    required: bool,
}

fn global() -> &'static RwLock<MorphMap> {
    static GLOBAL: OnceLock<RwLock<MorphMap>> = OnceLock::new();
    GLOBAL.get_or_init(|| RwLock::new(MorphMap::default()))
}

tokio::task_local! {
    static SCOPED: MorphMap;
}

impl MorphMap {
    /// An empty map (for [`scope`](MorphMap::scope)).
    pub fn new() -> Self {
        MorphMap::default()
    }

    /// The map with model `M` added under its morph name (replacing a
    /// model registered under the same name).
    pub fn with<M: Model>(mut self) -> Self {
        self.add::<M>();
        self
    }

    /// The map, requiring every polymorphic model to be registered.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    fn add<M: Model>(&mut self) {
        let name = M::morph_name();
        self.entries
            .retain(|entry| entry.name != name && entry.type_id != TypeId::of::<M>());
        self.entries.push(MorphEntry {
            name,
            type_id: TypeId::of::<M>(),
            model: M::meta().name(),
            target: Arc::new(MorphTargetOf::<M>::new()),
        });
    }

    /// Registers model `M` in the process-wide map, under its morph name.
    pub fn register<M: Model>() {
        global()
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .add::<M>();
    }

    /// Requires (or no longer requires) every polymorphic model to be in
    /// the process-wide map.
    pub fn require(required: bool) {
        global()
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .required = required;
    }

    /// The map in effect: the scoped one, else the process-wide one.
    pub(crate) fn current() -> MorphMap {
        SCOPED
            .try_with(MorphMap::clone)
            .unwrap_or_else(|_| {
                global()
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
            })
    }

    /// Runs `future` with this map in place of the process-wide one.
    pub async fn scope<F: Future>(self, future: F) -> F::Output {
        SCOPED.scope(self, future).await
    }

    /// The model registered under `name`, by its model name (`Deal`).
    pub fn model_for(name: &str) -> Option<&'static str> {
        MorphMap::current().entry(name).map(|entry| entry.model)
    }

    /// The name `M` is registered under; `None` when it is not
    /// registered.
    pub fn name_for<M: Model>() -> Option<Cow<'static, str>> {
        MorphMap::current()
            .entries
            .iter()
            .find(|entry| entry.type_id == TypeId::of::<M>())
            .map(|entry| entry.name.clone())
    }

    /// Whether every polymorphic model must be registered.
    pub fn is_required() -> bool {
        MorphMap::current().required
    }

    /// The registered names, in registration order.
    pub fn names() -> Vec<Cow<'static, str>> {
        MorphMap::current()
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    }

    /// The entry registered under `name`.
    pub(crate) fn entry(&self, name: &str) -> Option<&MorphEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    /// Every entry, in registration order.
    pub(crate) fn entries(&self) -> &[MorphEntry] {
        &self.entries
    }
}

/// The name to store in a type column for `M`: its morph name, or
/// [`OrmError::MorphNotMapped`] when the map is required and `M` is not in
/// it.
pub(crate) fn type_name_of<M: Model>() -> Result<Cow<'static, str>> {
    let map = MorphMap::current();
    if map.required
        && !map
            .entries
            .iter()
            .any(|entry| entry.type_id == TypeId::of::<M>())
    {
        return Err(OrmError::MorphNotMapped {
            model: Cow::Borrowed(M::meta().name()),
        });
    }
    Ok(M::morph_name())
}
