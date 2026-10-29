//! The ability engine: rules in CASL's JSON format, record checks and SQL list
//! filters. Pure: no database connection, HTTP or module wiring (that is the
//! `AccessModule` half of the crate); list filters are navree-db values and
//! statements.
//!
//! - [`Rules`] collects rules in order with [`Rules::can`] / [`Rules::cannot`];
//!   [`Rules::build`] validates them into an [`Ability`].
//! - [`Ability::can`] answers "may this principal do `action` on this
//!   subject type (or this record)?" with CASL's semantics: the **last**
//!   matching rule wins, `manage` matches any action, `all` matches any
//!   subject, and no matching rule means deny.
//! - [`Ability::sql_filter`] compiles the same rules into one SQL `CASE`
//!   chain ([`SqlFilter`], written for a backend as a portable fragment by
//!   [`SqlFilter::to_statement`]), so a record check and a list filter never
//!   disagree.
//! - [`Ability::to_casl_json`] / [`Ability::from_casl_json`] speak CASL's JSON
//!   rule format, which `createMongoAbility` in `@casl/ability` accepts as is.
//!
//! # Semantics
//!
//! - **Order:** rules are scanned from last to first; the first rule whose
//!   action, subject type, field and conditions match decides.
//! - **Type-level checks** (no record) follow CASL: a conditional `can` rule
//!   matches ("some records may be allowed"), a conditional `cannot` rule is
//!   skipped. [`Ability::has_unconditional`] is the strict variant used for
//!   permission checks.
//! - **Fields:** a field-limited `cannot` rule never denies a whole-record
//!   check, only [`Ability::can_field`] for its fields (CASL behaviour).
//! - **Conditions** apply to top-level scalar fields with `$eq`, `$ne`, `$lt`,
//!   `$lte`, `$gt`, `$gte`, `$in` and `$nin`; anything else (`$exists`,
//!   `$regex`, `$or`, nested `a.b` fields, field patterns) is rejected when
//!   the rules are built or parsed. `$eq: null` matches null, `$ne` matches
//!   null fields (JavaScript `!==`, SQL `IS DISTINCT FROM`), and `$in` /
//!   `$nin` match null fields only through / unless the list has a `null`.
//! - **Types are not coerced:** `7` never equals `"7"`. Numbers compare by
//!   value (`1 == 1.0`), strings by code point. Integers must be
//!   JavaScript-safe (|n| ≤ 2^53 − 1); larger integers and decimals travel as
//!   strings. Timestamps are RFC 3339 UTC strings with fixed precision (see
//!   [`IntoCondValue`]), so text order is time order.
//!
//! Where this deliberately differs from `@casl/ability` in the browser (the
//! API always re-checks, so these only affect what the UI shows):
//!
//! - a field the record lacks counts as `null` (CASL sees `undefined`, which
//!   `$eq: null` does not match);
//! - range operators never match `null` or a value of another type
//!   (JavaScript would order `null` as `0` and coerce numeric strings);
//! - strings order by code point (JavaScript uses UTF-16 code units; they
//!   differ only between characters above U+FFFF and U+E000–U+FFFF).
//!
//! ```
//! use navree_access::engine::{field, Rules, DELETE, READ, UPDATE};
//! use serde_json::json;
//!
//! let user_id = 7;
//! let mut rules = Rules::new();
//! rules.can(READ, "Post").when(field("published").eq(true));
//! rules
//!     .can(UPDATE, "Post")
//!     .fields(["title", "body"])
//!     .when(field("author_id").eq(user_id));
//! rules.can(DELETE, "Post").when(field("author_id").eq(user_id));
//! rules.cannot(DELETE, "Post").when(field("locked").eq(true)); // last: last match wins
//! let ability = rules.build();
//!
//! let post = json!({ "id": 1, "author_id": 7, "published": false, "locked": true });
//! assert!(ability.can(UPDATE, "Post", Some(&post)));
//! assert!(!ability.can_field(UPDATE, "Post", Some(&post), "author_id"));
//! assert!(!ability.can(DELETE, "Post", Some(&post)));
//! assert!(ability.can(READ, "Post", None)); // type level: some posts are readable
//! ```

mod ability;
mod casl;
mod condition;
mod rule;
mod sql;

pub use ability::Ability;
pub use condition::{Cond, Conditions, FieldRef, IntoCondValue, field};
pub use rule::{Rule, RuleBuilder, RuleError, Rules};
pub use sql::{
    DefaultColumns, SqlColumn, SqlColumns, SqlFilter, SqlOp, TypedColumns, accepts_kind,
    sql_value, typed_value, value_json,
};

/// The standard `read` action.
pub const READ: &str = "read";
/// The standard `create` action.
pub const CREATE: &str = "create";
/// The standard `update` action.
pub const UPDATE: &str = "update";
/// The standard `delete` action.
pub const DELETE: &str = "delete";
/// `manage`: a rule with this action matches every action.
pub const MANAGE: &str = "manage";
/// `all`: a rule with this subject matches every subject type.
pub const ALL: &str = "all";

/// A type that abilities can be checked against, such as an entity.
///
/// ```
/// use navree_access::engine::Subject;
///
/// struct Post;
/// impl Subject for Post {
///     const SUBJECT: &'static str = "Post";
///     const PERMISSION_PREFIX: &'static str = "posts";
/// }
/// ```
pub trait Subject {
    /// The CASL subject type name, e.g. `"Post"`.
    const SUBJECT: &'static str;
    /// The prefix of permission strings for this subject, e.g. `"posts"` for
    /// `posts.publish`.
    const PERMISSION_PREFIX: &'static str;
}

/// A type-level action, so extractors such as `Authorized<Post, Update>` can
/// name the action they check.
pub trait ActionMarker {
    /// The action name, e.g. `"update"`.
    const NAME: &'static str;
}

macro_rules! action_markers {
    ($($(#[$doc:meta])* $name:ident => $value:ident;)*) => {
        $(
            $(#[$doc])*
            #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
            pub struct $name;

            impl ActionMarker for $name {
                const NAME: &'static str = $value;
            }
        )*
    };
}

action_markers! {
    /// The [`READ`] action.
    Read => READ;
    /// The [`CREATE`] action.
    Create => CREATE;
    /// The [`UPDATE`] action.
    Update => UPDATE;
    /// The [`DELETE`] action.
    Delete => DELETE;
    /// The [`MANAGE`] action (any action).
    Manage => MANAGE;
}
