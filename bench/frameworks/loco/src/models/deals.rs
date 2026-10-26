use loco_rs::{
    model::query::{self, PageResponse, PaginationQuery},
    prelude::*,
    validator::ValidationError,
};
use serde::Deserialize;

pub use super::_entities::deals::{self, ActiveModel, Entity, Model};
use super::users;

pub const STAGES: [&str; 6] = ["lead", "qualified", "proposal", "negotiation", "won", "lost"];

/// The body of `POST /api/deals`.
#[derive(Debug, Deserialize, Validate)]
pub struct CreateParams {
    #[validate(length(min = 1, max = 200, message = "title must be 1 to 200 characters"))]
    pub title: String,
    #[validate(range(
        min = 0,
        max = 1_000_000_000_000_i64,
        message = "amount_cents must be between 0 and 1000000000000"
    ))]
    pub amount_cents: i64,
    #[validate(custom(function = "validate_stage"))]
    pub stage: String,
}

fn validate_stage(stage: &str) -> Result<(), ValidationError> {
    if STAGES.contains(&stage) {
        Ok(())
    } else {
        Err(ValidationError::new("stage")
            .with_message("stage must be one of lead, qualified, proposal, negotiation, won, lost".into()))
    }
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    /// A new deal gets its id and timestamps here; an update bumps `updated_at`.
    async fn before_save<C>(self, _db: &C, insert: bool) -> std::result::Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        let mut this = self;
        let now: DateTimeWithTimeZone = chrono::Utc::now().into();
        if insert {
            if this.id.is_not_set() {
                this.id = ActiveValue::Set(Uuid::new_v4());
            }
            this.created_at = ActiveValue::Set(now);
            this.updated_at = ActiveValue::Set(now);
        } else if this.updated_at.is_unchanged() {
            this.updated_at = ActiveValue::Set(now);
        }
        Ok(this)
    }
}

impl Model {
    /// Counts all deals, then loads one page, newest first, with each deal's
    /// owner joined in (one query for the page).
    ///
    /// # Errors
    /// On a database error.
    pub async fn page_with_owners(
        db: &DatabaseConnection,
        pagination: &PaginationQuery,
    ) -> Result<PageResponse<(Self, Option<users::Model>)>> {
        let select = Entity::find()
            .find_also_related(users::Entity)
            .order_by_desc(deals::Column::CreatedAt)
            .order_by_desc(deals::Column::Id);
        query::fetch_page(db, select, pagination).await
    }

    /// Inserts a deal owned by `owner_id`.
    ///
    /// # Errors
    /// On a database error.
    pub async fn create(db: &DatabaseConnection, owner_id: Uuid, params: CreateParams) -> ModelResult<Self> {
        let deal = ActiveModel {
            title: ActiveValue::Set(params.title),
            amount_cents: ActiveValue::Set(params.amount_cents),
            stage: ActiveValue::Set(params.stage),
            owner_id: ActiveValue::Set(owner_id),
            ..Default::default()
        }
        .insert(db)
        .await?;
        Ok(deal)
    }
}
