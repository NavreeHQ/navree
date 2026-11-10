//! `system-app`: the Navree system app. One binary, three process types
//! in the e2e tests (`e2e/docker/compose.yml`):
//!
//! ```text
//! system-app serve            # web: HTTP, WebSockets, the UI tools
//! system-app queue:work       # worker: queued jobs, search sync, mail
//! system-app schedule:work    # scheduler: scheduled tasks
//! system-app migrate:fresh --seed   # schema + the system data
//! ```
#![forbid(unsafe_code)]

use navree::prelude::*;

fn main() -> Result<(), BoxError> {
    system_app::app(ConfigSources::defaults()).run()
}
