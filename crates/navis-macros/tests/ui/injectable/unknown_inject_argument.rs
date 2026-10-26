use std::sync::Arc;

use navis_macros::injectable;

struct Repo;

#[injectable]
struct Service {
    #[inject(optional)]
    repo: Arc<Repo>,
}

fn main() {}
