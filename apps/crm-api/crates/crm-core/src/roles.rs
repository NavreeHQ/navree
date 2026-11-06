//! The CRM's roles. Roles live in navree-access's `roles` table;
//! the `crm-roles` seeder creates them and grants their permissions.

use std::fmt;
use std::str::FromStr;

use navree::openapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::enums::UnknownValue;

/// Everything, including users, roles and the audit log (the super-admin
/// role: `can manage all`, after every other rule).
pub const ADMIN: &str = "admin";
/// Every customer record, deal reassignment, contact export, reports.
pub const MANAGER: &str = "manager";
/// Reads everything, creates records, changes only what they own.
pub const REP: &str = "rep";

/// A CRM role, as `POST /api/v1/users` and `PATCH /api/v1/users/{id}` take
/// it (`"admin" | "manager" | "rep"`).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "lowercase")]
#[schema(as = Role)]
pub enum UserRole {
    /// [`ADMIN`].
    Admin,
    /// [`MANAGER`].
    Manager,
    /// [`REP`].
    Rep,
}

impl UserRole {
    /// Every role, most powerful first.
    pub const ALL: [UserRole; 3] = [UserRole::Admin, UserRole::Manager, UserRole::Rep];

    /// The role name stored in navree-access.
    pub const fn as_str(self) -> &'static str {
        match self {
            UserRole::Admin => ADMIN,
            UserRole::Manager => MANAGER,
            UserRole::Rep => REP,
        }
    }

    /// What the role is for (the role's description).
    pub const fn description(self) -> &'static str {
        match self {
            UserRole::Admin => "Everything, including users, roles and the audit log",
            UserRole::Manager => "All customer records, deal reassignment, contact export",
            UserRole::Rep => "Reads everything; creates records; changes what they own",
        }
    }

    /// The permissions the role grants (on top of the rules in
    /// [`abilities::define`](crate::abilities::define)).
    pub const fn permissions(self) -> &'static [&'static str] {
        match self {
            UserRole::Manager => &crate::permissions::MANAGER,
            UserRole::Admin | UserRole::Rep => &[],
        }
    }
}

impl fmt::Display for UserRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for UserRole {
    type Err = UnknownValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        UserRole::ALL
            .into_iter()
            .find(|role| role.as_str() == value)
            .ok_or_else(|| UnknownValue {
                kind: "role",
                value: value.to_owned(),
            })
    }
}
