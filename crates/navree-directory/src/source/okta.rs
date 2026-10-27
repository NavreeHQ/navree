//! Okta through its Users and Groups APIs.
//!
//! Each pull reads every user (`/api/v1/users`, 200 a page, following the
//! `Link: rel="next"` header), every group and each group's users: a
//! snapshot. Okta leaves deprovisioned users out of the listing, so they
//! are deactivated as missing; suspended and staged users are inactive.
//! Connect Okta in navree-integrations as an `api_key` provider with
//! `scheme = "SSWS"` (an API token) or as an `oauth2` service app.

use std::collections::HashSet;

use navree_integrations::IntegrationClient;
use navree_kernel::BoxFuture;
use serde_json::Value;

use super::graph::error_message;
use super::{DirectorySource, PullRequest, text};
use crate::error::DirectoryError;
use crate::pulled::Pulled;
use crate::types::OKTA;
use crate::user::{ContactValue, DirectoryGroup, DirectoryUser};

/// The most pages one listing follows.
const MAX_PAGES: usize = 100_000;

/// The `okta` source.
pub(crate) struct OktaSource;

impl DirectorySource for OktaSource {
    fn pull<'a>(
        &'a self,
        request: &'a PullRequest,
    ) -> BoxFuture<'a, Result<Pulled, DirectoryError>> {
        Box::pin(async move {
            let client = request.client(OKTA).await?;
            let mut first = "/api/v1/users?limit=200".to_owned();
            if let Some(search) = request.setting("search") {
                first = format!(
                    "{first}&search={}",
                    url::form_urlencoded::byte_serialize(search.as_bytes()).collect::<String>()
                );
            }
            let users: Vec<DirectoryUser> = list(&client, &first)
                .await?
                .iter()
                .filter_map(user_of)
                .collect();
            let known: HashSet<&str> = users.iter().map(|u| u.external_id.as_str()).collect();
            let mut groups = Vec::new();
            if request.flag("groups", true) {
                let mut first = "/api/v1/groups?limit=200".to_owned();
                if let Some(filter) = request.setting("group_filter") {
                    first = format!(
                        "{first}&filter={}",
                        url::form_urlencoded::byte_serialize(filter.as_bytes()).collect::<String>()
                    );
                }
                for item in list(&client, &first).await? {
                    let Some(id) = text(item.get("id")) else {
                        continue;
                    };
                    let name = text(item.pointer("/profile/name")).unwrap_or_else(|| id.clone());
                    let mut group = DirectoryGroup::new(id.clone(), name);
                    group.description = text(item.pointer("/profile/description"));
                    if let Some(kind) = text(item.get("type")) {
                        group
                            .attributes
                            .insert("type".to_owned(), Value::String(kind));
                    }
                    for member in
                        list(&client, &format!("/api/v1/groups/{id}/users?limit=200")).await?
                    {
                        if let Some(member_id) = text(member.get("id"))
                            && known.contains(member_id.as_str())
                        {
                            group.members.push(member_id);
                        }
                    }
                    groups.push(group);
                }
            }
            Ok(Pulled::snapshot(users, groups))
        })
    }
}

/// Every item of a listing, following `Link: <...>; rel="next"`.
async fn list(client: &IntegrationClient, first: &str) -> Result<Vec<Value>, DirectoryError> {
    let mut items = Vec::new();
    let mut next = Some(first.to_owned());
    for _ in 0..MAX_PAGES {
        let Some(target) = next.take() else {
            return Ok(items);
        };
        let response = client.get(target).send().await?;
        let status = response.status().as_u16();
        if !response.is_success() {
            return Err(failed(error_message(status, &response.text())));
        }
        let page: Value = response
            .json()
            .map_err(|error| failed(format!("Okta answered with invalid JSON: {error}")))?;
        if let Value::Array(values) = page {
            items.extend(values);
        }
        next = response
            .headers()
            .get_all(http::header::LINK)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(next_link);
    }
    Err(failed("Okta's listing did not end"))
}

/// The `rel="next"` URL of a `Link` header value.
pub(crate) fn next_link(header: &str) -> Option<String> {
    header.split(',').find_map(|part| {
        let mut pieces = part.split(';');
        let url = pieces.next()?.trim();
        let is_next = pieces.any(|param| {
            let param = param.trim();
            param.eq_ignore_ascii_case("rel=\"next\"") || param.eq_ignore_ascii_case("rel=next")
        });
        (is_next && url.starts_with('<') && url.ends_with('>'))
            .then(|| url[1..url.len() - 1].to_owned())
    })
}

/// An Okta user as a [`DirectoryUser`].
pub(crate) fn user_of(item: &Value) -> Option<DirectoryUser> {
    let id = text(item.get("id"))?;
    let profile = item.get("profile")?;
    let login = text(profile.get("login"))?;
    let mut user = DirectoryUser::new(id, login);
    user.active = match item.get("status").and_then(Value::as_str) {
        Some(status) => !matches!(
            status.to_ascii_uppercase().as_str(),
            "SUSPENDED" | "DEPROVISIONED" | "STAGED"
        ),
        None => true,
    };
    user.given_name = text(profile.get("firstName"));
    user.family_name = text(profile.get("lastName"));
    user.display_name = text(profile.get("displayName"));
    if let Some(email) = text(profile.get("email")) {
        user.emails
            .push(ContactValue::new(email, Some("work"), true));
    }
    if let Some(second) = text(profile.get("secondEmail"))
        && !user
            .emails
            .iter()
            .any(|e| e.value.eq_ignore_ascii_case(&second))
    {
        user.emails
            .push(ContactValue::new(second, Some("other"), false));
    }
    if let Some(phone) = text(profile.get("primaryPhone")) {
        user.phone_numbers
            .push(ContactValue::new(phone, Some("work"), true));
    }
    if let Some(mobile) = text(profile.get("mobilePhone")) {
        let primary = user.phone_numbers.is_empty();
        user.phone_numbers
            .push(ContactValue::new(mobile, Some("mobile"), primary));
    }
    user.title = text(profile.get("title"));
    user.department = text(profile.get("department"));
    user.organization = text(profile.get("organization"));
    user.employee_number = text(profile.get("employeeNumber"));
    user.manager_id = text(profile.get("managerId"));
    user.locale = text(profile.get("preferredLanguage")).or_else(|| text(profile.get("locale")));
    user.time_zone = text(profile.get("timezone"));
    for key in ["costCenter", "division", "userType"] {
        if let Some(value) = text(profile.get(key)) {
            user.attributes.insert(key.to_owned(), Value::String(value));
        }
    }
    if let Some(status) = text(item.get("status")) {
        user.attributes
            .insert("status".to_owned(), Value::String(status));
    }
    Some(user)
}

fn failed(message: impl Into<String>) -> DirectoryError {
    DirectoryError::source_failed("Okta", message)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn okta_users_are_normalized() {
        let user = user_of(&json!({
            "id": "00ub0oNGTSWTBKOLGLNR",
            "status": "SUSPENDED",
            "profile": {
                "login": "ada@acme.test",
                "email": "ada@acme.test",
                "secondEmail": "countess@home.test",
                "firstName": "Ada",
                "lastName": "Lovelace",
                "title": "Principal engineer",
                "department": "Engineering",
                "employeeNumber": "1815",
                "managerId": "00ub0oNGTSWTBKOCHARL",
                "mobilePhone": "+44 7700 900123",
                "preferredLanguage": "en-GB",
                "timezone": "Europe/London",
                "costCenter": "4130"
            }
        }))
        .unwrap();
        assert_eq!(user.external_id, "00ub0oNGTSWTBKOLGLNR");
        assert_eq!(user.user_name, "ada@acme.test");
        assert!(!user.active);
        assert_eq!(user.emails.len(), 2);
        assert_eq!(user.primary_phone(), Some("+44 7700 900123"));
        assert_eq!(user.manager_id.as_deref(), Some("00ub0oNGTSWTBKOCHARL"));
        assert_eq!(user.time_zone.as_deref(), Some("Europe/London"));
        assert_eq!(user.attributes["costCenter"], json!("4130"));
        assert_eq!(user.attributes["status"], json!("SUSPENDED"));
        let active =
            user_of(&json!({ "id": "x", "status": "PROVISIONED", "profile": { "login": "x" } }))
                .unwrap();
        assert!(active.active);
        let staged =
            user_of(&json!({ "id": "x", "status": "STAGED", "profile": { "login": "x" } }))
                .unwrap();
        assert!(!staged.active);
        assert!(user_of(&json!({ "id": "x", "profile": {} })).is_none());
    }

    #[test]
    fn link_headers_give_the_next_page() {
        assert_eq!(
            next_link(
                "<https://acme.okta.com/api/v1/users?limit=200>; rel=\"self\", \
                 <https://acme.okta.com/api/v1/users?after=00u2&limit=200>; rel=\"next\""
            )
            .as_deref(),
            Some("https://acme.okta.com/api/v1/users?after=00u2&limit=200")
        );
        assert_eq!(
            next_link("<https://acme.okta.com/api/v1/users?limit=200>; rel=\"self\""),
            None
        );
    }
}
