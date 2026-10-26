//! `contacts`: people, optionally at a company. Soft-deleted
//! (`deleted_at`); lists hide deleted rows.

use sea_orm::entity::prelude::*;
use serde::Serialize;

use crate::enums::ContactStatus;

/// A contact.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "contacts")]
pub struct Model {
    /// A UUID v7.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The first name (required).
    pub first_name: String,
    /// The last name (required).
    pub last_name: String,
    /// The email address.
    pub email: Option<String>,
    /// A phone number.
    pub phone: Option<String>,
    /// The job title.
    pub title: Option<String>,
    /// The life-cycle status (default `lead`).
    pub status: ContactStatus,
    /// The company they work at.
    pub company_id: Option<Uuid>,
    /// The user who owns the contact.
    pub owner_id: Uuid,
    /// When it was created.
    pub created_at: DateTimeUtc,
    /// When it last changed.
    pub updated_at: DateTimeUtc,
    /// When it was deleted (soft delete); `None` while it exists.
    pub deleted_at: Option<DateTimeUtc>,
}

impl Model {
    /// `"<first> <last>"`: the API's `full_name`.
    pub fn full_name(&self) -> String {
        full_name(&self.first_name, &self.last_name)
    }
}

/// `"<first> <last>"`, trimmed (either part may be empty).
pub fn full_name(first_name: &str, last_name: &str) -> String {
    format!("{} {}", first_name.trim(), last_name.trim())
        .trim()
        .to_owned()
}

/// A contact may work at a company and have deals; the owner is a user.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// The company (`company_id` → `companies.id`).
    #[sea_orm(
        belongs_to = "super::company::Entity",
        from = "Column::CompanyId",
        to = "super::company::Column::Id"
    )]
    Company,
    /// Deals where this is the contact.
    #[sea_orm(has_many = "super::deal::Entity")]
    Deals,
    /// The owner (`owner_id` → `users.id`).
    #[sea_orm(
        belongs_to = "navis::auth::user::Entity",
        from = "Column::OwnerId",
        to = "navis::auth::user::Column::Id"
    )]
    Owner,
}

impl Related<super::company::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Company.def()
    }
}

impl Related<super::deal::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Deals.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
