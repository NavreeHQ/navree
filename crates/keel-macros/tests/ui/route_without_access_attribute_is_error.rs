use keel_macros::{controller, injectable};

#[injectable]
struct DealController;

#[controller("/deals")]
impl DealController {
    #[get("/")]
    #[public]
    async fn index(&self) -> &'static str {
        "index"
    }

    #[get("/{id}")]
    async fn show(&self) -> &'static str {
        "show"
    }
}

fn main() {}
