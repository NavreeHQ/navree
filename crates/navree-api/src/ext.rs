//! [`ModuleBuilderApiExt`]: component schemas a module adds to the document.

use navree_http::{SchemaFn, schema_fn};
use navree_kernel::ModuleBuilder;
use navree_openapi::ToSchema;

/// API documentation registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderApiExt {
    /// Adds `T` (and the schemas it references) to the document's component
    /// schemas.
    ///
    /// Request and response schemas of routes are registered automatically.
    /// Register a type here when the document refers to it but no route
    /// uses it directly: the derive does not collect the schemas of generic type
    /// arguments, so a `Page<Note>` response refers to a `Note` component
    /// that only this registration provides. The app refuses to boot while
    /// the document refers to a schema nothing registered.
    ///
    /// ```
    /// use navree_api::ModuleBuilderApiExt;
    /// use navree_kernel::{Module, ModuleBuilder};
    /// use navree_openapi::ToSchema;
    ///
    /// #[derive(ToSchema)]
    /// struct Note {
    ///     body: String,
    /// }
    ///
    /// struct NotesModule;
    ///
    /// impl Module for NotesModule {
    ///     fn register(&self, m: &mut ModuleBuilder) {
    ///         m.api_schema::<Note>();
    ///     }
    /// }
    /// ```
    fn api_schema<T: ToSchema>(&mut self) -> &mut Self;
}

impl ModuleBuilderApiExt for ModuleBuilder {
    fn api_schema<T: ToSchema>(&mut self) -> &mut Self {
        self.contribute(ApiSchema(schema_fn::<T>))
    }
}

/// A schema contributed with [`ModuleBuilderApiExt::api_schema`].
#[derive(Clone, Copy)]
pub(crate) struct ApiSchema(pub(crate) SchemaFn);
