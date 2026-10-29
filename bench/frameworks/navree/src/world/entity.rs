//! [`World`] (`world`): 10,000 rows of `(id, randomnumber)`.

use navree::orm::prelude::*;
use serde::Serialize;

/// A row, serialized as `{"id": 3217, "randomNumber": 4512}`.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "world")]
pub struct World {
    /// 1..=10000.
    pub id: i32,
    /// 1..=10000.
    #[model(column = "randomnumber")]
    #[serde(rename = "randomNumber")]
    pub random_number: i32,
}
