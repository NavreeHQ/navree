use navree_macros::{injectable, resolver};

#[injectable]
struct DealResolver;

#[resolver]
#[query]
impl DealResolver {
    #[query]
    #[mutation]
    #[public]
    async fn twice(&self) -> String {
        "twice".to_owned()
    }

    #[public]
    fn helper(&self) {}

    #[query]
    #[public]
    fn not_async(&self) -> String {
        String::new()
    }

    #[query]
    #[public]
    async fn nothing(&self) {}
}

fn main() {}
