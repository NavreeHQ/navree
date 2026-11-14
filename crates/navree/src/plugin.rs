//! What a plugin names: its description, the files it ships, the report of
//! an app's plugins, and the extension traits a plugin's modules use
//! (`use navree::plugin::prelude::*;`).
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

pub use navree_kernel::{
    PluginInfo, PluginModuleReport, PluginReport, PluginsReport, PublishTag, Publishable,
};

/// The `ModuleBuilder` extension traits of plugins, imported as `_`. They are
/// not in `navree::prelude`, so a glob import of it never makes an app's own
/// method of the same name ambiguous.
pub mod prelude {}
