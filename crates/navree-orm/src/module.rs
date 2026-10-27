//! [`OrmModule`] (the `model:show` command) and [`ModuleBuilderOrmExt`]
//! (the models modules register, for commands and tooling).

use std::fmt;

use navree_kernel::{AppContext, BoxError, BoxFuture, Module, ModuleBuilder};

use crate::hooks::ModelHook;
use crate::model::{Model, ModelMeta};
use crate::show::ModelDescription;

/// The ORM's module: the `model:show` command over the registered models
/// ([`ModuleBuilderOrmExt::model`], which imports it, so apps rarely add
/// it themselves). The ORM works without it.
#[derive(Clone, Copy, Debug, Default)]
pub struct OrmModule;

impl Module for OrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.command(crate::show::command());
    }
}

/// Registers models with the app.
///
/// ```
/// # use navree_orm::__doc_factories::*;
/// use navree_kernel::{Module, ModuleBuilder};
/// use navree_orm::ModuleBuilderOrmExt;
///
/// struct DealsModule;
///
/// impl Module for DealsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.model::<Deal>().model::<Company>();
///     }
/// }
/// ```
pub trait ModuleBuilderOrmExt {
    /// Registers model `M` with the app, so commands and tooling find it
    /// (`model:show`). Rust cannot discover types at run time: a module
    /// lists its models once. Imports [`OrmModule`].
    fn model<M: Model>(&mut self) -> &mut Self;
}

impl ModuleBuilderOrmExt for ModuleBuilder {
    fn model<M: Model>(&mut self) -> &mut Self {
        self.import(OrmModule)
            .contribute(RegisteredModel::of::<M>())
    }
}

/// Describes model `M` with the hooks the app registers for it.
type Describe = fn(AppContext) -> BoxFuture<'static, Result<ModelDescription, BoxError>>;

/// A model a module registered (see [`ModuleBuilderOrmExt::model`]).
#[derive(Clone, Copy)]
pub struct RegisteredModel {
    meta: fn() -> &'static ModelMeta,
    describe: Describe,
}

impl RegisteredModel {
    /// The registration of model `M`.
    pub fn of<M: Model>() -> Self {
        RegisteredModel {
            meta: M::meta,
            describe: describe::<M>,
        }
    }

    /// The model's metadata.
    pub fn meta(&self) -> &'static ModelMeta {
        (self.meta)()
    }

    /// The model's name, e.g. `Deal`.
    pub fn name(&self) -> &'static str {
        self.meta().name()
    }

    /// The model as `model:show` shows it, with the hooks `app` registers
    /// for it.
    pub async fn describe(&self, app: &AppContext) -> Result<ModelDescription, BoxError> {
        (self.describe)(app.clone()).await
    }
}

impl fmt::Debug for RegisteredModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RegisteredModel")
            .field(&self.name())
            .finish()
    }
}

/// `M`'s description, with the hooks `app` registers for it.
fn describe<M: Model>(app: AppContext) -> BoxFuture<'static, Result<ModelDescription, BoxError>> {
    Box::pin(async move {
        let registered = app.get_all::<dyn ModelHook<M>>().await?;
        let app_hooks = registered.iter().map(|hook| hook.name()).collect();
        Ok(ModelDescription::of::<M>(app_hooks))
    })
}

/// The models the app's modules registered, in module dependency order
/// (each once).
pub fn registered_models(app: &AppContext) -> Vec<RegisteredModel> {
    let mut models: Vec<RegisteredModel> = Vec::new();
    for registration in app.contributions::<RegisteredModel>() {
        let model = *registration.value;
        if !models
            .iter()
            .any(|known| std::ptr::eq(known.meta(), model.meta()))
        {
            models.push(model);
        }
    }
    models
}
