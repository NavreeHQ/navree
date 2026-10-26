// Invalid patterns and options, and two handlers for one pattern.
use navree_http::Result;
use navree_macros::message_handlers;

pub struct Patterns;

#[message_handlers]
#[public]
impl Patterns {
    #[message_pattern("deals.*")]
    async fn wildcard(&self) -> Result<()> {
        Ok(())
    }

    #[message_pattern("Deals.Get")]
    async fn upper(&self) -> Result<()> {
        Ok(())
    }

    #[message_pattern("deals.retried", max_attempts = 3)]
    async fn retried(&self) -> Result<()> {
        Ok(())
    }

    #[event_pattern("deal.deduped", dedupe)]
    async fn deduped(&self) -> Result<()> {
        Ok(())
    }

    #[event_pattern("deal.both", ordered, broadcast)]
    async fn both(&self) -> Result<()> {
        Ok(())
    }

    #[event_pattern("deal.zero", concurrency = 0)]
    async fn zero(&self) -> Result<()> {
        Ok(())
    }

    #[message_pattern("deals.twice")]
    async fn first(&self) -> Result<()> {
        Ok(())
    }

    #[message_pattern("deals.twice")]
    async fn second(&self) -> Result<()> {
        Ok(())
    }
}

fn main() {}
