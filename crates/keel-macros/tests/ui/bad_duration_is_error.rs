use keel_macros::{controller, injectable};

#[injectable]
struct ReportController;

#[controller("/reports")]
impl ReportController {
    #[get("/")]
    #[public]
    #[timeout("5 minutes")]
    async fn index(&self) -> &'static str {
        "index"
    }

    #[post("/")]
    #[public]
    #[throttle(limit = 10, per = "1d")]
    async fn store(&self) -> &'static str {
        "store"
    }

    #[delete("/")]
    #[public]
    #[timeout("0s")]
    async fn purge(&self) -> &'static str {
        "purge"
    }
}

fn main() {}
