//! `agent-demo`: a shop's back office with an assistant. See the library's docs.
#![forbid(unsafe_code)]

use navree::prelude::BoxError;

fn main() -> Result<(), BoxError> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(8098);
    let url = std::env::var("AGENT_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://target/agent-demo.db?mode=rwc".to_owned());
    agent_demo::app(port, url, agent_demo::Settings::from_env()).run()
}
