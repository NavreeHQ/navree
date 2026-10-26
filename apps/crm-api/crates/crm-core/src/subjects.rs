//! The access subjects (CASL subject types) of the CRM's resources and
//! their permission prefixes (`deals.manage` → `manage` on `Deal`).

/// Companies.
pub const COMPANY: &str = "Company";
/// Contacts.
pub const CONTACT: &str = "Contact";
/// Deals.
pub const DEAL: &str = "Deal";
/// Activities (notes, calls, emails, meetings, tasks).
pub const ACTIVITY: &str = "Activity";
/// Users (navis-auth's `users`).
pub const USER: &str = "User";
