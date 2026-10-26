use loco_rs::prelude::*;

pub use super::_entities::world::{self, ActiveModel, Entity, Model};

/// `world` holds ids 1..=10,000.
pub const ROWS: i32 = 10_000;

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    /// # Errors
    /// `ModelError::EntityNotFound` when no row has this id.
    pub async fn find_by_id(db: &DatabaseConnection, id: i32) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .one(db)
            .await?
            .ok_or_else(|| ModelError::EntityNotFound)
    }

    /// One row, picked uniformly at random.
    ///
    /// # Errors
    /// On a database error.
    pub async fn find_random(db: &DatabaseConnection) -> ModelResult<Self> {
        let id = rand::random_range(1..=ROWS);
        Self::find_by_id(db, id).await
    }
}
