//! `store-demo`: a shop's orders, inventory and activity, live over server-sent
//! events. See the library's docs.
#![forbid(unsafe_code)]

use navree::prelude::BoxError;

fn main() -> Result<(), BoxError> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(8095);
    let url = std::env::var("STORE_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://target/store-demo.db?mode=rwc".to_owned());
    store_demo::app(port, url, store_demo::Settings::from_env()).run()
}
