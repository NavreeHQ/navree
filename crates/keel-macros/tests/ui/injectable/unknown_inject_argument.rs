use std::sync::Arc;

use keel_macros::injectable;

struct Repo;

#[injectable]
struct Service {
    #[inject(optional)]
    repo: Arc<Repo>,
}

fn main() {}
