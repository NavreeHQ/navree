use navree_macros::{controller, injectable};

#[injectable]
struct DealController;

#[controller("/deals")]
#[authenticated]
#[timeout("5s")]
#[get("/")]
impl DealController {
    #[get("/")]
    #[post("/")]
    async fn index(&self) -> &'static str {
        "index"
    }

    #[get("/{id}")]
    #[timeout("1s")]
    #[timeout("2s")]
    #[name("deals.show")]
    #[name("deals.view")]
    async fn show(&self) -> &'static str {
        "show"
    }

    // Not a route: the helper attributes would be silently ignored.
    #[roles("admin")]
    #[transactional]
    fn helper(&self) {}
}

fn main() {}
