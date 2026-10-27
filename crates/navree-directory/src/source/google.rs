//! Google Workspace through the Admin SDK Directory API.
//!
//! Each pull reads every user (`/admin/directory/v1/users`, 500 a page),
//! every group and each group's members: a snapshot. Suspended and
//! archived users are inactive. A service-account connection acts as the
//! admin named in `settings.admin_email` (domain-wide delegation).

use std::collections::HashMap;

use navree_integrations::GoogleClient;
use navree_kernel::BoxFuture;
use serde_json::Value;

use super::graph::error_message;
use super::{DirectorySource, PullRequest, text};
use crate::error::DirectoryError;
use crate::pulled::Pulled;
use crate::types::GOOGLE;
use crate::user::{ContactValue, DirectoryGroup, DirectoryUser};

const DIRECTORY: &str = "/admin/directory/v1";
/// The most pages one listing follows.
const MAX_PAGES: usize = 100_000;

/// The `google` source.
pub(crate) struct GoogleSource;

impl DirectorySource for GoogleSource {
    fn check_settings(&self, settings: &Value, _has_secret: bool) -> Result<(), String> {
        if let Some(email) = settings.get("admin_email")
            && !email.as_str().is_some_and(|e| e.contains('@'))
        {
            return Err("settings.admin_email must be an email address".to_owned());
        }
        Ok(())
    }

    fn pull<'a>(&'a self, request: &'a PullRequest) -> BoxFuture<'a, Result<Pulled, DirectoryError>> {
        Box::pin(async move {
            let mut client = GoogleClient::from(request.client(GOOGLE).await?);
            if let Some(admin) = request.setting("admin_email") {
                client = client.as_user(admin);
            }
            // Users of one domain, or of the whole account.
            let scope: Vec<(&str, String)> = match request.setting("domain") {
                Some(domain) => vec![("domain", domain.to_owned())],
                None => vec![(
                    "customer",
                    request.setting("customer").unwrap_or("my_customer").to_owned(),
                )],
            };
            let mut query = scope.clone();
            query.push(("maxResults", "500".to_owned()));
            query.push(("projection", "full".to_owned()));
            if let Some(search) = request.setting("query") {
                query.push(("query", search.to_owned()));
            }
            let items = list(&client, &format!("{DIRECTORY}/users"), &query, "users").await?;
            let mut users: Vec<DirectoryUser> = items.iter().filter_map(user_of).collect();
            // Managers are named by email: point them at the manager's id.
            let by_email: HashMap<String, String> = users
                .iter()
                .map(|user| (user.user_name.to_lowercase(), user.external_id.clone()))
                .collect();
            for user in &mut users {
                if let Some(manager) = user.manager_id.take() {
                    user.manager_id = by_email.get(&manager.to_lowercase()).cloned().or(Some(manager));
                }
            }
            let mut groups = Vec::new();
            if request.flag("groups", true) {
                let mut query = scope;
                query.push(("maxResults", "200".to_owned()));
                let items = list(&client, &format!("{DIRECTORY}/groups"), &query, "groups").await?;
                let known: HashMap<&str, ()> = users
                    .iter()
                    .map(|user| (user.external_id.as_str(), ()))
                    .collect();
                for item in &items {
                    let Some(id) = text(item.get("id")) else {
                        continue;
                    };
                    let name = text(item.get("name"))
                        .or_else(|| text(item.get("email")))
                        .unwrap_or_else(|| id.clone());
                    let mut group = DirectoryGroup::new(id.clone(), name);
                    group.description = text(item.get("description"));
                    group.email = text(item.get("email"));
                    let members = list(
                        &client,
                        &format!("{DIRECTORY}/groups/{id}/members"),
                        &[("maxResults", "200".to_owned())],
                        "members",
                    )
                    .await?;
                    for member in &members {
                        let is_user = member
                            .get("type")
                            .and_then(Value::as_str)
                            .is_none_or(|kind| kind.eq_ignore_ascii_case("USER"));
                        if let Some(member_id) = text(member.get("id"))
                            && is_user
                            && known.contains_key(member_id.as_str())
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

/// Every item under `key` of every page, following `nextPageToken`.
async fn list(
    client: &GoogleClient,
    path: &str,
    query: &[(&str, String)],
    key: &str,
) -> Result<Vec<Value>, DirectoryError> {
    let mut items = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut pairs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
        if let Some(token) = &token {
            pairs.push(("pageToken", token.as_str()));
        }
        let response = client.get(path).query(&pairs).send().await?;
        let status = response.status().as_u16();
        if !response.is_success() {
            return Err(failed(error_message(status, &response.text())));
        }
        let page: Value = response
            .json()
            .map_err(|error| failed(format!("Google answered with invalid JSON: {error}")))?;
        if let Some(Value::Array(values)) = page.get(key) {
            items.extend(values.iter().cloned());
        }
        token = text(page.get("nextPageToken"));
        if token.is_none() {
            return Ok(items);
        }
    }
    Err(failed("Google's listing did not end"))
}

/// A Google Workspace user as a [`DirectoryUser`].
pub(crate) fn user_of(item: &Value) -> Option<DirectoryUser> {
    let id = text(item.get("id"))?;
    let primary = text(item.get("primaryEmail"))?;
    let mut user = DirectoryUser::new(id, primary.clone());
    let suspended = item.get("suspended").and_then(Value::as_bool).unwrap_or(false);
    let archived = item.get("archived").and_then(Value::as_bool).unwrap_or(false);
    user.active = !suspended && !archived;
    user.given_name = text(item.pointer("/name/givenName"));
    user.family_name = text(item.pointer("/name/familyName"));
    user.display_name = text(item.pointer("/name/fullName"));
    user.emails.push(ContactValue::new(primary, Some("work"), true));
    if let Some(Value::Array(emails)) = item.get("emails") {
        for email in emails {
            let Some(address) = text(email.get("address")) else {
                continue;
            };
            if user.emails.iter().any(|e| e.value.eq_ignore_ascii_case(&address)) {
                continue;
            }
            let kind = text(email.get("type")).unwrap_or_else(|| "other".to_owned());
            user.emails.push(ContactValue::new(address, Some(&kind), false));
        }
    }
    if let Some(Value::Array(phones)) = item.get("phones") {
        for phone in phones {
            let Some(value) = text(phone.get("value")) else {
                continue;
            };
            let kind = text(phone.get("type")).unwrap_or_else(|| "work".to_owned());
            let primary = phone.get("primary").and_then(Value::as_bool).unwrap_or(false);
            user.phone_numbers.push(ContactValue::new(value, Some(&kind), primary));
        }
    }
    if let Some(Value::Array(organizations)) = item.get("organizations") {
        let organization = organizations
            .iter()
            .find(|o| o.get("primary").and_then(Value::as_bool).unwrap_or(false))
            .or_else(|| organizations.first());
        if let Some(organization) = organization {
            user.title = text(organization.get("title"));
            user.department = text(organization.get("department"));
            user.organization = text(organization.get("name"));
        }
    }
    if let Some(Value::Array(ids)) = item.get("externalIds") {
        user.employee_number = ids
            .iter()
            .find(|id| id.get("type").and_then(Value::as_str) == Some("organization"))
            .and_then(|id| text(id.get("value")));
    }
    if let Some(Value::Array(relations)) = item.get("relations") {
        user.manager_id = relations
            .iter()
            .find(|r| r.get("type").and_then(Value::as_str) == Some("manager"))
            .and_then(|r| text(r.get("value")));
    }
    if let Some(Value::Array(languages)) = item.get("languages") {
        user.locale = languages
            .iter()
            .find(|l| l.get("preference").and_then(Value::as_str) == Some("preferred"))
            .or_else(|| languages.first())
            .and_then(|l| text(l.get("languageCode")));
    }
    if let Some(path) = text(item.get("orgUnitPath")) {
        user.attributes.insert("orgUnitPath".to_owned(), Value::String(path));
    }
    Some(user)
}

fn failed(message: impl Into<String>) -> DirectoryError {
    DirectoryError::source_failed("Google Workspace", message)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn google_users_are_normalized() {
        let user = user_of(&json!({
            "id": "104857300203849504382",
            "primaryEmail": "ada@acme.test",
            "name": { "givenName": "Ada", "familyName": "Lovelace", "fullName": "Ada Lovelace" },
            "suspended": false,
            "archived": true,
            "emails": [
                { "address": "ada@acme.test", "primary": true },
                { "address": "countess@home.test", "type": "home" }
            ],
            "phones": [{ "value": "+44 20 7946 0958", "type": "work", "primary": true }],
            "organizations": [
                { "title": "Analyst", "department": "Finance" },
                { "title": "Principal engineer", "department": "Engineering", "name": "Acme", "primary": true }
            ],
            "externalIds": [{ "type": "organization", "value": "1815" }],
            "relations": [{ "type": "manager", "value": "charles@acme.test" }],
            "languages": [{ "languageCode": "en-GB", "preference": "preferred" }],
            "orgUnitPath": "/Engineering"
        }))
        .unwrap();
        assert_eq!(user.external_id, "104857300203849504382");
        assert_eq!(user.user_name, "ada@acme.test");
        assert!(!user.active);
        assert_eq!(user.full_name(), "Ada Lovelace");
        assert_eq!(user.emails.len(), 2);
        assert_eq!(user.emails[1].kind.as_deref(), Some("home"));
        assert_eq!(user.primary_phone(), Some("+44 20 7946 0958"));
        assert_eq!(user.title.as_deref(), Some("Principal engineer"));
        assert_eq!(user.department.as_deref(), Some("Engineering"));
        assert_eq!(user.organization.as_deref(), Some("Acme"));
        assert_eq!(user.employee_number.as_deref(), Some("1815"));
        assert_eq!(user.manager_id.as_deref(), Some("charles@acme.test"));
        assert_eq!(user.locale.as_deref(), Some("en-GB"));
        assert_eq!(user.attributes["orgUnitPath"], json!("/Engineering"));
        assert!(user_of(&json!({ "id": "1" })).is_none());
    }

    #[test]
    fn settings_are_checked() {
        assert!(GoogleSource.check_settings(&json!({}), false).is_ok());
        assert!(
            GoogleSource
                .check_settings(&json!({ "admin_email": "admin@acme.test" }), false)
                .is_ok()
        );
        assert_eq!(
            GoogleSource
                .check_settings(&json!({ "admin_email": "admin" }), false)
                .unwrap_err(),
            "settings.admin_email must be an email address"
        );
    }
}
