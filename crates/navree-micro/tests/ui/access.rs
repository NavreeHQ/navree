// Message patterns are deny by default; events take no access declaration.
use navree_http::Result;
use navree_macros::message_handlers;

pub struct Deals;

#[message_handlers]
impl Deals {
    #[message_pattern("deals.get")]
    async fn get(&self) -> Result<()> {
        Ok(())
    }

    #[event_pattern("deal.won")]
    #[authenticated]
    async fn won(&self) -> Result<()> {
        Ok(())
    }

    #[message_pattern("deals.open")]
    #[public]
    #[roles("admin")]
    async fn open(&self) -> Result<()> {
        Ok(())
    }
}

pub struct Guarded;

#[message_handlers]
#[guard(std::sync::Arc::new(Always))]
impl Guarded {
    #[message_pattern("guarded.get")]
    async fn get(&self) -> Result<()> {
        Ok(())
    }
}

fn main() {}
