//! The API's deal shape: [`DealResource`] with its [`OwnerRef`] embed.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use navree::auth::User;
use sea_orm::sea_query::{Alias, Expr, ExprTrait, Query};
use sea_orm::{ConnectionTrait, DbErr};
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
        let select = Query::select()
            .columns([Alias::new("id"), Alias::new("name")])
            .from(Alias::new("users"))
            .and_where(Expr::col(Alias::new("id")).is_in(ids))
            .to_owned();
        let rows = conn
            .query_all(conn.get_database_backend().build(&select))
            .await?;
        rows.iter()
            .map(|row| {
                let id: Uuid = row.try_get("", "id")?;
                let name: String = row.try_get("", "name")?;
                Ok((id, OwnerRef { id, name }))
            })
            .collect()
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
