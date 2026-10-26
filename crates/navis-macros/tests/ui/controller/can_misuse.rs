use navis_access::{Read, Subject, Update};
use navis_macros::{controller, injectable};

struct Deal;

impl Subject for Deal {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}

#[injectable]
struct DealController;

#[controller("/deals")]
#[can(Read, Deal)]
impl DealController {
    #[get("/")]
    async fn index(&self) -> &'static str {
        "index"
    }

    #[patch("/{id}")]
    #[can(Update, Deal, from = "id")]
    async fn update(&self) -> &'static str {
        "update"
    }

    #[put("/{id}")]
    #[can(Update)]
    async fn replace(&self) -> &'static str {
        "replace"
    }

    #[delete("/{id}")]
    #[can(Update, Deal, form = "id")]
    async fn destroy(&self) -> &'static str {
        "destroy"
    }
}

fn main() {}
