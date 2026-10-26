use navis_macros::{controller, injectable};

#[injectable]
struct DealController;

#[controller("/deals")]
#[public]
impl DealController {
    #[get("/")]
    async fn index(&self) -> &'static str {
        "index"
    }

    #[get("")]
    async fn list(&self) -> &'static str {
        "list"
    }

    #[post("/")]
    async fn store(&self) -> &'static str {
        "store"
    }

    #[get("/{id}")]
    async fn show(&self) -> &'static str {
        "show"
    }

    #[get("/{deal}")]
    async fn show_again(&self) -> &'static str {
        "show"
    }

    // Mutually exclusive `cfg`s: not a duplicate.
    #[get("/export")]
    #[cfg(all())]
    async fn export(&self) -> &'static str {
        "export"
    }

    #[get("/export")]
    #[cfg(not(all()))]
    async fn export_legacy(&self) -> &'static str {
        "export"
    }
}

fn main() {}
