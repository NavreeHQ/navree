//! Shapes the framework's routes and lists share, as component schemas: [`PageMeta`],
//! [`AbilityRule`], and the access module's [`AccessRole`] and [`AccessUserRoles`].

use navree_openapi::{Schema, ToSchema};
use serde::{Deserialize, Serialize};

/// The pagination metadata of a numbered page: `{current_page, per_page, total, last_page}`.
///
/// The `meta` of every `Page<T>` navree-db returns has this shape. A page schema can refer to it
/// (`pub meta: PageMeta`) instead of describing the four numbers again, so a client has one type
/// for them. Register it with
/// [`api_schema`](crate::ModuleBuilderApiExt::api_schema) when a page schema refers to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PageMeta {
    /// The page number (1-based).
    pub current_page: u64,
    /// The page size.
    pub per_page: u64,
    /// The number of rows that match.
    pub total: u64,
    /// The last page number (at least 1).
    pub last_page: u64,
}

/// One rule of the signed-in user's abilities, as CASL's `createMongoAbility` takes it
/// (`GET /api/me/abilities` answers with a list of them).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AbilityRule {
    /// What the rule is about: an action (`read`, `update`, `manage`), or several.
    #[schema(schema_with = text_or_list)]
    pub action: Vec<String>,
    /// What it applies to: a subject type (`Deal`, `all`), or several.
    #[schema(schema_with = text_or_list)]
    pub subject: Vec<String>,
    /// Limits the rule to records that match (the MongoDB query language).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = conditions)]
    pub conditions: Option<serde_json::Value>,
    /// The fields the rule covers, when it covers some of them.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = text_or_list)]
    pub fields: Option<Vec<String>>,
    /// Whether the rule takes the ability away (`cannot`). Left out, never null, when it
    /// doesn't.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub inverted: Option<bool>,
    /// Why the rule exists, for the message of a refusal. Left out, never null, when
    /// there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub reason: Option<String>,
}

/// A role with the permissions it grants, as the access module's role routes
/// (`/api/access/roles`) answer with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AccessRole {
    /// The id.
    #[schema(schema_with = uuid)]
    pub id: String,
    /// The unique name (`manager`).
    pub name: String,
    /// What the role is for.
    pub description: Option<String>,
    /// The permissions the role grants, sorted (`deals.manage`).
    pub permissions: Vec<String>,
    /// When the role was created.
    #[schema(schema_with = date_time)]
    pub created_at: String,
}

/// A user's roles, as `PUT /api/access/users/{user}/roles` answers with them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AccessUserRoles {
    /// The user.
    #[schema(schema_with = uuid)]
    pub user_id: String,
    /// The user's roles now, sorted.
    pub roles: Vec<String>,
}

/// A UUID.
fn uuid() -> Schema {
    Schema::string().format("uuid")
}

/// An RFC 3339 date and time.
fn date_time() -> Schema {
    Schema::string().format("date-time")
}

/// A string, or a list of strings.
fn text_or_list() -> Schema {
    Schema::one_of(vec![Schema::string(), Schema::array(Schema::string())])
}

/// An object of query conditions.
fn conditions() -> Schema {
    Schema::object()
        .description("Conditions on the record's fields, in the MongoDB query language.")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn page_meta_has_the_four_numbers() {
        let schema = serde_json::to_value(PageMeta::schema()).unwrap();
        assert_eq!(
            schema["required"],
            json!(["current_page", "per_page", "total", "last_page"])
        );
        assert_eq!(PageMeta::name(), "PageMeta");
    }

    #[test]
    fn an_ability_rule_takes_one_or_several_actions_and_subjects() {
        let schema = serde_json::to_value(AbilityRule::schema()).unwrap();
        assert_eq!(schema["required"], json!(["action", "subject"]));
        assert_eq!(schema["properties"]["action"]["oneOf"][0]["type"], "string");
        assert_eq!(schema["properties"]["action"]["oneOf"][1]["type"], "array");
        assert_eq!(schema["properties"]["conditions"]["type"], "object");
        assert!(schema["properties"].get("inverted").is_some());
        assert!(schema["properties"].get("reason").is_some());
    }
}
