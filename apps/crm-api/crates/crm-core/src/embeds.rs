//! The small records the API embeds in its resources (spec §7):
//! `owner: {id, name}`, `company: {id, name}`, `contact: {id, full_name}`.
//!
//! They come from relations eager loaded with the records, one query per
//! relation for a whole page, never one per row:
//!
//! ```no_run
//! # use crm_core::OwnerRef;
//! # use crm_core::models::Deal;
//! # use navree::db::Uow;
//! # use navree::orm::Model;
//! # async fn demo(uow: Uow) -> navree::orm::Result<()> {
//! let deals = Deal::query().with_all(["company", "contact", "owner"]).get(&uow).await?;
//! let owner = OwnerRef::loaded(deals[0].loaded(Deal::owner), deals[0].owner_id);
//! # let _ = owner;
//! # Ok(()) }
//! ```

use navree::orm::Record;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::models::{Company, Contact, User};

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

impl OwnerRef {
    /// The embed of user `id` from a record's loaded relation
    /// (`deal.loaded(Deal::owner)`). Foreign keys keep owners, so a user
    /// is never missing; if one were (or the relation was not loaded), the
    /// embed has an empty name rather than failing the page.
    pub fn loaded(user: Option<&Option<Record<User>>>, id: Uuid) -> Self {
        user.and_then(Option::as_ref).map_or_else(
            || OwnerRef {
                id,
                name: String::new(),
            },
            OwnerRef::from,
        )
    }
}

impl CompanyRef {
    /// The embed of company `id` from a record's loaded relation
    /// (`deal.loaded(Deal::company)`); an empty name if it is missing.
    pub fn loaded(company: Option<&Option<Record<Company>>>, id: Uuid) -> Self {
        company.and_then(Option::as_ref).map_or_else(
            || CompanyRef {
                id,
                name: String::new(),
            },
            CompanyRef::from,
        )
    }
}

impl From<&Record<User>> for OwnerRef {
    fn from(user: &Record<User>) -> Self {
        OwnerRef {
            id: user.id,
            name: user.name.clone(),
        }
    }
}

impl From<&Record<Company>> for CompanyRef {
    fn from(company: &Record<Company>) -> Self {
        CompanyRef {
            id: company.id,
            name: company.name.clone(),
        }
    }
}

impl From<&Record<Contact>> for ContactRef {
    fn from(contact: &Record<Contact>) -> Self {
        ContactRef {
            id: contact.id,
            full_name: contact.full_name(),
        }
    }
}

/// The entity-based batch loaders the feature crates use until they read
/// through the models.
mod entity_loaders {
    use std::collections::{BTreeSet, HashMap};

    use navree::auth::user;
    use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect};
    use uuid::Uuid;

    use super::{CompanyRef, ContactRef, OwnerRef};
    use crate::entities::{company, contact};

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
}
