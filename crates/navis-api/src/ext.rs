//! [`ModuleBuilderApiExt`]: component schemas a module adds to the document.

use navis_http::{SchemaFn, schema_fn};
use navis_kernel::ModuleBuilder;
use utoipa::ToSchema;

/// API documentation registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderApiExt {
    /// Adds `T` (and the schemas it references) to the document's component
    /// schemas.
    ///
    /// Request and response schemas of routes are registered automatically.
    /// Register a type here when the document refers to it but no route
    /// uses it directly: utoipa does not collect the schemas of generic type
    /// arguments, so a `Page<Note>` response refers to a `Note` component
    /// that only this registration provides. The app refuses to boot while
    /// the document refers to a schema nothing registered.
    ///
    /// ```
    /// use navis_api::ModuleBuilderApiExt;
    /// use navis_kernel::{Module, ModuleBuilder};
    /// use utoipa::ToSchema;
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
