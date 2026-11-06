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
/// Users (navree-auth's `users`).
pub const USER: &str = "User";

/// The schema of a resource's `__type`: a string that is always `subject`. Every resource says it
/// the same way, so a client types them alike (`"Deal"`, not `string`).
pub fn type_schema(subject: &'static str) -> navree::openapi::Schema {
    navree::openapi::Schema::string_enum([subject])
        .description(format!("Always `\"{subject}\"` (the CASL subject type)."))
}
