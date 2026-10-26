use std::sync::Arc;

use navis_macros::injectable;

struct Repo;

#[injectable]
struct Service {
    repo: Arc<Repo>,
    retries: u32,
}

fn main() {}
