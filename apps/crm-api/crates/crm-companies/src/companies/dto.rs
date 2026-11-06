//! Company request bodies and the list query, with their validation rules.

use crm_core::CompanySize;
use crm_core::models::{Company, User};
use navree::openapi::ToSchema;
use navree::orm::list::{Field, ListQuery, ListResource, ListSpec, Op};
use navree::orm::validation::exists;
use navree::orm::{Builder, Expr};
use navree::validation::Validate;
use navree::validation::{AsyncRule, AsyncRules};
use serde::Deserialize;
use uuid::Uuid;

use crate::common::{
    MAX_TEXT, not_blank, nullable, nullable_text, optional_text, present, present_trimmed,
    set_some, trimmed,
};

/// The body of `POST /api/v1/companies`. Text fields are trimmed; blank
/// optional fields are stored as `null`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(as = CompanyInput)]
pub struct CreateCompany {
    /// The name (required).
    #[serde(default, deserialize_with = "trimmed")]
    #[validate(custom(not_blank), length(max = MAX_TEXT))]
    pub name: String,
    /// The web domain, e.g. `agra.com`.
    #[serde(default, deserialize_with = "optional_text")]
    #[validate(length(max = MAX_TEXT))]
    pub domain: Option<String>,
    /// The industry, free text.
    #[serde(default, deserialize_with = "optional_text")]
    #[validate(length(max = MAX_TEXT))]
    pub industry: Option<String>,
    /// The head count bracket.
    #[serde(default)]
    #[validate(skip)]
    pub size: Option<CompanySize>,
    /// A phone number.
    #[serde(default, deserialize_with = "optional_text")]
    #[validate(length(max = MAX_TEXT))]
    pub phone: Option<String>,
    /// The website URL.
    #[serde(default, deserialize_with = "optional_text")]
    #[validate(length(max = MAX_TEXT))]
    pub website: Option<String>,
    /// The city.
    #[serde(default, deserialize_with = "optional_text")]
    #[validate(length(max = MAX_TEXT))]
    pub city: Option<String>,
    /// The country.
    #[serde(default, deserialize_with = "optional_text")]
    #[validate(length(max = MAX_TEXT))]
    pub country: Option<String>,
    /// The owner (default: the caller). Only users who can manage companies
    /// may name someone else (403 otherwise).
    #[serde(default)]
    #[validate(skip)]
    pub owner_id: Option<Uuid>,
}

impl CreateCompany {
    /// The new company, owned by `owner_id`.
    pub fn into_company(self, owner_id: Uuid) -> Company {
        Company {
            name: self.name,
            domain: self.domain,
            industry: self.industry,
            size: self.size,
            phone: self.phone,
            website: self.website,
            city: self.city,
            country: self.country,
            owner_id,
            ..Company::default()
        }
    }
}

impl AsyncRules for CreateCompany {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![exists(User::ID, self.owner_id).field("owner_id").into()]
    }
}

/// The body of `PATCH /api/v1/companies/{id}`: only the fields sent
/// change; `null` (or a blank string) clears an optional field.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[schema(as = CompanyUpdateInput)]
pub struct UpdateCompany {
    /// The name (cannot be cleared).
    #[serde(default, deserialize_with = "present_trimmed")]
    #[schema(nullable = false)]
    #[validate(custom(not_blank), length(max = MAX_TEXT))]
    pub name: Option<String>,
    /// The web domain.
    #[serde(default, deserialize_with = "nullable_text")]
    #[validate(length(max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub domain: Option<Option<String>>,
    /// The industry.
    #[serde(default, deserialize_with = "nullable_text")]
    #[validate(length(max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub industry: Option<Option<String>>,
    /// The head count bracket.
    #[serde(default, deserialize_with = "nullable")]
    #[validate(skip)]
    #[schema(value_type = Option<CompanySize>)]
    pub size: Option<Option<CompanySize>>,
    /// A phone number.
    #[serde(default, deserialize_with = "nullable_text")]
    #[validate(length(max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub phone: Option<Option<String>>,
    /// The website URL.
    #[serde(default, deserialize_with = "nullable_text")]
    #[validate(length(max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub website: Option<Option<String>>,
    /// The city.
    #[serde(default, deserialize_with = "nullable_text")]
    #[validate(length(max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub city: Option<Option<String>>,
    /// The country.
    #[serde(default, deserialize_with = "nullable_text")]
    #[validate(length(max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub country: Option<Option<String>>,
    /// The new owner: managers and admins only (403 otherwise).
    #[serde(default, deserialize_with = "present")]
    #[validate(skip)]
    #[schema(nullable = false)]
    pub owner_id: Option<Uuid>,
}

impl UpdateCompany {
    /// Changes the fields sent (not the owner: who may change it is the
    /// service's call).
    pub fn apply_to(self, company: &mut Company) {
        set_some(&mut company.name, self.name);
        set_some(&mut company.domain, self.domain);
        set_some(&mut company.industry, self.industry);
        set_some(&mut company.size, self.size);
        set_some(&mut company.phone, self.phone);
        set_some(&mut company.website, self.website);
        set_some(&mut company.city, self.city);
        set_some(&mut company.country, self.country);
    }
}

impl AsyncRules for UpdateCompany {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![exists(User::ID, self.owner_id).field("owner_id").into()]
    }
}

/// The list query of `GET /api/v1/companies`, in the list-query grammar:
///
/// - `filter[owner_id]`: a user's id.
/// - `filter[industry]`: an industry, ignoring case.
/// - `search`: part of the name or domain, ignoring case.
/// - `sort`: `name` (ignoring case) or `created_at`, with `-` in front for
///   descending. The default is `name`.
/// - `page` and `per_page` (default 15, at most 100).
pub struct CompanyList;

impl ListResource for CompanyList {
    const SPEC: ListSpec = ListSpec::new(&[
        Field::uuid("owner_id"),
        // Matched without regard to case, so the service applies it.
        Field::text("industry").ops(&[Op::Eq]).by_app(),
    ])
    .sortable(&["name", "created_at"])
    .default_sort(&["name"])
    .searchable_by_app();
}

impl CompanyList {
    /// `query` in the order `list` asks for (by name when it names none),
    /// then by id so pages never overlap. Names compare ignoring ASCII case,
    /// which the generic sort of `Builder::list` does not do.
    pub fn order(query: Builder<Company>, list: &ListQuery) -> Builder<Company> {
        Self::SPEC
            .sort_of(list)
            .iter()
            .fold(query.reorder(), |query, key| {
                match (key.field.as_str(), key.descending) {
                    ("name", false) => query.order_by(Expr::lower(Company::NAME)),
                    ("name", true) => query.order_by_desc(Expr::lower(Company::NAME)),
                    ("created_at", false) => query.order_by(Company::CREATED_AT),
                    ("created_at", true) => query.order_by_desc(Company::CREATED_AT),
                    _ => query,
                }
            })
            .order_by(Company::ID)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn industry_is_checked_but_left_to_the_service() {
        let field = CompanyList::SPEC
            .field("industry")
            .expect("an industry filter");
        assert!(field.by_app);
        assert_eq!(field.allowed_ops(), [Op::Eq]);
        // `contains` would be accepted and then ignored: it is refused.
        let error = CompanyList::SPEC
            .parse("filter[industry][contains]=soft")
            .expect_err("only equality");
        assert_eq!(error.problems[0].param, "filter[industry][contains]");
    }

    #[test]
    fn names_sort_by_default() {
        let list = CompanyList::SPEC.parse("").expect("an empty query");
        let sorts = CompanyList::SPEC.sort_of(&list);
        assert_eq!(sorts.len(), 1);
        assert_eq!(sorts[0].term(), "name");
        let list = CompanyList::SPEC
            .parse("sort=-created_at")
            .expect("a sortable field");
        assert_eq!(CompanyList::SPEC.sort_of(&list)[0].term(), "-created_at");
        assert!(CompanyList::SPEC.parse("sort=domain").is_err());
    }
}
