//! The small records the API embeds in its resources (spec §7):
//! `owner: {id, name}`, `company: {id, name}`, `contact: {id, full_name}`.
//!
//! Each has a `load(conn, ids)` that reads every distinct id in one query
//! (unknown ids are left out), so a page of resources needs one query per
//! embed, not one per row:
//!
//! ```ignore
//! let owners = OwnerRef::load(&*tx, deals.iter().map(|d| d.owner_id)).await?;
//! let owner = owners.get(&deal.owner_id).cloned();
//! ```

use std::collections::{BTreeSet, HashMap};

use navis::auth::user;
use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::entities::{company, contact};

/// A user as embedded in a record (`owner`, `assignee`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct OwnerRef {
    /// The user's id.
    pub id: Uuid,
    /// The user's name.
    pub name: String,
}

/// A company as embedded in a contact or deal.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct CompanyRef {
    /// The company's id.
    pub id: Uuid,
    /// The company's name.
    pub name: String,
}

/// A contact as embedded in a deal.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct ContactRef {
    /// The contact's id.
    pub id: Uuid,
    /// `"<first> <last>"`.
    pub full_name: String,
}

/// The distinct ids, or `None` when there are none (no query needed).
fn distinct(ids: impl IntoIterator<Item = Uuid>) -> Option<Vec<Uuid>> {
    let ids: BTreeSet<Uuid> = ids.into_iter().collect();
    (!ids.is_empty()).then(|| ids.into_iter().collect())
}

impl OwnerRef {
    /// The users with `ids`, by id.
    pub async fn load<C: ConnectionTrait>(
        conn: &C,
        ids: impl IntoIterator<Item = Uuid>,
    ) -> Result<HashMap<Uuid, OwnerRef>, DbErr> {
        let Some(ids) = distinct(ids) else {
            return Ok(HashMap::new());
        };
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

impl CompanyRef {
    /// The companies with `ids` (soft-deleted ones included), by id.
    pub async fn load<C: ConnectionTrait>(
        conn: &C,
        ids: impl IntoIterator<Item = Uuid>,
    ) -> Result<HashMap<Uuid, CompanyRef>, DbErr> {
        let Some(ids) = distinct(ids) else {
            return Ok(HashMap::new());
        };
        let rows: Vec<(Uuid, String)> = company::Entity::find()
            .select_only()
            .column(company::Column::Id)
            .column(company::Column::Name)
            .filter(company::Column::Id.is_in(ids))
            .into_tuple()
            .all(conn)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name)| (id, CompanyRef { id, name }))
            .collect())
    }
}

impl From<&company::Model> for CompanyRef {
    fn from(company: &company::Model) -> Self {
        CompanyRef {
            id: company.id,
            name: company.name.clone(),
        }
    }
}

impl ContactRef {
    /// The contacts with `ids` (soft-deleted ones included), by id.
    pub async fn load<C: ConnectionTrait>(
        conn: &C,
        ids: impl IntoIterator<Item = Uuid>,
    ) -> Result<HashMap<Uuid, ContactRef>, DbErr> {
        let Some(ids) = distinct(ids) else {
            return Ok(HashMap::new());
        };
        let rows: Vec<(Uuid, String, String)> = contact::Entity::find()
            .select_only()
            .column(contact::Column::Id)
            .column(contact::Column::FirstName)
            .column(contact::Column::LastName)
            .filter(contact::Column::Id.is_in(ids))
            .into_tuple()
            .all(conn)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(id, first, last)| {
                let full_name = contact::full_name(&first, &last);
                (id, ContactRef { id, full_name })
            })
            .collect())
    }
}

impl From<&contact::Model> for ContactRef {
    fn from(contact: &contact::Model) -> Self {
        ContactRef {
            id: contact.id,
            full_name: contact.full_name(),
        }
    }
}

impl From<&user::Model> for OwnerRef {
    fn from(user: &user::Model) -> Self {
        OwnerRef {
            id: user.id,
            name: user.name.clone(),
        }
    }
}
