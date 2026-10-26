use loco_rs::prelude::*;

pub use super::_entities::users::{self, ActiveModel, Entity, Model};

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    /// The user an access token names in `sub`.
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` when no user has this id.
    pub async fn find_by_id(db: &DatabaseConnection, id: Uuid) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .one(db)
            .await?
            .ok_or_else(|| ModelError::EntityNotFound)
    }
}
