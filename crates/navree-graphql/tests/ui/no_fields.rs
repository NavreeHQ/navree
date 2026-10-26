use navree_macros::{injectable, resolver};

#[injectable]
struct DealResolver;

#[resolver]
impl DealResolver {
    fn helper(&self) {}
}

fn main() {}
