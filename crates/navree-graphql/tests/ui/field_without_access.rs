use navree_macros::{injectable, resolver};

#[injectable]
struct DealResolver;

#[resolver]
impl DealResolver {
    #[query]
    #[public]
    async fn hello(&self) -> String {
        "hello".to_owned()
    }

    #[query]
    async fn secret(&self) -> String {
        "secret".to_owned()
    }
}

fn main() {}
