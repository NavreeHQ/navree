use navree_macros::{controller, injectable};

#[injectable]
struct DealController;

#[controller("/deals")]
#[authenticated]
#[requires_flag(NewPipeline)]
impl DealController {
    #[get("/forecast")]
    #[requires_flag(NewPipeline, status = 500)]
    async fn forecast(&self) -> &'static str {
        "forecast"
    }

    #[get("/board")]
    #[requires_flag(NewPipeline, code = 403)]
    async fn board(&self) -> &'static str {
        "board"
    }

    #[get("/summary")]
    #[requires_flag]
    async fn summary(&self) -> &'static str {
        "summary"
    }

    #[get("/export")]
    #[requires_flag(NewPipeline)]
    #[requires_flag(DealExport)]
    async fn export(&self) -> &'static str {
        "export"
    }

    #[requires_flag(NewPipeline)]
    async fn helper(&self) -> &'static str {
        "helper"
    }
}

fn main() {}
