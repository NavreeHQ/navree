use std::sync::Arc;

use navis_macros::injectable;

struct Repo;

#[injectable]
struct Service(Arc<Repo>);

fn main() {}
