//! `crm-api`: runs the Navis CRM API. Run it from `apps/crm-api` (it reads
//! `.env` and `config/app.toml` from the working directory):
//!
//! ```text
//! cargo run -p crm-api -- migrate:fresh --seed   # schema + demo data
//! cargo run -p crm-api -- serve                  # http://127.0.0.1:8080
//! cargo run -p crm-api -- queue:work             # queued mail and notifications
//! cargo run -p crm-api -- schedule:work          # daily reminders, audit pruning
//! cargo run -p crm-api -- list                   # every command
//! ```
#![forbid(unsafe_code)]

use navis::prelude::*;

fn main() -> Result<(), BoxError> {
    crm_api::app(ConfigSources::defaults()).run()
}
