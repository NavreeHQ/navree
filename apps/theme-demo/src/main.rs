//! `theme-demo`: the live theme builder at `/demo/ui/`, behind the site's reverse proxy.
//!
//! ```text
//! cargo run --release -p theme-demo        # http://127.0.0.1:8095/demo/ui/themes/builder
//! PORT=9000 theme-demo                     # another port
//! ```
//!
//! Every `APP_` variable of the environment overrides the defaults in
//! [`theme_demo::settings`].
#![forbid(unsafe_code)]

use navree::prelude::*;
use navree::ui::{PagesOptions, UiModule};
use theme_demo::{DemoModule, PORT, settings};

fn main() -> Result<(), BoxError> {
    let port = std::env::var("PORT").unwrap_or_else(|_| PORT.to_owned());
    let sources = ConfigSources::new().env_vars("APP_", settings(&port)).env();
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::from_config())
        .module(UiModule::with_pages(PagesOptions::default()))
        .module(DemoModule)
        .run()
}
