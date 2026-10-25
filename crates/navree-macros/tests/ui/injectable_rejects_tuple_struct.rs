use std::sync::Arc;

use navree_macros::injectable;

struct Repo;

#[injectable]
struct Service(Arc<Repo>);

fn main() {}
