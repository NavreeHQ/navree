use loco_rs::prelude::*;

use crate::views::home::MessageResponse;

#[debug_handler]
async fn json() -> Result<Response> {
    format::json(MessageResponse::new("Hello, World!"))
}

pub fn routes() -> Routes {
    Routes::new().add("/json", get(json))
}
