//! The API's deal shape: [`DealResource`] with its [`OwnerRef`] embed.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use keel::auth::{User, user};
use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect};
use serde::Serialize;
use uuid::Uuid;

use super::entity::{self as deal, Stage};

/// A deal as the API returns it.
#[derive(Clone, Debug, Serialize)]
pub struct DealResource {
    /// The id.
    pub id: Uuid,
    /// The title.
    pub title: String,
    /// The value in cents.
    pub amount_cents: i64,
    /// The stage.
    pub stage: Stage,
    /// The owner.
    pub owner: OwnerRef,
    /// When it was created.
    pub created_at: DateTime<Utc>,
}

impl DealResource {
    /// The resource of `deal`, owned by `owner`.
    pub fn new(deal: deal::Model, owner: OwnerRef) -> Self {
        Self {
            id: deal.id,
            title: deal.title,
            amount_cents: deal.amount_cents,
            stage: deal.stage,
            owner,
            created_at: deal.created_at,
        }
    }

    /// The resources of `deals`, in order, with their owners read through
    /// `conn` in one query.
    pub async fn load_many<C: ConnectionTrait>(
        conn: &C,
        deals: Vec<deal::Model>,
    ) -> Result<Vec<DealResource>, DbErr> {
        let owners = OwnerRef::load(conn, deals.iter().map(|deal| deal.owner_id)).await?;
        Ok(deals
            .into_iter()
            .map(|deal| {
                // The foreign key keeps the owner; a missing row (never
                // expected) shows with an empty name.
                let owner = owners.get(&deal.owner_id).cloned().unwrap_or(OwnerRef {
                    id: deal.owner_id,
                    name: String::new(),
                });
                DealResource::new(deal, owner)
            })
            .collect())
    }
}

/// A user as embedded in a deal: `{id, name}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnerRef {
    /// The user's id.
    pub id: Uuid,
    /// The user's name.
    pub name: String,
}

impl OwnerRef {
    /// The users with `ids`, by id: one `SELECT id, name ... WHERE id IN
    /// (..)` for the distinct ids (none for an empty list).
    pub async fn load<C: ConnectionTrait>(
        conn: &C,
        ids: impl IntoIterator<Item = Uuid>,
    ) -> Result<HashMap<Uuid, OwnerRef>, DbErr> {
        let ids: BTreeSet<Uuid> = ids.into_iter().collect();
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<(Uuid, String)> = user::Entity::find()
            .select_only()
            .column(user::Column::Id)
            .column(user::Column::Name)
            .filter(user::Column::Id.is_in(ids))
            .into_tuple()
            .all(conn)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name)| (id, OwnerRef { id, name }))
            .collect())
    }
}

impl From<&User> for OwnerRef {
    fn from(user: &User) -> Self {
        OwnerRef {
            id: user.id,
            name: user.name.clone(),
        }
    }
}
