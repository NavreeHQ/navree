use std::sync::Arc;

use navree_macros::injectable;

struct Repo;

#[injectable]
struct Service {
    #[inject(optional)]
    repo: Arc<Repo>,
}

fn main() {}
