use std::sync::Arc;

use keel_macros::injectable;

struct Repo;

#[injectable]
struct Service(Arc<Repo>);

fn main() {}
