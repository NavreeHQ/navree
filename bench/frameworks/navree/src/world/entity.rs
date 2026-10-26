//! `world`: 10,000 rows of `(id, randomnumber)`.

use sea_orm::entity::prelude::*;
use serde::Serialize;

/// A row, serialized as `{"id": 3217, "randomNumber": 4512}`.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "world")]
pub struct Model {
    /// 1..=10000.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
    /// 1..=10000.
    #[sea_orm(column_name = "randomnumber")]
    #[serde(rename = "randomNumber")]
    pub random_number: i32,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
