//! Custom timestamp column for Agra warehouse receipts.

use chrono::{DateTime, Utc};
use navree::orm::{Field, Timestamp, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReceiptEpoch(pub i64);

impl Timestamp for ReceiptEpoch {
    fn from_now(now: DateTime<Utc>) -> Self {
        Self(now.timestamp())
    }

    fn is_empty(&self) -> bool {
        self.0 == 0
    }

    fn empty() -> Self {
        Self(0)
    }
}

pub fn touch_column<C: Timestamp + navree_orm::Timestamp>(
    column: &mut C,
    now: DateTime<Utc>,
) -> Value {
    *column = <C as Timestamp>::from_now(now);
    Value::Timestamp(now)
}
