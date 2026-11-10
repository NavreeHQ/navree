use navree_macros::{injectable, resolver};

#[injectable]
struct DealResolver;

#[resolver]
#[public]
impl DealResolver {
    #[query]
    #[roles("admin")]
    async fn secret(&self) -> String {
        "secret".to_owned()
    }
}

fn main() {}
