// Pattern attributes outside #[message_handlers], route attributes and
// handler signatures it cannot call.
use navree_http::Result;
use navree_macros::{message_handlers, message_pattern};

pub struct Loose;

impl Loose {
    #[message_pattern("loose.get")]
    async fn get(&self) -> Result<()> {
        Ok(())
    }
}

pub struct Misused;

#[message_handlers]
#[public]
impl Misused {
    #[message_pattern("misused.throttled")]
    #[throttle(10, "1m")]
    async fn throttled(&self) -> Result<()> {
        Ok(())
    }

    #[message_pattern("misused.sync")]
    fn sync(&self) -> Result<()> {
        Ok(())
    }

    #[message_pattern("misused.owned")]
    async fn owned(self) -> Result<()> {
        Ok(())
    }

    #[timeout("2s")]
    async fn helper(&self) {}
}

fn main() {}
