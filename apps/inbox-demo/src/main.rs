//! `inbox-demo`: a software company's support inbox. See the library's docs.
#![forbid(unsafe_code)]

use navree::prelude::BoxError;

fn main() -> Result<(), BoxError> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(8096);
    let url = std::env::var("INBOX_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://target/inbox-demo.db?mode=rwc".to_owned());
    inbox_demo::app(port, url, inbox_demo::Settings::from_env()).run()
}
