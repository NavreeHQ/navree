//! # navree-validation
//!
//! `ValidationModule`: validated extractors on [garde](https://docs.rs/garde),
//! field-level messages and database `unique` / `exists` rules.
//!
//! - [`Valid`] wraps `Json<T>`, `Query<T>` or `Form<T>` (`T:
//!   garde::Validate<Context = ()> + DeserializeOwned`). Every failure is a
//!   400 error in the JSON error shape: `message` lists every message and
//!   `errors` maps dotted field paths to their messages: malformed JSON
//!   (`{"body": ["body must be a valid JSON string"]}`), serde errors at
//!   their path (a missing field is `required`, `"title should not be
//!   empty"`; a wrong type `string` / `integer` / ...; an unknown enum
//!   variant `in`) and garde rules (`"email must be a valid email
//!   address"`, `"items.1.quantity must not be less than 1"`).
//! - [`ValidAsync`] then runs the DTO's [`AsyncRules`] ([`AsyncRule::unique`],
//!   [`AsyncRule::exists`]) in the request's unit of work, as parameterised
//!   `SELECT 1 ... LIMIT 1` queries on validated identifiers.
//! - [`rules`] adds what garde lacks: [`confirmed`], [`rules::uuid`],
//!   [`rules::date_ymd`], [`rules::one_of`]; [`rule_error`] gives your own
//!   rules table-driven messages.
//! - [`Messages`] holds the templates ([`DEFAULT_MESSAGES`] plus overrides by
//!   rule, by `field.rule` and attribute names). [`ValidationModule`]
//!   (global) provides them from [`ValidationOptions`] or the `validation`
//!   config section; without the module the defaults apply.
//! - International input: [`rules::unicode_name`] (names in any script),
//!   [`fold_digits`] and [`Folded`] (Arabic-Indic, Persian, Devanagari and
//!   every other script's digits as ASCII before rules run), [`Country`]
//!   (ISO 3166-1 alpha-2) and [`Locale`] (BCP 47), with the `phone` feature
//!   `Phone` (E.164, the default region from `validation.phone` or a
//!   [`RegionSource`]) and with the `html` feature `SafeHtml` (rich text
//!   sanitized by the app's [`HtmlPolicy`] while deserializing). Their
//!   messages come from the same table (`phone`, `country`, `locale`,
//!   `unicode_name`), so translated tables translate them too.
//!
//! ```
//! use garde::Validate;
//! use navree_db::Uow;
//! use navree_http::{Created, Json};
//! use navree_validation::{AsyncRule, AsyncRules, ValidAsync, confirmed, rules};
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, Validate)]
//! struct Register {
//!     #[garde(length(min = 1, max = 255))]
//!     name: String,
//!     #[garde(email)]
//!     email: String,
//!     #[garde(length(min = 8), custom(confirmed(&self.password_confirmation)))]
//!     password: String,
//!     #[garde(skip)]
//!     password_confirmation: String,
//!     #[garde(custom(rules::one_of(&["admin", "manager", "rep"])))]
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
//!     // `input` passed garde and the unique rule; `uow` is the same unit of work.
//!     Ok(Created("/api/users/1".to_owned(), input.name))
//! }
//! # let _: axum::routing::MethodRouter = axum::routing::post(register);
//! ```
//!
//! garde's derive refers to `::garde`, so apps add `garde` as a direct
//! dependency (the re-export below is for paths such as `garde::Report`).
#![forbid(unsafe_code)]

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

pub use async_rules::{AsyncRule, AsyncRules, RuleError};
pub use context::{InputContext, RegionSource};
pub use country::{Country, InvalidCountry};
pub use digits::{Folded, fold_digits};
pub use errors::ValidationErrors;
pub use extract::{Valid, ValidAsync, validate, validate_in};
pub use failure::rule_error;
pub use html::HtmlPolicy;
#[cfg(feature = "html")]
pub use html::SafeHtml;
pub use locale::{InvalidLocale, Locale};
pub use messages::{DEFAULT_MESSAGES, Messages};
pub use module::{
    CONFIG_SECTION, ModuleBuilderValidationExt, PhoneOptions, ValidationModule, ValidationOptions,
};
#[cfg(feature = "phone")]
pub use phone::{InvalidPhone, Phone};
pub use rules::confirmed;

/// garde, the validation library behind [`Valid`].
pub use garde;
