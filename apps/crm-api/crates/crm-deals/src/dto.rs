//! Request bodies and queries, with their validation rules.
//!
//! Required body fields are `Option`s with garde's `required` rule, so a
//! request missing several of them gets every error at once (a serde
//! "missing field" error would stop at the first).

use crm_core::Stage;
use garde::Validate;
use navree::validation::rule_error;
use navree::validation::rules::{self, RuleInput};
use serde::{Deserialize, Deserializer};
use utoipa::ToSchema;
use uuid::Uuid;

/// The body of `POST /api/v1/deals`.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[schema(as = DealInput)]
pub struct CreateDeal {
    /// The title.
    #[garde(required, custom(not_blank), length(max = 255))]
    #[schema(value_type = String, required = true)]
    pub title: Option<String>,
    /// The company (must exist).
    #[garde(required)]
    #[schema(value_type = String, format = Uuid, required = true)]
    pub company_id: Option<Uuid>,
    /// The main contact (must exist).
    #[garde(skip)]
    pub contact_id: Option<Uuid>,
    /// The value in cents, at least 0.
    #[garde(required, range(min = 0))]
    #[schema(value_type = i64, required = true)]
    pub amount_cents: Option<i64>,
    /// The ISO 4217 code (default `USD`; stored upper-case).
    #[garde(custom(currency_code))]
    pub currency: Option<String>,
    /// The stage (default `lead`).
    #[garde(skip)]
    pub stage: Option<Stage>,
    /// Why the deal was lost: required when `stage` is `lost`.
    #[garde(length(max = 1000))]
    pub lost_reason: Option<String>,
    /// `YYYY-MM-DD`.
    #[garde(custom(rules::date_ymd))]
    #[schema(format = Date)]
    pub expected_close_date: Option<String>,
    /// The owner (default: the signed-in user; someone else needs
    /// `manage` on deals).
    #[garde(skip)]
    pub owner_id: Option<Uuid>,
}

/// The body of `PATCH /api/v1/deals/{id}`: only the fields present
/// change; `contact_id` and `expected_close_date` may be `null` to clear
/// them.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[schema(as = DealUpdateInput)]
pub struct UpdateDeal {
    /// The title.
    #[garde(custom(not_blank), length(max = 255))]
    pub title: Option<String>,
    /// The company (must exist).
    #[garde(skip)]
    pub company_id: Option<Uuid>,
    /// The main contact (must exist), or `null`.
    #[serde(default, deserialize_with = "present")]
    #[garde(skip)]
    #[schema(value_type = Option<String>, format = Uuid)]
    pub contact_id: Option<Option<Uuid>>,
    /// The value in cents, at least 0.
    #[garde(range(min = 0))]
    pub amount_cents: Option<i64>,
    /// The ISO 4217 code (stored upper-case).
    #[garde(custom(currency_code))]
    pub currency: Option<String>,
    /// A new stage: the deal moves to the end of that column (needs `move`
    /// on the deal).
    #[garde(skip)]
    pub stage: Option<Stage>,
    /// Why the deal was lost: required when it becomes `lost`.
    #[garde(length(max = 1000))]
    pub lost_reason: Option<String>,
    /// `YYYY-MM-DD`, or `null`.
    #[serde(default, deserialize_with = "present")]
    #[garde(custom(rules::date_ymd))]
    #[schema(value_type = Option<String>, format = Date)]
    pub expected_close_date: Option<Option<String>>,
    /// A new owner (needs `manage` on deals).
    #[garde(skip)]
    pub owner_id: Option<Uuid>,
}

/// The body of `POST /api/v1/deals/{id}/move`.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[schema(as = DealMoveInput)]
pub struct MoveDeal {
    /// The target stage.
    #[garde(required)]
    #[schema(value_type = Stage, required = true)]
    pub stage: Option<Stage>,
    /// The 0-based index within the target stage (default: the end;
    /// larger values mean the end); the other deals there shift.
    #[garde(range(min = 0))]
    pub position: Option<i64>,
    /// Why the deal was lost: required when `stage` is `lost`.
    #[garde(length(max = 1000))]
    pub lost_reason: Option<String>,
}

/// The query of `GET /api/v1/deals` (besides `page` and `per_page`).
#[derive(Debug, Default, Deserialize, Validate)]
pub struct DealFilter {
    /// Only deals in this stage.
    #[garde(skip)]
    pub stage: Option<Stage>,
    /// Only deals this user owns.
    #[garde(skip)]
    pub owner_id: Option<Uuid>,
    /// Only deals of this company.
    #[garde(skip)]
    pub company_id: Option<Uuid>,
    /// Part of the title (case-insensitive).
    #[garde(skip)]
    pub search: Option<String>,
}

/// The query of `GET /api/v1/deals/board`.
#[derive(Debug, Default, Deserialize, Validate)]
pub struct BoardFilter {
    /// Only deals this user owns.
    #[garde(skip)]
    pub owner_id: Option<Uuid>,
}

/// A field that may be `null`: present (`Some(None)` for `null`) versus
/// absent (`None`, through `#[serde(default)]`).
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Not empty or whitespace only (message key `required`).
fn not_blank<T: RuleInput + ?Sized, C: ?Sized>(value: &T, _ctx: &C) -> garde::Result {
    match value.rule_str() {
        Some(text) if text.trim().is_empty() => Err(rule_error("required", &[])),
        _ => Ok(()),
    }
}

/// Three ASCII letters (any case).
fn currency_code<T: RuleInput + ?Sized, C: ?Sized>(value: &T, _ctx: &C) -> garde::Result {
    match value.rule_str() {
        Some(code) if !is_currency_code(code) => Err(garde::Error::new(
            ":attribute must be a 3-letter currency code",
        )),
        _ => Ok(()),
    }
}

fn is_currency_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn nullable_fields_tell_null_from_absent() {
        let absent: UpdateDeal = serde_json::from_value(json!({})).unwrap();
        assert_eq!(absent.contact_id, None);
        assert_eq!(absent.expected_close_date, None);
        let cleared: UpdateDeal =
            serde_json::from_value(json!({ "contact_id": null, "expected_close_date": null }))
                .unwrap();
        assert_eq!(cleared.contact_id, Some(None));
        assert_eq!(cleared.expected_close_date, Some(None));
        let id = Uuid::now_v7();
        let set: UpdateDeal = serde_json::from_value(
            json!({ "contact_id": id, "expected_close_date": "2026-12-01" }),
        )
        .unwrap();
        assert_eq!(set.contact_id, Some(Some(id)));
        assert_eq!(set.expected_close_date, Some(Some("2026-12-01".to_owned())));
    }

    #[test]
    fn currency_codes_are_three_letters() {
        for good in ["USD", "eur", "Gbp"] {
            assert!(is_currency_code(good), "{good}");
        }
        for bad in ["", "US", "USDT", "U5D", "€€€"] {
            assert!(!is_currency_code(bad), "{bad}");
        }
    }
}
