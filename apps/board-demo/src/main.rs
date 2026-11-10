//! `board-demo`: a product team's board, cards that move by pointer, touch or
//! keys, checked by the server and live for everyone. See the library's docs.
#![forbid(unsafe_code)]

use navree::prelude::BoxError;

fn main() -> Result<(), BoxError> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(8097);
    let url = std::env::var("BOARD_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://target/board-demo.db?mode=rwc".to_owned());
    board_demo::app(port, url, board_demo::Settings::from_env()).run()
}
