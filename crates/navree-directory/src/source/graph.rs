//! Microsoft Entra ID through Microsoft Graph, with delta queries.
//!
//! The first pull reads `/users/delta` and `/groups/delta` to the end (a
//! snapshot) and keeps their delta links as the cursor; later pulls read
//! from those links and get only what changed. Graph sends changed objects
//! with only some of their properties, so changed users and groups are
//! read again in full (`$filter=id in (...)`, 15 at a time). Members come
//! from `members@delta`. A delta link Graph no longer knows (410 Gone)
//! starts over with a snapshot.

use std::collections::{BTreeMap, HashSet};

use navree_integrations::IntegrationClient;
use navree_kernel::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{DirectorySource, PullRequest, text};
use crate::error::DirectoryError;
use crate::pulled::Pulled;
use crate::types::MICROSOFT;
use crate::user::{ContactValue, DirectoryGroup, DirectoryUser};

/// The user properties read.
pub(crate) const USER_SELECT: &str = "id,userPrincipalName,displayName,givenName,surname,mail,\
otherMails,accountEnabled,jobTitle,department,companyName,employeeId,mobilePhone,businessPhones,\
preferredLanguage,usageLocation,officeLocation,userType";
/// The group properties read.
pub(crate) const GROUP_SELECT: &str = "id,displayName,description,mail";
/// How many ids one `$filter=id in (...)` names (Graph's limit).
const IDS_PER_FILTER: usize = 15;
/// The most pages one delta round follows.
const MAX_PAGES: usize = 100_000;

/// The `microsoft` source.
pub(crate) struct GraphSource;

/// The cursor: the delta links of users and groups.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Cursor {
    users: String,
    #[serde(default)]
    groups: Option<String>,
}

impl DirectorySource for GraphSource {
    fn pull<'a>(
        &'a self,
        request: &'a PullRequest,
    ) -> BoxFuture<'a, Result<Pulled, DirectoryError>> {
        Box::pin(async move {
            let client = request.client(MICROSOFT).await?;
            let groups = request.flag("groups", true);
            let cursor = request
                .cursor
                .as_deref()
                .and_then(|cursor| serde_json::from_str::<Cursor>(cursor).ok());
            match cursor {
                Some(cursor) => match delta(&client, &cursor, groups).await {
                    Err(Round::Resync) => snapshot(&client, groups).await,
                    Err(Round::Failed(error)) => Err(error),
                    Ok(pulled) => Ok(pulled),
                },
                None => snapshot(&client, groups).await,
            }
        })
    }
}

/// Why a delta round stopped.
enum Round {
    /// Graph no longer knows the delta link.
    Resync,
    Failed(DirectoryError),
}

impl From<DirectoryError> for Round {
    fn from(error: DirectoryError) -> Self {
        Round::Failed(error)
    }
}

async fn snapshot(client: &IntegrationClient, with_groups: bool) -> Result<Pulled, DirectoryError> {
    let start = format!("/users/delta?$select={USER_SELECT}");
    let (items, users_link) = match pages(client, &start).await {
        Ok(round) => round,
        Err(Round::Resync) => return Err(failed("Graph refused a new delta query (410)")),
        Err(Round::Failed(error)) => return Err(error),
    };
    let users: Vec<DirectoryUser> = items
        .iter()
        .filter(|item| item.get("@removed").is_none())
        .filter_map(user_of)
        .collect();
    let mut groups = Vec::new();
    let mut groups_link = None;
    if with_groups {
        let start = "/groups/delta?$select=displayName,description,mail,members";
        let (items, link) = match pages(client, start).await {
            Ok(round) => round,
            Err(Round::Resync) => return Err(failed("Graph refused a new delta query (410)")),
            Err(Round::Failed(error)) => return Err(error),
        };
        groups_link = Some(link);
        // A group spread over pages carries its members in pieces.
        let mut merged: BTreeMap<String, DirectoryGroup> = BTreeMap::new();
        let mut order = Vec::new();
        for item in items.iter().filter(|item| item.get("@removed").is_none()) {
            let Some(id) = text(item.get("id")) else {
                continue;
            };
            let group = merged.entry(id.clone()).or_insert_with(|| {
                order.push(id.clone());
                DirectoryGroup::new(id.clone(), "")
            });
            apply_group_attributes(group, item);
            for (member, added) in member_changes(item) {
                if added && !group.members.contains(&member) {
                    group.members.push(member);
                }
            }
        }
        groups = order
            .into_iter()
            .filter_map(|id| merged.remove(&id))
            .collect();
    }
    let cursor = Cursor {
        users: users_link,
        groups: groups_link,
    };
    Ok(Pulled::snapshot(users, groups).cursor(serde_json::to_string(&cursor).unwrap_or_default()))
}

async fn delta(
    client: &IntegrationClient,
    cursor: &Cursor,
    with_groups: bool,
) -> Result<Pulled, Round> {
    let (items, users_link) = pages(client, &cursor.users).await?;
    let mut pulled = Pulled::delta(Vec::new(), Vec::new());
    let mut changed = Vec::new();
    for item in &items {
        let Some(id) = text(item.get("id")) else {
            continue;
        };
        if item.get("@removed").is_some() {
            pulled = pulled.removed_user(id);
        } else if !changed.contains(&id) {
            changed.push(id);
        }
    }
    for chunk in changed.chunks(IDS_PER_FILTER) {
        for item in by_ids(client, "/users", chunk, USER_SELECT).await? {
            if let Some(user) = user_of(&item) {
                pulled.users.push(user);
            }
        }
    }
    let mut groups_link = cursor.groups.clone();
    if with_groups {
        // Groups were off for the first pull: start their delta now.
        let start = cursor.groups.clone().unwrap_or_else(|| {
            "/groups/delta?$select=displayName,description,mail,members".to_owned()
        });
        let first = cursor.groups.is_none();
        let (items, link) = pages(client, &start).await?;
        groups_link = Some(link);
        let mut changed = Vec::new();
        let mut seen = HashSet::new();
        for item in &items {
            let Some(id) = text(item.get("id")) else {
                continue;
            };
            if item.get("@removed").is_some() {
                pulled = pulled.removed_group(id);
                continue;
            }
            if seen.insert(id.clone()) {
                changed.push(id.clone());
            }
            for (member, added) in member_changes(item) {
                pulled = if added || first {
                    pulled.member_added(id.clone(), member)
                } else {
                    pulled.member_removed(id.clone(), member)
                };
            }
        }
        for chunk in changed.chunks(IDS_PER_FILTER) {
            for item in by_ids(client, "/groups", chunk, GROUP_SELECT).await? {
                let Some(id) = text(item.get("id")) else {
                    continue;
                };
                let mut group = DirectoryGroup::new(id, "");
                apply_group_attributes(&mut group, &item);
                pulled.groups.push(group);
            }
        }
    }
    let cursor = Cursor {
        users: users_link,
        groups: groups_link,
    };
    pulled.cursor = Some(serde_json::to_string(&cursor).unwrap_or_default());
    Ok(pulled)
}

/// Every item of a delta round from `start`, and the delta link it ends
/// with.
async fn pages(client: &IntegrationClient, start: &str) -> Result<(Vec<Value>, String), Round> {
    let mut items = Vec::new();
    let mut next = start.to_owned();
    for _ in 0..MAX_PAGES {
        let response = client
            .get(next.clone())
            .send()
            .await
            .map_err(DirectoryError::from)?;
        let status = response.status().as_u16();
        if status == 410 {
            return Err(Round::Resync);
        }
        if !response.is_success() {
            return Err(Round::Failed(failed(error_message(
                status,
                &response.text(),
            ))));
        }
        let page: Value = response
            .json()
            .map_err(|error| failed(format!("Graph answered with invalid JSON: {error}")))?;
        if let Some(Value::Array(values)) = page.get("value") {
            items.extend(values.iter().cloned());
        }
        if let Some(link) = text(page.get("@odata.nextLink")) {
            next = link;
            continue;
        }
        let link = text(page.get("@odata.deltaLink"))
            .ok_or_else(|| failed("Graph ended a delta round without a delta link"))?;
        return Ok((items, link));
    }
    Err(Round::Failed(failed("Graph's delta round did not end")))
}

/// The objects with these ids (`$filter=id in (...)`).
async fn by_ids(
    client: &IntegrationClient,
    path: &str,
    ids: &[String],
    select: &str,
) -> Result<Vec<Value>, Round> {
    let quoted: Vec<String> = ids
        .iter()
        .map(|id| format!("'{}'", id.replace('\'', "''")))
        .collect();
    let filter = format!("id in ({})", quoted.join(","));
    let response = client
        .get(path)
        .query(&[("$filter", filter.as_str()), ("$select", select)])
        .send()
        .await
        .map_err(DirectoryError::from)?;
    let status = response.status().as_u16();
    if !response.is_success() {
        return Err(Round::Failed(failed(error_message(
            status,
            &response.text(),
        ))));
    }
    let page: Value = response
        .json()
        .map_err(|error| failed(format!("Graph answered with invalid JSON: {error}")))?;
    Ok(match page.get("value") {
        Some(Value::Array(values)) => values.clone(),
        _ => Vec::new(),
    })
}

/// The user members of a group item's `members@delta`: (id, added).
fn member_changes(item: &Value) -> Vec<(String, bool)> {
    let Some(Value::Array(members)) = item.get("members@delta") else {
        return Vec::new();
    };
    members
        .iter()
        .filter(|member| {
            member
                .get("@odata.type")
                .and_then(Value::as_str)
                .is_none_or(|kind| kind.eq_ignore_ascii_case("#microsoft.graph.user"))
        })
        .filter_map(|member| {
            let id = text(member.get("id"))?;
            Some((id, member.get("@removed").is_none()))
        })
        .collect()
}

fn apply_group_attributes(group: &mut DirectoryGroup, item: &Value) {
    if let Some(name) = text(item.get("displayName")) {
        group.display_name = name;
    }
    if item.get("description").is_some() {
        group.description = text(item.get("description"));
    }
    if item.get("mail").is_some() {
        group.email = text(item.get("mail"));
    }
}

/// A Graph user as a [`DirectoryUser`].
pub(crate) fn user_of(item: &Value) -> Option<DirectoryUser> {
    let id = text(item.get("id"))?;
    let user_name = text(item.get("userPrincipalName")).or_else(|| text(item.get("mail")))?;
    let mut user = DirectoryUser::new(id, user_name);
    user.active = item
        .get("accountEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    user.given_name = text(item.get("givenName"));
    user.family_name = text(item.get("surname"));
    user.display_name = text(item.get("displayName"));
    if let Some(mail) = text(item.get("mail")) {
        user.emails
            .push(ContactValue::new(mail, Some("work"), true));
    }
    if let Some(Value::Array(others)) = item.get("otherMails") {
        for other in others.iter().filter_map(|v| text(Some(v))) {
            if !user
                .emails
                .iter()
                .any(|email| email.value.eq_ignore_ascii_case(&other))
            {
                user.emails
                    .push(ContactValue::new(other, Some("other"), false));
            }
        }
    }
    if let Some(Value::Array(phones)) = item.get("businessPhones") {
        for phone in phones.iter().filter_map(|v| text(Some(v))) {
            let primary = user.phone_numbers.is_empty();
            user.phone_numbers
                .push(ContactValue::new(phone, Some("work"), primary));
        }
    }
    if let Some(mobile) = text(item.get("mobilePhone")) {
        let primary = user.phone_numbers.is_empty();
        user.phone_numbers
            .push(ContactValue::new(mobile, Some("mobile"), primary));
    }
    user.title = text(item.get("jobTitle"));
    user.department = text(item.get("department"));
    user.organization = text(item.get("companyName"));
    user.employee_number = text(item.get("employeeId"));
    user.locale = text(item.get("preferredLanguage"));
    for key in ["usageLocation", "officeLocation", "userType"] {
        if let Some(value) = text(item.get(key)) {
            user.attributes.insert(key.to_owned(), Value::String(value));
        }
    }
    Some(user)
}

fn failed(message: impl Into<String>) -> DirectoryError {
    DirectoryError::source_failed("Microsoft Graph", message)
}

/// `"403: Insufficient privileges to complete the operation."` from a
/// Graph error body.
pub(crate) fn error_message(status: u16, body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|body| {
            body.pointer("/error/message")
                .or_else(|| body.pointer("/errorSummary"))
                .or_else(|| body.pointer("/error_description"))
        })
        .and_then(Value::as_str)
        .map(str::to_owned);
    match message {
        Some(message) => format!("{status}: {message}"),
        None => format!("the API answered {status}"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn graph_users_are_normalized() {
        let user = user_of(&json!({
            "id": "6e7b768e-07e2-4810-8459-485f84f8f204",
            "userPrincipalName": "anu@acme.test",
            "displayName": "Anu Lakhotia",
            "givenName": "Anu",
            "surname": "Lakhotia",
            "mail": "anu@acme.test",
            "otherMails": ["ANU@acme.test", "countess@home.test"],
            "accountEnabled": false,
            "jobTitle": "Principal engineer",
            "department": "Engineering",
            "companyName": "Agra",
            "employeeId": "1815",
            "businessPhones": ["+44 20 7946 0958"],
            "mobilePhone": "+44 7700 900123",
            "preferredLanguage": "en-GB",
            "usageLocation": "GB",
            "officeLocation": null
        }))
        .unwrap();
        assert_eq!(user.external_id, "6e7b768e-07e2-4810-8459-485f84f8f204");
        assert_eq!(user.user_name, "anu@acme.test");
        assert!(!user.active);
        assert_eq!(user.full_name(), "Anu Lakhotia");
        assert_eq!(user.emails.len(), 2);
        assert_eq!(user.primary_email(), Some("anu@acme.test"));
        assert_eq!(user.emails[1].kind.as_deref(), Some("other"));
        assert_eq!(user.phone_numbers.len(), 2);
        assert_eq!(user.primary_phone(), Some("+44 20 7946 0958"));
        assert_eq!(user.title.as_deref(), Some("Principal engineer"));
        assert_eq!(user.organization.as_deref(), Some("Agra"));
        assert_eq!(user.employee_number.as_deref(), Some("1815"));
        assert_eq!(user.locale.as_deref(), Some("en-GB"));
        assert_eq!(user.attributes["usageLocation"], json!("GB"));
        assert!(!user.attributes.contains_key("officeLocation"));
        assert!(user_of(&json!({ "id": "x" })).is_none());
    }

    #[test]
    fn members_delta_lists_users_added_and_removed() {
        let changes = member_changes(&json!({
            "id": "g1",
            "members@delta": [
                { "@odata.type": "#microsoft.graph.user", "id": "u1" },
                { "@odata.type": "#microsoft.graph.group", "id": "g2" },
                { "@odata.type": "#microsoft.graph.user", "id": "u2", "@removed": { "reason": "deleted" } }
            ]
        }));
        assert_eq!(
            changes,
            vec![("u1".to_owned(), true), ("u2".to_owned(), false)]
        );
    }

    #[test]
    fn error_bodies_give_their_message() {
        assert_eq!(
            error_message(
                403,
                r#"{"error":{"code":"Authorization_RequestDenied","message":"Insufficient privileges"}}"#
            ),
            "403: Insufficient privileges"
        );
        assert_eq!(error_message(500, "oops"), "the API answered 500");
    }
}
