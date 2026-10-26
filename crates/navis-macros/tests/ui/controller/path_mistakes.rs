use navis_macros::{controller, injectable};

#[injectable]
struct DealController;

#[controller("/deals")]
impl DealController {
    #[get("/:id")]
    #[public]
    async fn show(&self) -> &'static str {
        "show"
    }

    #[get("/files/*rest")]
    #[public]
    async fn file(&self) -> &'static str {
        "file"
    }

    #[get("/{id")]
    #[public]
    async fn broken(&self) -> &'static str {
        "broken"
    }

    #[patch("/{deal}")]
    #[can("update", "Deal", from = "id")]
    async fn update(&self) -> &'static str {
        "update"
    }
}

#[injectable]
struct TeamDealController;

// Parameters of the prefix count for `from`.
#[controller("/teams/{team}/deals")]
impl TeamDealController {
    #[delete("/{id}")]
    #[can("delete", "Team", from = "team")]
    async fn destroy(&self) -> &'static str {
        "destroy"
    }

    // An absolute path ignores the prefix.
    #[post("!/archive/{id}")]
    #[can("archive", "Team", from = "team")]
    async fn archive(&self) -> &'static str {
        "archive"
    }
}

fn main() {}
