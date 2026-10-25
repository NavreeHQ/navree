use std::sync::Arc;

use navree_macros::injectable;

struct Repo;

#[injectable]
struct Service {
    repo: Arc<Repo>,
    retries: u32,
}

fn main() {}
