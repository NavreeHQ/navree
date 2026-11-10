//! [`ModuleBuilderApiExt`]: component schemas a module adds to the document.

use navree_http::{SchemaFn, schema_fn};
use navree_kernel::ModuleBuilder;
use navree_openapi::ToSchema;

use crate::route_doc::{ApiRouteDoc, ApiSecurity, RouteDoc};

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

    /// Adds what a route's `#[api]` metadata cannot say: its query parameters (the list query of a
    /// route that takes `List<R>`), a response that is not JSON, another success status, or the
    /// security it needs. See [`RouteDoc`].
    ///
    /// A doc that names a route the app does not have stops the boot, so a renamed route cannot
    /// leave its description behind.
    ///
    /// ```
    /// use navree_api::{ModuleBuilderApiExt, RouteDoc};
    /// use navree_kernel::{Module, ModuleBuilder};
    ///
    /// struct ContactsModule;
    ///
    /// impl Module for ContactsModule {
    ///     fn register(&self, m: &mut ModuleBuilder) {
    ///         m.api_route(
    ///             RouteDoc::get("/api/v1/contacts/export")
    ///                 .text_response(200, "text/csv", "The contacts, one row each."),
    ///         );
    ///     }
    /// }
    /// ```
    fn api_route(&mut self, doc: RouteDoc) -> &mut Self;

    /// The security schemes this app uses: `cookie` (the session cookie), `bearer` (a JWT access
    /// token), or both, which is what the document says without this. An app whose clients all
    /// send a bearer token says `api_security(&["bearer"])`, and the document then lists no
    /// cookie scheme, on any route. Several modules may call it; the app uses the union.
    fn api_security(&mut self, schemes: &[&str]) -> &mut Self;
}

impl ModuleBuilderApiExt for ModuleBuilder {
    fn api_schema<T: ToSchema>(&mut self) -> &mut Self {
        self.contribute(ApiSchema(schema_fn::<T>))
    }

    fn api_route(&mut self, doc: RouteDoc) -> &mut Self {
        self.contribute(ApiRouteDoc(doc))
    }

    fn api_security(&mut self, schemes: &[&str]) -> &mut Self {
        self.contribute(ApiSecurity(
            schemes.iter().map(|scheme| (*scheme).to_owned()).collect(),
        ))
    }
}

/// A schema contributed with [`ModuleBuilderApiExt::api_schema`].
#[derive(Clone, Copy)]
pub(crate) struct ApiSchema(pub(crate) SchemaFn);
