use navis_http::ValidSignature;
use navis_macros::{controller, injectable};

#[injectable]
struct DownloadController;

#[controller("/downloads")]
impl DownloadController {
    // A guard is not an access declaration: the route is still undeclared.
    #[get("/{file}")]
    #[guard(ValidSignature::guard())]
    async fn show(&self) -> &'static str {
        "file"
    }
}

fn main() {}
