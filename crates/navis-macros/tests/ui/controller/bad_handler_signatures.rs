use navis_macros::{controller, injectable};

#[injectable]
struct DealController;

#[controller("/deals")]
#[public]
impl DealController {
    #[get("/a")]
    fn not_async(&self) -> &'static str {
        "a"
    }

    #[get("/b")]
    async fn mutable(&mut self) -> &'static str {
        "b"
    }

    #[get("/c")]
    async fn owned(self) -> &'static str {
        "c"
    }

    #[get("/d")]
    async fn no_receiver() -> &'static str {
        "d"
    }

    #[get("/e")]
    async fn generic<T: Send>(&self) -> &'static str {
        "e"
    }
}

fn main() {}
