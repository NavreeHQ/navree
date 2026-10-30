//! Permission strings (`<prefix>.<action>`, navree-access) the CRM grants
//! through roles. A permission maps to a rule on its prefix's subject:
//! `deals.manage` → `can manage Deal`.

/// Manage every company.
pub const COMPANIES_MANAGE: &str = "companies.manage";
/// Manage every contact.
pub const CONTACTS_MANAGE: &str = "contacts.manage";
/// Export contacts as CSV.
pub const CONTACTS_EXPORT: &str = "contacts.export";
/// Manage every deal (including reassigning it).
pub const DEALS_MANAGE: &str = "deals.manage";
/// Manage every activity.
pub const ACTIVITIES_MANAGE: &str = "activities.manage";
/// Read users.
pub const USERS_READ: &str = "users.read";

/// What the `manager` role grants.
pub const MANAGER: [&str; 6] = [
    COMPANIES_MANAGE,
    CONTACTS_MANAGE,
    CONTACTS_EXPORT,
    DEALS_MANAGE,
    ACTIVITIES_MANAGE,
    USERS_READ,
];
