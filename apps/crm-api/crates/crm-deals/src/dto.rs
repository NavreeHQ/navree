//! Request bodies and queries, with their validation rules.
//!
//! Required body fields are `Option`s with `required` rule, so a
//! request missing several of them gets every error at once (a serde
//! "missing field" error would stop at the first).

use crm_core::Stage;
use navree::openapi::ToSchema;
use navree::orm::list::{Field, ListResource, ListSpec};
use navree::validation::Validate;
use navree::validation::rules::{self, RuleInput};
use navree::validation::{Violation, rule_error};
use serde::{Deserialize, Deserializer};
use uuid::Uuid;

/// The largest `amount_cents` a deal may have: 10^15 cents (ten trillion
/// in the currency's main unit). Below 2^53, so the web app's JavaScript
/// numbers stay exact, and sums over many deals stay far from `i64::MAX`.
pub const MAX_AMOUNT_CENTS: i64 = 1_000_000_000_000_000;

/// The body of `POST /api/v1/deals`.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[schema(as = DealInput)]
pub struct CreateDeal {
    /// The title.
    #[validate(required, custom(not_blank), length(max = 255))]
    #[schema(value_type = String, required = true)]
    pub title: Option<String>,
    /// The company (must exist).
    #[validate(required)]
    #[schema(value_type = String, format = Uuid, required = true)]
    pub company_id: Option<Uuid>,
    /// The main contact (must exist).
    #[validate(skip)]
    pub contact_id: Option<Uuid>,
    /// The value in cents, from 0 to [`MAX_AMOUNT_CENTS`].
    #[validate(required, range(min = 0, max = MAX_AMOUNT_CENTS))]
    #[schema(value_type = i64, required = true, minimum = 0, maximum = 1_000_000_000_000_000_i64)]
    pub amount_cents: Option<i64>,
    /// The ISO 4217 code (default `INR`; stored upper-case).
    #[validate(custom(currency_code))]
    pub currency: Option<String>,
    /// The stage (default `lead`).
    #[validate(skip)]
    pub stage: Option<Stage>,
    /// Why the deal was lost: required when `stage` is `lost`.
    #[validate(length(max = 1000))]
    pub lost_reason: Option<String>,
    /// `YYYY-MM-DD`.
    #[validate(custom(rules::date_ymd))]
    #[schema(format = Date)]
    pub expected_close_date: Option<String>,
    /// The owner (default: the signed-in user; someone else needs
    /// `manage` on deals).
    #[validate(skip)]
    pub owner_id: Option<Uuid>,
}

/// The body of `PATCH /api/v1/deals/{id}`: only the fields present
/// change; `contact_id` and `expected_close_date` may be `null` to clear
/// them.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[schema(as = DealUpdateInput)]
pub struct UpdateDeal {
    /// The title.
    #[validate(custom(not_blank), length(max = 255))]
    pub title: Option<String>,
    /// The company (must exist).
    #[validate(skip)]
    pub company_id: Option<Uuid>,
    /// The main contact (must exist), or `null`.
    #[serde(default, deserialize_with = "present")]
    #[validate(skip)]
    #[schema(value_type = Option<String>, format = Uuid)]
    pub contact_id: Option<Option<Uuid>>,
    /// The value in cents, from 0 to [`MAX_AMOUNT_CENTS`].
    #[validate(range(min = 0, max = MAX_AMOUNT_CENTS))]
    #[schema(minimum = 0, maximum = 1_000_000_000_000_000_i64)]
    pub amount_cents: Option<i64>,
    /// The ISO 4217 code (stored upper-case).
    #[validate(custom(currency_code))]
    pub currency: Option<String>,
    /// A new stage: the deal moves to the end of that column (needs `move`
    /// on the deal).
    #[validate(skip)]
    pub stage: Option<Stage>,
    /// Why the deal was lost: required when it becomes `lost`.
    #[validate(length(max = 1000))]
    pub lost_reason: Option<String>,
    /// `YYYY-MM-DD`, or `null`.
    #[serde(default, deserialize_with = "present")]
    #[validate(custom(rules::date_ymd))]
    #[schema(value_type = Option<String>, format = Date)]
    pub expected_close_date: Option<Option<String>>,
    /// A new owner (needs `manage` on deals).
    #[validate(skip)]
    pub owner_id: Option<Uuid>,
}

/// The body of `POST /api/v1/deals/{id}/move`.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[schema(as = DealMoveInput)]
pub struct MoveDeal {
    /// The target stage.
    #[validate(required)]
    #[schema(value_type = Stage, required = true)]
    pub stage: Option<Stage>,
    /// The 0-based index within the target stage (default: the end;
    /// larger values mean the end); the other deals there shift.
    #[validate(range(min = 0))]
    pub position: Option<i64>,
    /// Why the deal was lost: required when `stage` is `lost`.
    #[validate(length(max = 1000))]
    pub lost_reason: Option<String>,
}

/// The strings of [`Stage`], in board order (`filter[stage]` takes these).
const STAGES: &[&str] = &[
    "lead",
    "qualified",
    "proposal",
    "negotiation",
    "won",
    "lost",
];

/// The list query of `GET /api/v1/deals`, in the list-query grammar:
///
/// - `filter[stage]`: one stage, or several with `filter[stage][in]=lead,won`.
/// - `filter[owner_id]` and `filter[company_id]`: a user's or a company's id.
/// - `search`: part of the title, ignoring case.
/// - `sort`: `created_at`, `title`, `amount_cents` or `expected_close_date`,
///   with `-` in front for descending. The default is `-created_at`.
/// - `page` and `per_page` (default 15, at most 100).
pub struct DealList;

impl ListResource for DealList {
    const SPEC: ListSpec = ListSpec::new(&[
        Field::one_of("stage", STAGES),
        Field::uuid("owner_id"),
        Field::uuid("company_id"),
    ])
    .sortable(&["created_at", "title", "amount_cents", "expected_close_date"])
    .default_sort(&["-created_at"])
    .searchable(&["title"]);
}

/// The query of `GET /api/v1/deals/board`.
#[derive(Debug, Default, Deserialize, Validate)]
pub struct BoardFilter {
    /// Only deals this user owns.
    #[validate(skip)]
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
fn not_blank<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    match value.rule_str() {
        Some(text) if text.trim().is_empty() => Err(rule_error("required", &[])),
        _ => Ok(()),
    }
}

/// Three ASCII letters (any case).
fn currency_code<T: RuleInput + ?Sized>(value: &T) -> Result<(), &'static str> {
    match value.rule_str() {
        Some(code) if !is_currency_code(code) => Err(":attribute must be a 3-letter currency code"),
        _ => Ok(()),
    }
}

fn is_currency_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use navree::orm::list::Kind;
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
    fn the_stage_filter_takes_exactly_the_stages() {
        let field = DealList::SPEC.field("stage").expect("a stage filter");
        let words: Vec<&str> = Stage::ALL.iter().map(|stage| stage.as_str()).collect();
        assert_eq!(field.kind, Kind::OneOf(STAGES));
        assert_eq!(STAGES, words);
    }

    #[test]
    fn the_list_query_reads_the_documented_parameters() {
        let id = Uuid::now_v7();
        let query = format!(
            "filter[stage][in]=lead,won&filter[owner_id]={id}&filter[company_id]={id}\
             &search=agra&sort=-amount_cents,title&page=2&per_page=100"
        );
        let list = DealList::SPEC.parse(&query).expect("a valid query");
        assert_eq!(list.filter.len(), 3);
        assert_eq!(list.search.as_deref(), Some("agra"));
        assert_eq!(list.sort.len(), 2);

        for (query, param) in [
            ("filter[stage]=closed", "filter[stage]"),
            ("filter[title]=x", "filter[title]"),
            ("filter[owner_id]=nope", "filter[owner_id]"),
            ("sort=password", "sort"),
            ("per_page=101", "per_page"),
            ("limit=20&page=2", "limit"),
        ] {
            let error = DealList::SPEC.parse(query).expect_err(query);
            assert!(
                error.problems.iter().any(|problem| problem.param == param),
                "{query}: {error}"
            );
        }
        // The flat names of the old query are not the list query's.
        let flat = DealList::SPEC.parse("stage=won&search=").expect("ignored");
        assert!(flat.filter.is_empty());
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
