//! # navree-plugin-geo
//!
//! Maps and places for a Navree app: a map component and a drop-point form field. It is the
//! repository's worked example of a plugin that ships components of its own, and it is built
//! from the public API of the `navree` crate alone.
//!
//! - [`MapView`] draws one place on a [Leaflet](https://leafletjs.com) map, with its name and
//!   point as text under it. A page that may not load the map's tiles shows the text and a
//!   fallback in the map's place.
//! - [`LOCATION`] is a [`FieldControl`](navree::ui::form::FieldControl): a text field for a
//!   point, a map to click on and a button that picks where the device is. A form names it with
//!   `#[form(control = navree_plugin_geo::LOCATION)]` and a table cell with `EditCell::control`.
//!   The server holds the point to the delivery area on every path it arrives by.
//! - [`GeoModule`] registers all of it: the component's files, the page sources it asks for, the
//!   `geo` settings ([`GeoOptions`]) and two routes under `/geo/`.
//!
//! The map's tiles come from a host the app allows for pictures. The plugin asks for
//! OpenStreetMap's, and the app says yes in its configuration:
//!
//! ```toml
//! [ui.policy.allow.geo]
//! img_src = ["https://tile.openstreetmap.org"]
//! ```
//!
//! Until it does, a map is not drawn and shows its fallback, and the boot says in its log what
//! the plugin asked for.
//!
//! ```no_run
//! use navree::prelude::*;
//! use navree_plugin_geo::GeoModule;
//!
//! fn main() -> Result<(), BoxError> {
//!     Navree::app()
//!         .module(ConfigModule::for_root(ConfigSources::defaults()))
//!         .module(HttpModule::from_config())
//!         .module(UiModule)
//!         .module(GeoModule)
//!         .run()
//! }
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use navree::config::{Config, ConfigSection, ModuleBuilderConfigExt as _};
use navree::http::ModuleBuilderHttpExt as _;
use navree::ui::{Directive, PageSources};
use navree::{Factory, Module, ModuleBuilder, Resolver};
use std::sync::Arc;

mod options;
mod places;
mod point;
mod routes;
pub mod ui;

pub use options::{Area, GeoOptions};
pub use places::nearest;
pub use point::{GeoPoint, ParsePointError};
pub use ui::location::{LOCATION, LocationControl};
pub use ui::map::{MAP, MapProps, MapView, Tiles};

/// The module of the plugin: add it to an app that has `UiModule`.
///
/// It says what the plugin is (`geo`, "Maps and places"), registers the `geo` settings, serves
/// the map component's script and stylesheet through Navree UI, asks for the tile host in the
/// page policy, and mounts `GET /geo/tiles.json` and `POST /geo/reverse`.
#[derive(Clone, Copy, Debug, Default)]
pub struct GeoModule;

impl Module for GeoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(navree::plugin_info!(title = "Maps and places"));
        // The settings are read by the factory below, so an app with no `ConfigModule` gets the
        // defaults; the section is registered so the configuration's tools know its keys.
        m.config_section::<GeoOptions>();
        m.factory(Factory::<GeoOptions>::new(|r: Resolver| async move {
            let options = match r.app().try_get::<Config>().await {
                Some(config) => config.section_or_default::<GeoOptions>(GeoOptions::KEY)?,
                None => GeoOptions::default(),
            };
            Ok(Arc::new(options))
        }));
        m.contribute(ui::map::MAP);
        // Asking allows nothing: the app's `ui.policy` section decides.
        m.contribute(PageSources::new().ask(
            Directive::ImgSrc,
            "https://tile.openstreetmap.org",
            "map tiles",
        ));
        m.controller::<routes::GeoController>();
    }
}
