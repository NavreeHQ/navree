//! `evidence-app`: the Navree evidence app. One binary, three process types
//! in the e2e harness (`e2e/docker/compose.yml`):
//!
//! ```text
//! evidence-app serve            # web: HTTP, WebSockets, the UI tools
//! evidence-app queue:work       # worker: queued jobs, search sync, mail
//! evidence-app schedule:work    # scheduler: scheduled tasks
//! evidence-app migrate:fresh --seed   # schema + the evidence data
//! ```
#![forbid(unsafe_code)]

use navree::prelude::*;

fn main() -> Result<(), BoxError> {
    evidence_app::app(ConfigSources::defaults()).run()
}
