//! Custom timestamp column for Agra warehouse receipts.

use navree::datetime::Timestamp;
use navree::orm::{Field, HasTimestamps, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReceiptEpoch(pub i64);

impl HasTimestamps for ReceiptEpoch {
    fn from_now(now: Timestamp) -> Self {
        Self(now.as_secs())
    }

    fn is_empty(&self) -> bool {
        self.0 == 0
    }

    fn empty() -> Self {
        Self(0)
    }
}

pub fn touch_column<C: HasTimestamps + navree_orm::HasTimestamps>(
    column: &mut C,
    now: Timestamp,
) -> Value {
    *column = <C as HasTimestamps>::from_now(now);
    Value::Timestamp(now)
}
