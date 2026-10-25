//! `companies`: the organisations the team sells to. Soft-deleted
//! (`deleted_at`); lists hide deleted rows.

use sea_orm::entity::prelude::*;
use serde::Serialize;

use crate::enums::CompanySize;

/// A company.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "companies")]
pub struct Model {
    /// A UUID v7.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The name (required).
    pub name: String,
    /// The web domain, e.g. `acme.com`.
    pub domain: Option<String>,
    /// The industry, free text.
    pub industry: Option<String>,
    /// The head count bracket.
    pub size: Option<CompanySize>,
    /// A phone number.
    pub phone: Option<String>,
    /// The website URL.
    pub website: Option<String>,
    /// The city.
    pub city: Option<String>,
    /// The country.
    pub country: Option<String>,
    /// The user who owns the account.
    pub owner_id: Uuid,
    /// When it was created.
    pub created_at: DateTimeUtc,
    /// When it last changed.
    pub updated_at: DateTimeUtc,
    /// When it was deleted (soft delete); `None` while it exists.
    pub deleted_at: Option<DateTimeUtc>,
}

/// Contacts and deals belong to a company; the owner is a user.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// The people at the company.
    #[sea_orm(has_many = "super::contact::Entity")]
    Contacts,
    /// The company's deals.
    #[sea_orm(has_many = "super::deal::Entity")]
    Deals,
    /// The owner (`owner_id` → `users.id`).
    #[sea_orm(
        belongs_to = "navree::auth::user::Entity",
        from = "Column::OwnerId",
        to = "navree::auth::user::Column::Id"
    )]
    Owner,
}

impl Related<super::contact::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Contacts.def()
    }
}

impl Related<super::deal::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Deals.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
