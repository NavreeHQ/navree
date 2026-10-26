use navree_graphql::Context;
use navree_macros::{injectable, resolver};

#[injectable]
struct DealResolver;

#[resolver]
impl DealResolver {
    #[query]
    #[can("read", "Deal", from = "deal_id")]
    async fn deal(&self, ctx: &Context<'_>, id: String) -> String {
        let _ = ctx;
        id
    }
}

fn main() {}
