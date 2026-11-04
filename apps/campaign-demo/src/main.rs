//! `campaign-demo`: an email campaign's recipients, a recipient's timeline in a
//! sheet, and its activity live over server-sent events. See the library's docs.
#![forbid(unsafe_code)]

use navree::prelude::BoxError;

fn main() -> Result<(), BoxError> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(8095);
    let url = std::env::var("CAMPAIGN_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://target/campaign-demo.db?mode=rwc".to_owned());
    campaign_demo::app(port, url, campaign_demo::Settings::from_env()).run()
}
