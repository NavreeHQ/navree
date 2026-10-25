use keel_macros::{controller, injectable};

#[injectable]
struct DealController;

#[controller("/deals")]
impl DealController {
    #[get("/")]
    #[public]
    #[roles("admin")]
    async fn index(&self) -> &'static str {
        "index"
    }
}

#[injectable]
struct OpenController;

#[controller("/open")]
#[public]
impl OpenController {
    #[get("/")]
    async fn index(&self) -> &'static str {
        "index"
    }

    #[post("/")]
    #[permissions("open.write")]
    async fn store(&self) -> &'static str {
        "store"
    }
}

#[injectable]
struct MixedController;

#[controller("/mixed")]
#[public]
#[authenticated]
impl MixedController {
    #[get("/")]
    async fn index(&self) -> &'static str {
        "index"
    }
}

fn main() {}
