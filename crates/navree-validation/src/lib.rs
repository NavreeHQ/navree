//! # navree-validation
//!
//! `ValidationModule`: `#[derive(Validate)]` rules, validated extractors,
//! field-level messages and database `unique` / `exists` rules.
//!
//! - [`Validate`] (the trait, and the derive of the same name) checks a
//!   value's rules: each field states them in `#[validate(...)]`
//!   (`length`, `range`, `required`, `email`, `url`, `pattern`, `custom`,
//!   `nested`, `each`, ...; see the [derive](derive@Validate)), or `skip`.
//!   Failures are [`Violations`]: a [`Violation`] (a rule key and its
//!   parameters, or a text) at each dotted [`FieldPath`].
//! - [`Valid`] wraps `Json<T>`, `Query<T>` or `Form<T>` (`T:
//!   navree_validation::Validate + DeserializeOwned`). Every failure is a
//!   400 error in the JSON error shape: `message` lists every message and
//!   `errors` maps dotted field paths to their messages: malformed JSON
//!   (`{"body": ["body must be a valid JSON string"]}`), serde errors at
//!   their path (a missing field is `required`, `"title should not be
//!   empty"`; a wrong type `string` / `integer` / ...; an unknown enum
//!   variant `in`) and the rules (`"email must be a valid email
//!   address"`, `"items.1.quantity must not be less than 1"`).
//! - [`ValidAsync`] then runs the DTO's [`AsyncRules`] ([`AsyncRule::unique`],
//!   [`AsyncRule::exists`]) in the request's unit of work, as parameterised
//!   `SELECT 1 ... LIMIT 1` queries on validated identifiers.
//! - [`rules`] has the rules' functions, and Navree's rules for `custom`:
//!   [`confirmed`], [`rules::uuid`], [`rules::date_ymd`],
//!   [`rules::one_of`]; [`rule_error`] gives your own rules table-driven
//!   messages.
//! - [`validate_json`] deserializes and checks JSON that does not come from
//!   a request (a model's tool arguments) with the same messages.
//! - [`Messages`] holds the templates ([`DEFAULT_MESSAGES`] plus overrides by
//!   rule, by `field.rule` and attribute names). [`ValidationModule`]
//!   (global) provides them from [`ValidationOptions`] or the `validation`
//!   config section; without the module the defaults apply. A
//!   [`MessageTranslator`] in the app (navree-i18n registers one) renders
//!   the messages in the request's language instead.
//! - International input: [`rules::unicode_name`] (names in any script),
//!   [`fold_digits`] and [`Folded`] (Arabic-Indic, Persian, Devanagari and
//!   every other script's digits as ASCII before rules run), [`Country`]
//!   (ISO 3166-1 alpha-2) and [`Locale`] (BCP 47), with the `phone` feature
//!   `Phone` (E.164, the default region from `validation.phone` or a
//!   [`RegionSource`]) and with the `html` feature `SafeHtml` (rich text
//!   sanitized by the app's [`HtmlPolicy`] while deserializing). Their
//!   messages come from the same table (`phone`, `country`, `locale`,
//!   `unicode_name`), so translated tables translate them too.
//! - With the `time` feature, navree-time's `Zone`, `LocalDay` and `DayRange`
//!   implement [`Validate`] (a `DayRange` must not end before it starts), so
//!   `Valid<Query<DayRange>>` takes a report window from a request.
//!
//! ```
//! use navree_validation::Validate;
//! use navree_db::Uow;
//! use navree_http::{Created, Json};
//! use navree_validation::{AsyncRule, AsyncRules, ValidAsync, confirmed, rules};
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, Validate)]
//! struct Register {
//!     #[validate(length(min = 1, max = 255))]
//!     name: String,
//!     #[validate(email)]
//!     email: String,
//!     #[validate(length(min = 8), custom(confirmed(&self.password_confirmation)))]
//!     password: String,
//!     #[validate(skip)]
//!     password_confirmation: String,
//!     #[validate(custom(rules::one_of(&["admin", "manager", "rep"])))]
//!     role: String,
//! }
//!
//! impl AsyncRules for Register {
//!     fn async_rules(&self) -> Vec<AsyncRule> {
//!         vec![AsyncRule::unique("email", "users", "email", self.email.clone(), None)]
//!     }
//! }
//!
//! async fn register(
//!     uow: Uow,
//!     ValidAsync(Json(input)): ValidAsync<Json<Register>>,
//! ) -> navree_http::Result<Created<String>> {
//!     // `input` passed its rules and the unique rule; `uow` is the same unit of work.
//!     Ok(Created("/api/users/1".to_owned(), input.name))
//! }
//! # let _ = navree_http::RouteDef::on(http::Method::POST, "/", register);
//! ```
//!
//! The derive names this crate as `::navree_validation`, or as
//! `::navree::validation` in an app that depends on the `navree` facade
//! only.
#![forbid(unsafe_code)]

// The derive names the crate `::navree_validation`, here too.
extern crate self as navree_validation;

mod async_rules;
mod context;
mod country;
mod digits;
mod errors;
mod extract;
mod failure;
mod html;
mod locale;
mod messages;
mod module;
#[cfg(feature = "phone")]
mod phone;
pub mod rules;
#[cfg(feature = "time")]
mod time;
mod validate;
mod value;

pub use async_rules::{AsyncRule, AsyncRules, RuleError, UowCheck};
pub use context::{InputContext, RegionSource};
pub use country::{Country, InvalidCountry};
pub use digits::{Folded, fold_digits};
pub use errors::ValidationErrors;
pub use extract::{Valid, ValidAsync, ValidInput, validate, validate_in, validate_value};
pub use failure::rule_error;
pub use html::HtmlPolicy;
#[cfg(feature = "html")]
pub use html::SafeHtml;
pub use locale::{InvalidLocale, Locale};
pub use messages::{DEFAULT_MESSAGES, MessageTranslator, Messages};
pub use module::{
    CONFIG_SECTION, ModuleBuilderValidationExt, PhoneOptions, ValidationModule, ValidationOptions,
};
#[cfg(feature = "phone")]
pub use phone::{InvalidPhone, Phone};
pub use rules::confirmed;
pub use validate::{FieldPath, Validate, Violation, Violations};
pub use value::validate_json;

/// Implements [`Validate`] from each field's `#[validate(...)]` rules.
///
/// Every field lists its rules, or says `skip`:
///
/// ```
/// use navree_validation::{Validate, rules};
/// use serde::Deserialize;
///
/// #[derive(Deserialize, Validate)]
/// struct CreateContact {
///     #[validate(length(min = 1, max = 100), custom(rules::unicode_name))]
///     name: String,
///     #[validate(required, email)]
///     email: Option<String>,
///     #[validate(range(min = 16, max = 120))]
///     age: Option<u8>,
///     #[validate(length(max = 10), each(length(min = 1, max = 30)))]
///     tags: Vec<String>,
///     #[validate(nested)]
///     address: Option<Address>,
///     #[validate(skip)]
///     source: String,
/// }
///
/// #[derive(Deserialize, Validate)]
/// struct Address {
///     #[validate(length(min = 1))]
///     city: String,
/// }
///
/// let contact = CreateContact {
///     name: "Ada".into(),
///     email: None,
///     age: Some(12),
///     tags: vec!["vip".into(), String::new()],
///     address: Some(Address { city: String::new() }),
///     source: "web".into(),
/// };
/// let violations = contact.validate().unwrap_err();
/// let paths: Vec<&str> = violations.iter().map(|(path, _)| path).collect();
/// assert_eq!(paths, ["email", "age", "tags.1", "address.city"]);
/// ```
///
/// | Rule | Checks | Message key |
/// |---|---|---|
/// | `skip` | nothing | |
/// | `required` | an `Option` is `Some` | `required` |
/// | `length(min = 1, max = 255)` | characters of text (`length(bytes, ...)`: bytes), items of a collection | `min.string`, `max.string`, `min.array`, `max.array` |
/// | `range(min = 1, max = 100)` | a number, bounds included | `min.numeric`, `max.numeric` |
/// | `email`, `url` | an email address ([`rules::is_email`]), an absolute URL ([`rules::is_url`]) | `email`, `url` |
/// | `ip`, `ipv4`, `ipv6` | an IP address | `ip`, `ipv4`, `ipv6` |
/// | `ascii`, `alphanumeric` | ASCII only; letters and digits only | `ascii`, `alpha_num` |
/// | `credit_card` | a card number with a valid check digit | `credit_card` |
/// | `contains("x")`, `starts_with("x")`, `ends_with("x")` | text around `x` | `contains`, `starts_with`, `ends_with` |
/// | `pattern("^[a-z]+$")` | a regular expression, checked when the code compiles | `regex` |
/// | `same_as(other)` | equals the field `other` | `same` |
/// | `custom(f)` | `f(&value)` returns `Ok(())` | the [`Violation`] `f` returns |
/// | `nested` | the value's own rules (a struct that derives `Validate`, or a list, map or `Option` of them) | theirs, at `field.inner` paths |
/// | `each(...)` | the rules inside it, on every item of a list, set or map | at `field.0`, `field.key` |
///
/// Rules on an `Option` pass on `None`; `required` demands a value.
/// `custom` expressions may use `self` (`custom(confirmed(&self.x))`).
/// Paths use the names serde reads (`#[serde(rename = "...")]`,
/// `rename_all`); a `#[serde(flatten)]` field's rules report at the
/// parent's path. `#[validate(rename = "...")]` sets a field's name in
/// paths. On the type, `#[validate(skip_unmarked)]` lets fields without
/// `#[validate]` go unchecked, and `#[validate(crate = "...")]` names this
/// crate's path.
///
/// The derive works on structs (named, tuple or newtype) and enums, not
/// unions, with generics. A newtype's field reports at the path of the value
/// itself; other tuple fields are named `0`, `1`.
pub use navree_validation_macros::Validate;

/// What `#[derive(Validate)]` expands to. Not public API.
#[doc(hidden)]
pub mod __private {
    pub use crate::rules::{
        Each, Pattern, Unit, alphanumeric, ascii, contains, credit_card, email, ends_with, ip,
        ipv4, ipv6, length, pattern, range, required, same_as, starts_with, url,
    };

    /// A `custom` rule's result, its error as a [`Violation`](crate::Violation).
    pub fn custom<E: Into<crate::Violation>>(
        result: Result<(), E>,
    ) -> Result<(), crate::Violation> {
        result.map_err(Into::into)
    }
}
