use keel_macros::{delete, get};

struct DealController;

impl DealController {
    #[get("/")]
    async fn index(&self) -> &'static str {
        "index"
    }
}

#[delete("/{id}")]
async fn destroy() {}

fn main() {
    let _ = DealController.index();
    let _ = destroy();
}
