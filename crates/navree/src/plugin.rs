//! What a plugin names: its description, the files it ships, the report of
//! an app's plugins, and the extension traits a plugin's modules use
//! (`use navree::plugin::prelude::*;`).
//!
//! A plugin can also add a driver under a name of its own for each driver
//! kind (`m.cache_store`, and the methods beside it), with what it needs in
//! [`DriverName`] and [`DriverContext`].
//!
//! ```no_run
//! use navree::prelude::*;
//! use navree::plugin::prelude::*;
//!
//! /// Notices for every member of a workspace.
//! pub struct AnnouncementsModule;
//!
//! impl Module for AnnouncementsModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.plugin(navree::plugin_info!(title = "Announcements"));
//!     }
//! }
//!
//! fn main() -> Result<(), BoxError> {
//!     Navree::app().module(AnnouncementsModule).run()
//! }
//! ```

pub use navree_config::ClientModule;
pub use navree_config::{DriverContext, DriverName};
pub use navree_contracts::EmbeddedLocales;
/// A permission a module describes with `m.permissions(..)`; describing grants it to nobody.
pub use navree_contracts::Permission;
pub use navree_kernel::{
    PluginInfo, PluginModuleReport, PluginReport, PluginsReport, PublishTag, Publishable,
};
/// What a plugin's UI components name: their files, their form field and the page sources they ask for.
#[cfg(feature = "ui")]
pub use navree_ui::{
    Control, ControlCx, ControlField, Directive, EditedValue, ExtensionFile, FieldControl,
    PagePolicy, PageSources, UiExtension,
};
#[cfg(feature = "view")]
pub use navree_view::TemplateDir;

/// The `ModuleBuilder` extension traits of plugins, imported as `_`. They are
/// not in `navree::prelude`, so a glob import of it never makes an app's own
/// method of the same name ambiguous.
pub mod prelude {
    #[cfg(feature = "ai")]
    pub use navree_ai::ModuleBuilderAiProviderExt as _;
    #[cfg(feature = "cache")]
    pub use navree_cache::ModuleBuilderCacheExt as _;
    pub use navree_contracts::ModuleBuilderLocalesExt as _;
    pub use navree_contracts::ModuleBuilderPermissionsExt as _;
    #[cfg(feature = "grpc")]
    pub use navree_grpc::ModuleBuilderTonicExt as _;
    #[cfg(feature = "helm")]
    pub use navree_helm::ModuleBuilderHelmPanelExt as _;
    #[cfg(feature = "http")]
    pub use navree_http::ModuleBuilderMountExt as _;
    #[cfg(feature = "mail")]
    pub use navree_mail::ModuleBuilderMailTransportExt as _;
    #[cfg(feature = "notify")]
    pub use navree_notify::ModuleBuilderNotifyChannelsExt as _;
    #[cfg(feature = "queue")]
    pub use navree_queue::ModuleBuilderQueueExt as _;
    #[cfg(feature = "realtime")]
    pub use navree_realtime::ModuleBuilderBackplaneExt as _;
    #[cfg(feature = "search")]
    pub use navree_search::ModuleBuilderSearchExt as _;
    #[cfg(feature = "session")]
    pub use navree_session::ModuleBuilderSessionExt as _;
    #[cfg(feature = "storage")]
    pub use navree_storage::ModuleBuilderStorageExt as _;
    #[cfg(feature = "ui")]
    pub use navree_ui::ModuleBuilderShellExt as _;
    #[cfg(feature = "view")]
    pub use navree_view::ModuleBuilderPluginTemplatesExt as _;
}
