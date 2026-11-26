//! The `navree-plugin-godam` community plugin for Navree: registers the
//! `godam` storage disk driver over [`navree::storage::store::Store`].

use navree::plugin::prelude::*;
use navree::prelude::*;

pub mod disk;
pub mod settings;

pub use disk::GodamDisk;
pub use settings::GodamSettings;

/// Registers the `godam` plugin and its `godam` storage disk driver.
///
/// Any disk configured with `driver = "godam"` in `[storage.disks.NAME]` is
/// built from [`GodamSettings`] and served by [`GodamDisk`].
#[derive(Clone, Copy, Debug, Default)]
pub struct GodamModule;

impl GodamModule {
    /// Returns the module ready to register on an application builder.
    pub fn from_config() -> GodamModule {
        GodamModule
    }
}

impl Module for GodamModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(navree::plugin_info!(title = "Godam disks"));
        m.disk_driver("godam", |ctx| async move {
            let settings: GodamSettings = ctx.settings()?;
            settings.validate(ctx.instance())?;
            GodamDisk::new(settings)
        });
        // navree:wire
    }
}
