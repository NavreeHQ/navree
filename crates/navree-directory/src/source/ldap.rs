//! LDAP and Active Directory through ldap3, with TLS on rustls (ring).
//!
//! Each pull binds (with `settings.bind_dn` and the connection's secret,
//! or anonymously), searches users and groups with paged results, and
//! gives a snapshot. Two flavors set the defaults:
//!
//! | | `active_directory` (default) | `openldap` |
//! |---|---|---|
//! | Users | `(&(objectCategory=person)(objectClass=user))` | `(\|(objectClass=inetOrgPerson)(objectClass=person))` |
//! | Groups | `(objectClass=group)` | `(\|(objectClass=groupOfNames)(objectClass=groupOfUniqueNames)(objectClass=posixGroup))` |
//! | Id | `objectGUID` (as a UUID) | `entryUUID` |
//! | Sign-in name | `userPrincipalName`, else `sAMAccountName` | `uid`, else `mail` |
//! | Disabled | `userAccountControl` bit 2 | `pwdAccountLockedTime` or `nsAccountLock` |
//!
//! Group members are read from the groups' `member`, `uniqueMember` and
//! `memberUid` and from the users' `memberOf` (which also covers Active
//! Directory groups too large for one `member` value list). Nested groups
//! are not expanded.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use ldap3::adapters::{Adapter, EntriesOnly, PagedResults};
use ldap3::{LdapConnAsync, LdapConnSettings, Scope, SearchEntry};
use navree_kernel::BoxFuture;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use serde_json::Value;
use uuid::Uuid;

use super::{DirectorySource, PullRequest};
use crate::error::DirectoryError;
use crate::pulled::Pulled;
use crate::user::{ContactValue, DirectoryGroup, DirectoryUser};

/// The `ldap` source.
pub(crate) struct LdapSource;

/// A directory flavor: its default filters and attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flavor {
    ActiveDirectory,
    OpenLdap,
}

impl Flavor {
    fn parse(value: Option<&str>) -> Result<Flavor, String> {
        match value.unwrap_or("active_directory") {
            "active_directory" | "ad" => Ok(Flavor::ActiveDirectory),
            "openldap" | "ldap" => Ok(Flavor::OpenLdap),
            other => Err(format!(
                "settings.flavor must be active_directory or openldap, got `{other}`"
            )),
        }
    }

    fn user_filter(self) -> &'static str {
        match self {
            Flavor::ActiveDirectory => "(&(objectCategory=person)(objectClass=user))",
            Flavor::OpenLdap => "(|(objectClass=inetOrgPerson)(objectClass=person))",
        }
    }

    fn group_filter(self) -> &'static str {
        match self {
            Flavor::ActiveDirectory => "(objectClass=group)",
            Flavor::OpenLdap => {
                "(|(objectClass=groupOfNames)(objectClass=groupOfUniqueNames)(objectClass=posixGroup))"
            }
        }
    }

    fn id_attribute(self) -> &'static str {
        match self {
            Flavor::ActiveDirectory => "objectGUID",
            Flavor::OpenLdap => "entryUUID",
        }
    }

    fn user_name_attributes(self) -> &'static [&'static str] {
        match self {
            Flavor::ActiveDirectory => &["userPrincipalName", "sAMAccountName"],
            Flavor::OpenLdap => &["uid", "mail", "cn"],
        }
    }
}

/// The settings of an `ldap` connection.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LdapSettings {
    pub(crate) url: String,
    pub(crate) starttls: bool,
    pub(crate) bind_dn: Option<String>,
    pub(crate) base_dn: String,
    pub(crate) user_base_dn: Option<String>,
    pub(crate) group_base_dn: Option<String>,
    pub(crate) user_filter: Option<String>,
    pub(crate) group_filter: Option<String>,
    pub(crate) flavor: Flavor,
    pub(crate) id_attribute: Option<String>,
    pub(crate) page_size: i32,
    pub(crate) timeout: Duration,
    pub(crate) ca_certificate: Option<String>,
    pub(crate) insecure_skip_verify: bool,
    pub(crate) groups: bool,
    pub(crate) attributes: Vec<String>,
}

impl LdapSettings {
    pub(crate) fn from_value(settings: &Value) -> Result<LdapSettings, String> {
        let string = |key: &str| -> Result<Option<String>, String> {
            match settings.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(value)) if value.trim().is_empty() => Ok(None),
                Some(Value::String(value)) => Ok(Some(value.trim().to_owned())),
                Some(_) => Err(format!("settings.{key} must be a string")),
            }
        };
        let flag = |key: &str, default: bool| -> Result<bool, String> {
            match settings.get(key) {
                None | Some(Value::Null) => Ok(default),
                Some(Value::Bool(value)) => Ok(*value),
                Some(Value::String(text)) if text == "true" => Ok(true),
                Some(Value::String(text)) if text == "false" => Ok(false),
                Some(_) => Err(format!("settings.{key} must be true or false")),
            }
        };
        let number = |key: &str, default: u64| -> Result<u64, String> {
            match settings.get(key) {
                None | Some(Value::Null) => Ok(default),
                Some(Value::Number(n)) => n
                    .as_u64()
                    .ok_or_else(|| format!("settings.{key} must be a positive number")),
                Some(Value::String(text)) => text
                    .trim()
                    .parse()
                    .map_err(|_| format!("settings.{key} must be a positive number")),
                Some(_) => Err(format!("settings.{key} must be a positive number")),
            }
        };
        let url = string("url")?.ok_or("settings.url is required (ldap:// or ldaps://)")?;
        let scheme = url
            .split("://")
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(scheme.as_str(), "ldap" | "ldaps") || !url.contains("://") {
            return Err(format!(
                "settings.url must start with ldap:// or ldaps://, got `{url}`"
            ));
        }
        let base_dn = string("base_dn")?.ok_or("settings.base_dn is required")?;
        let starttls = flag("starttls", false)?;
        if starttls && scheme == "ldaps" {
            return Err(
                "settings.starttls is for ldap:// URLs; ldaps:// is TLS already".to_owned(),
            );
        }
        let page_size = number("page_size", 500)?;
        if page_size == 0 || page_size > 10_000 {
            return Err("settings.page_size must be between 1 and 10000".to_owned());
        }
        let timeout = number("timeout_secs", 30)?.max(1);
        let ca_certificate = string("ca_certificate")?;
        if let Some(pem) = &ca_certificate
            && CertificateDer::pem_slice_iter(pem.as_bytes())
                .next()
                .is_none()
        {
            return Err("settings.ca_certificate must be a PEM certificate".to_owned());
        }
        let attributes = match settings.get("attributes") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .ok_or("settings.attributes must be a list of attribute names")
                })
                .collect::<Result<_, _>>()?,
            Some(Value::String(list)) => list
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect(),
            Some(_) => {
                return Err("settings.attributes must be a list of attribute names".to_owned());
            }
        };
        Ok(LdapSettings {
            url,
            starttls,
            bind_dn: string("bind_dn")?,
            base_dn,
            user_base_dn: string("user_base_dn")?,
            group_base_dn: string("group_base_dn")?,
            user_filter: string("user_filter")?,
            group_filter: string("group_filter")?,
            flavor: Flavor::parse(string("flavor")?.as_deref())?,
            id_attribute: string("id_attribute")?,
            page_size: i32::try_from(page_size).unwrap_or(500),
            timeout: Duration::from_secs(timeout),
            ca_certificate,
            insecure_skip_verify: flag("insecure_skip_verify", false)?,
            groups: flag("groups", true)?,
            attributes,
        })
    }

    fn id_attribute(&self) -> &str {
        self.id_attribute
            .as_deref()
            .unwrap_or_else(|| self.flavor.id_attribute())
    }
}

impl DirectorySource for LdapSource {
    fn check_settings(&self, settings: &Value, has_secret: bool) -> Result<(), String> {
        let settings = LdapSettings::from_value(settings)?;
        if settings.bind_dn.is_some() && !has_secret {
            return Err(
                "settings.bind_dn needs the bind password as the connection's secret".to_owned(),
            );
        }
        Ok(())
    }

    fn pull<'a>(
        &'a self,
        request: &'a PullRequest,
    ) -> BoxFuture<'a, Result<Pulled, DirectoryError>> {
        Box::pin(async move {
            let settings = LdapSettings::from_value(&request.connection.settings)
                .map_err(DirectoryError::InvalidSettings)?;
            let password = request
                .secret
                .as_ref()
                .map(|secret| secret.expose().clone());
            pull(&settings, password.as_deref()).await
        })
    }
}

/// Binds, searches and maps.
pub(crate) async fn pull(
    settings: &LdapSettings,
    password: Option<&str>,
) -> Result<Pulled, DirectoryError> {
    let mut conn_settings = LdapConnSettings::new()
        .set_conn_timeout(settings.timeout)
        .set_starttls(settings.starttls);
    if settings.insecure_skip_verify {
        conn_settings = conn_settings.set_no_tls_verify(true);
    } else if let Some(pem) = &settings.ca_certificate {
        conn_settings = conn_settings.set_config(tls_config(pem)?);
    }
    let (conn, mut ldap) = LdapConnAsync::with_settings(conn_settings, &settings.url)
        .await
        .map_err(|error| failed(format!("could not connect to {}: {error}", settings.url)))?;
    ldap3::drive!(conn);
    if let Some(dn) = &settings.bind_dn {
        ldap.with_timeout(settings.timeout)
            .simple_bind(dn, password.unwrap_or_default())
            .await
            .and_then(ldap3::LdapResult::success)
            .map_err(|error| {
                failed(format!("the bind as {dn} failed: {}", ldap_message(&error)))
            })?;
    }
    let mut user_attrs: Vec<String> = [
        settings.id_attribute(),
        "userPrincipalName",
        "sAMAccountName",
        "uid",
        "mail",
        "givenName",
        "sn",
        "displayName",
        "cn",
        "title",
        "department",
        "company",
        "o",
        "employeeID",
        "employeeNumber",
        "manager",
        "telephoneNumber",
        "mobile",
        "preferredLanguage",
        "userAccountControl",
        "memberOf",
        "pwdAccountLockedTime",
        "nsAccountLock",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for extra in &settings.attributes {
        if !user_attrs.iter().any(|a| a.eq_ignore_ascii_case(extra)) {
            user_attrs.push(extra.clone());
        }
    }
    let user_base = settings
        .user_base_dn
        .as_deref()
        .unwrap_or(&settings.base_dn);
    let user_filter = settings
        .user_filter
        .as_deref()
        .unwrap_or_else(|| settings.flavor.user_filter());
    let user_entries = search(&mut ldap, settings, user_base, user_filter, user_attrs).await?;
    let mut group_entries = Vec::new();
    if settings.groups {
        let group_attrs: Vec<String> = [
            settings.id_attribute(),
            "cn",
            "displayName",
            "description",
            "mail",
            "member",
            "uniqueMember",
            "memberUid",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let group_base = settings
            .group_base_dn
            .as_deref()
            .unwrap_or(&settings.base_dn);
        let group_filter = settings
            .group_filter
            .as_deref()
            .unwrap_or_else(|| settings.flavor.group_filter());
        group_entries = search(&mut ldap, settings, group_base, group_filter, group_attrs).await?;
    }
    let _ = ldap.unbind().await;
    Ok(map_entries(settings, &user_entries, &group_entries))
}

async fn search(
    ldap: &mut ldap3::Ldap,
    settings: &LdapSettings,
    base: &str,
    filter: &str,
    attrs: Vec<String>,
) -> Result<Vec<LdapEntry>, DirectoryError> {
    let adapters: Vec<Box<dyn Adapter<'_, String, Vec<String>>>> = vec![
        Box::new(EntriesOnly::new()),
        Box::new(PagedResults::new(settings.page_size)),
    ];
    let mut stream = ldap
        .with_timeout(settings.timeout)
        .streaming_search_with(adapters, base, Scope::Subtree, filter, attrs)
        .await
        .map_err(|error| {
            failed(format!(
                "the search under {base} failed: {}",
                ldap_message(&error)
            ))
        })?;
    let mut entries = Vec::new();
    while let Some(entry) = stream.next().await.map_err(|error| {
        failed(format!(
            "the search under {base} failed: {}",
            ldap_message(&error)
        ))
    })? {
        entries.push(LdapEntry::from(SearchEntry::construct(entry)));
    }
    stream.finish().await.success().map_err(|error| {
        failed(format!(
            "the search under {base} failed: {}",
            ldap_message(&error)
        ))
    })?;
    Ok(entries)
}

/// A rustls configuration (ring) that trusts the PEM certificates given.
fn tls_config(pem: &str) -> Result<Arc<rustls::ClientConfig>, DirectoryError> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(pem.as_bytes()) {
        let cert = cert.map_err(|_| {
            DirectoryError::InvalidSettings(
                "settings.ca_certificate must be a PEM certificate".to_owned(),
            )
        })?;
        roots.add(cert).map_err(|error| {
            DirectoryError::InvalidSettings(format!("settings.ca_certificate: {error}"))
        })?;
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| DirectoryError::Config(error.to_string()))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Arc::new(config))
}

/// An LDAP error, without the request.
fn ldap_message(error: &ldap3::LdapError) -> String {
    match error {
        ldap3::LdapError::LdapResult { result } => {
            let text = result.text.trim();
            if text.is_empty() {
                format!("result code {}", result.rc)
            } else {
                format!("result code {}: {text}", result.rc)
            }
        }
        other => other.to_string(),
    }
}

fn failed(message: impl Into<String>) -> DirectoryError {
    DirectoryError::source_failed("LDAP", message)
}

/// An entry, its attribute names lower-cased.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LdapEntry {
    pub(crate) dn: String,
    pub(crate) text: HashMap<String, Vec<String>>,
    pub(crate) binary: HashMap<String, Vec<Vec<u8>>>,
}

impl From<SearchEntry> for LdapEntry {
    fn from(entry: SearchEntry) -> Self {
        let mut text: HashMap<String, Vec<String>> = HashMap::new();
        for (name, values) in entry.attrs {
            text.entry(name.to_ascii_lowercase())
                .or_default()
                .extend(values);
        }
        let mut binary: HashMap<String, Vec<Vec<u8>>> = HashMap::new();
        for (name, values) in entry.bin_attrs {
            binary
                .entry(name.to_ascii_lowercase())
                .or_default()
                .extend(values);
        }
        LdapEntry {
            dn: entry.dn,
            text,
            binary,
        }
    }
}

impl LdapEntry {
    fn first(&self, name: &str) -> Option<String> {
        self.text
            .get(&name.to_ascii_lowercase())
            .and_then(|values| values.iter().find(|value| !value.trim().is_empty()))
            .map(|value| value.trim().to_owned())
    }

    fn all(&self, name: &str) -> Vec<String> {
        self.text
            .get(&name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    /// The entry's id: `objectGUID` as a UUID (the bytes are little-endian
    /// in their first three fields), any other attribute as text, else
    /// the DN.
    fn id(&self, attribute: &str) -> String {
        let key = attribute.to_ascii_lowercase();
        if key == "objectguid" {
            let bytes = self
                .binary
                .get(&key)
                .and_then(|values| values.first().cloned())
                .or_else(|| {
                    self.text
                        .get(&key)
                        .and_then(|values| values.first())
                        .map(|value| value.as_bytes().to_vec())
                });
            if let Some(bytes) = bytes
                && let Ok(bytes) = <[u8; 16]>::try_from(bytes.as_slice())
            {
                return Uuid::from_bytes_le(bytes).to_string();
            }
        } else if let Some(id) = self.first(attribute) {
            return id;
        }
        self.dn.clone()
    }
}

/// A DN as compared: lower-case, without spaces around `,` and `=`.
pub(crate) fn normalize_dn(dn: &str) -> String {
    dn.split(',')
        .map(|rdn| rdn.split('=').map(str::trim).collect::<Vec<_>>().join("="))
        .collect::<Vec<_>>()
        .join(",")
        .to_lowercase()
}

/// Users and groups from the entries.
pub(crate) fn map_entries(
    settings: &LdapSettings,
    users: &[LdapEntry],
    groups: &[LdapEntry],
) -> Pulled {
    let id_attribute = settings.id_attribute();
    let mut by_dn: HashMap<String, String> = HashMap::new();
    let mut by_uid: HashMap<String, String> = HashMap::new();
    let mut mapped = Vec::new();
    for entry in users {
        let user_name = settings
            .flavor
            .user_name_attributes()
            .iter()
            .find_map(|name| entry.first(name));
        let Some(user_name) = user_name else {
            continue;
        };
        let id = entry.id(id_attribute);
        by_dn.insert(normalize_dn(&entry.dn), id.clone());
        if let Some(uid) = entry.first("uid") {
            by_uid.insert(uid.to_lowercase(), id.clone());
        }
        mapped.push((entry, id, user_name));
    }
    let mut members_of: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut directory_users = Vec::new();
    for (entry, id, user_name) in mapped {
        let mut user = DirectoryUser::new(id.clone(), user_name);
        user.active = match settings.flavor {
            Flavor::ActiveDirectory => !entry
                .first("userAccountControl")
                .and_then(|uac| uac.parse::<u64>().ok())
                .is_some_and(|uac| uac & 0x2 != 0),
            Flavor::OpenLdap => {
                entry.first("pwdAccountLockedTime").is_none()
                    && !entry
                        .first("nsAccountLock")
                        .is_some_and(|lock| lock.eq_ignore_ascii_case("true"))
            }
        };
        user.given_name = entry.first("givenName");
        user.family_name = entry.first("sn");
        user.display_name = entry.first("displayName").or_else(|| entry.first("cn"));
        for (n, mail) in entry.all("mail").into_iter().enumerate() {
            user.emails
                .push(ContactValue::new(mail, Some("work"), n == 0));
        }
        if let Some(phone) = entry.first("telephoneNumber") {
            user.phone_numbers
                .push(ContactValue::new(phone, Some("work"), true));
        }
        if let Some(mobile) = entry.first("mobile") {
            let primary = user.phone_numbers.is_empty();
            user.phone_numbers
                .push(ContactValue::new(mobile, Some("mobile"), primary));
        }
        user.title = entry.first("title");
        user.department = entry.first("department");
        user.organization = entry.first("company").or_else(|| entry.first("o"));
        user.employee_number = entry
            .first("employeeID")
            .or_else(|| entry.first("employeeNumber"));
        user.manager_id = entry
            .first("manager")
            .map(|dn| by_dn.get(&normalize_dn(&dn)).cloned().unwrap_or(dn));
        user.locale = entry.first("preferredLanguage");
        for name in &settings.attributes {
            let values = entry.all(name);
            match values.len() {
                0 => {}
                1 => {
                    user.attributes
                        .insert(name.clone(), Value::String(values[0].clone()));
                }
                _ => {
                    user.attributes.insert(
                        name.clone(),
                        Value::Array(values.into_iter().map(Value::String).collect()),
                    );
                }
            }
        }
        user.attributes
            .insert("dn".to_owned(), Value::String(entry.dn.clone()));
        for group_dn in entry.all("memberOf") {
            members_of
                .entry(normalize_dn(&group_dn))
                .or_default()
                .insert(id.clone());
        }
        directory_users.push(user);
    }
    let mut directory_groups = Vec::new();
    for entry in groups {
        let id = entry.id(id_attribute);
        let name = entry
            .first("displayName")
            .or_else(|| entry.first("cn"))
            .unwrap_or_else(|| entry.dn.clone());
        let mut group = DirectoryGroup::new(id, name);
        group.description = entry.first("description");
        group.email = entry.first("mail");
        let mut members: BTreeSet<String> = members_of
            .remove(&normalize_dn(&entry.dn))
            .unwrap_or_default();
        for dn in entry
            .all("member")
            .into_iter()
            .chain(entry.all("uniqueMember"))
        {
            if let Some(id) = by_dn.get(&normalize_dn(&dn)) {
                members.insert(id.clone());
            }
        }
        for uid in entry.all("memberUid") {
            if let Some(id) = by_uid.get(&uid.to_lowercase()) {
                members.insert(id.clone());
            }
        }
        group.members = members.into_iter().collect();
        group
            .attributes
            .insert("dn".to_owned(), Value::String(entry.dn.clone()));
        directory_groups.push(group);
    }
    Pulled::snapshot(directory_users, directory_groups)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn entry(dn: &str, attrs: &[(&str, &[&str])]) -> LdapEntry {
        LdapEntry {
            dn: dn.to_owned(),
            text: attrs
                .iter()
                .map(|(name, values)| {
                    (
                        name.to_ascii_lowercase(),
                        values.iter().map(|v| (*v).to_owned()).collect(),
                    )
                })
                .collect(),
            binary: HashMap::new(),
        }
    }

    fn settings(flavor: &str) -> LdapSettings {
        LdapSettings::from_value(&json!({
            "url": "ldap://dc1.acme.test",
            "base_dn": "dc=acme,dc=test",
            "flavor": flavor,
            "attributes": ["physicalDeliveryOfficeName"]
        }))
        .unwrap()
    }

    #[test]
    fn settings_are_read_and_checked() {
        let s = LdapSettings::from_value(&json!({
            "url": "ldaps://dc1.acme.test:636",
            "base_dn": "dc=acme,dc=test",
            "bind_dn": "CN=navree,OU=Service,DC=acme,DC=test",
            "page_size": "200",
            "groups": false,
            "attributes": "extensionAttribute1, physicalDeliveryOfficeName"
        }))
        .unwrap();
        assert_eq!(s.flavor, Flavor::ActiveDirectory);
        assert_eq!(s.page_size, 200);
        assert!(!s.groups);
        assert_eq!(
            s.attributes,
            vec!["extensionAttribute1", "physicalDeliveryOfficeName"]
        );
        assert_eq!(s.timeout, Duration::from_secs(30));
        for (settings, message) in [
            (json!({}), "settings.url is required (ldap:// or ldaps://)"),
            (
                json!({ "url": "https://x", "base_dn": "dc=x" }),
                "settings.url must start with ldap:// or ldaps://, got `https://x`",
            ),
            (json!({ "url": "ldap://x" }), "settings.base_dn is required"),
            (
                json!({ "url": "ldaps://x", "base_dn": "dc=x", "starttls": true }),
                "settings.starttls is for ldap:// URLs; ldaps:// is TLS already",
            ),
            (
                json!({ "url": "ldap://x", "base_dn": "dc=x", "flavor": "novell" }),
                "settings.flavor must be active_directory or openldap, got `novell`",
            ),
            (
                json!({ "url": "ldap://x", "base_dn": "dc=x", "page_size": 0 }),
                "settings.page_size must be between 1 and 10000",
            ),
            (
                json!({ "url": "ldap://x", "base_dn": "dc=x", "ca_certificate": "nope" }),
                "settings.ca_certificate must be a PEM certificate",
            ),
            (
                json!({ "url": "ldap://x", "base_dn": "dc=x", "groups": 3 }),
                "settings.groups must be true or false",
            ),
        ] {
            assert_eq!(LdapSettings::from_value(&settings).unwrap_err(), message);
        }
        assert_eq!(
            LdapSource
                .check_settings(
                    &json!({ "url": "ldap://x", "base_dn": "dc=x", "bind_dn": "cn=admin" }),
                    false
                )
                .unwrap_err(),
            "settings.bind_dn needs the bind password as the connection's secret"
        );
        assert!(
            LdapSource
                .check_settings(
                    &json!({ "url": "ldap://x", "base_dn": "dc=x", "bind_dn": "cn=admin" }),
                    true
                )
                .is_ok()
        );
    }

    #[test]
    fn object_guids_read_as_uuids() {
        // The bytes of 6e7b768e-07e2-4810-8459-485f84f8f204 as AD stores them.
        let bytes = vec![
            0x8e, 0x76, 0x7b, 0x6e, 0xe2, 0x07, 0x10, 0x48, 0x84, 0x59, 0x48, 0x5f, 0x84, 0xf8,
            0xf2, 0x04,
        ];
        let mut entry = LdapEntry {
            dn: "CN=Ada".into(),
            ..LdapEntry::default()
        };
        entry.binary.insert("objectguid".into(), vec![bytes]);
        assert_eq!(
            entry.id("objectGUID"),
            "6e7b768e-07e2-4810-8459-485f84f8f204"
        );
        let entry = entry_with_text_guid();
        // The first three fields swap byte order; the last eight stay.
        assert_eq!(
            entry.id("objectGUID"),
            "30313233-3435-3637-3938-616263646566"
        );
        assert_eq!(
            LdapEntry {
                dn: "CN=X".into(),
                ..LdapEntry::default()
            }
            .id("objectGUID"),
            "CN=X"
        );
    }

    fn entry_with_text_guid() -> LdapEntry {
        // Sixteen bytes that happen to be valid UTF-8 come back as text.
        entry("CN=Grace", &[("objectGUID", &["3210547698abcdef"])])
    }

    #[test]
    fn dns_compare_without_case_or_spaces() {
        assert_eq!(
            normalize_dn("CN=Ada Lovelace, OU=People , DC=acme,DC = test"),
            "cn=ada lovelace,ou=people,dc=acme,dc=test"
        );
    }

    #[test]
    fn active_directory_entries_map_to_users_and_groups() {
        let users = vec![
            entry(
                "CN=Ada Lovelace,OU=People,DC=acme,DC=test",
                &[
                    ("objectGUID", &["ada-guid"]),
                    ("userPrincipalName", &["ada@acme.test"]),
                    ("sAMAccountName", &["ada"]),
                    ("givenName", &["Ada"]),
                    ("sn", &["Lovelace"]),
                    ("displayName", &["Ada Lovelace"]),
                    ("mail", &["ada@acme.test"]),
                    ("title", &["Principal engineer"]),
                    ("department", &["Engineering"]),
                    ("company", &["Acme"]),
                    ("employeeID", &["1815"]),
                    ("telephoneNumber", &["+44 20 7946 0958"]),
                    (
                        "manager",
                        &["CN=Charles Babbage, OU=People,DC=acme,DC=test"],
                    ),
                    ("userAccountControl", &["512"]),
                    ("memberOf", &["CN=Engineering,OU=Groups,DC=acme,DC=test"]),
                    ("physicalDeliveryOfficeName", &["London"]),
                ],
            ),
            entry(
                "CN=Charles Babbage,OU=People,DC=acme,DC=test",
                &[
                    ("objectGUID", &["charles-guid"]),
                    ("sAMAccountName", &["charles"]),
                    ("userAccountControl", &["514"]),
                ],
            ),
            entry(
                "CN=No Name,OU=People,DC=acme,DC=test",
                &[("objectGUID", &["x"])],
            ),
        ];
        let groups = vec![entry(
            "CN=Engineering,OU=Groups,DC=acme,DC=test",
            &[
                ("objectGUID", &["eng-guid"]),
                ("cn", &["Engineering"]),
                ("description", &["Builders"]),
                (
                    "member",
                    &[
                        "CN=Charles Babbage,OU=People,DC=acme,DC=test",
                        "CN=Gone,DC=x",
                    ],
                ),
            ],
        )];
        let pulled = map_entries(&settings("active_directory"), &users, &groups);
        assert!(pulled.complete);
        assert_eq!(pulled.users.len(), 2);
        let ada = &pulled.users[0];
        // Not 16 bytes: the GUID falls back to the DN.
        assert_eq!(ada.external_id, "CN=Ada Lovelace,OU=People,DC=acme,DC=test");
        assert_eq!(ada.user_name, "ada@acme.test");
        assert!(ada.active);
        assert_eq!(ada.full_name(), "Ada Lovelace");
        assert_eq!(ada.primary_email(), Some("ada@acme.test"));
        assert_eq!(ada.organization.as_deref(), Some("Acme"));
        assert_eq!(ada.employee_number.as_deref(), Some("1815"));
        assert_eq!(
            ada.manager_id.as_deref(),
            Some("CN=Charles Babbage,OU=People,DC=acme,DC=test")
        );
        assert_eq!(
            ada.attributes["physicalDeliveryOfficeName"],
            json!("London")
        );
        let charles = &pulled.users[1];
        assert_eq!(charles.user_name, "charles");
        assert!(!charles.active, "userAccountControl 514 is disabled");
        let group = &pulled.groups[0];
        assert_eq!(group.display_name, "Engineering");
        assert_eq!(group.description.as_deref(), Some("Builders"));
        assert_eq!(
            group.members,
            vec![
                "CN=Ada Lovelace,OU=People,DC=acme,DC=test".to_owned(),
                "CN=Charles Babbage,OU=People,DC=acme,DC=test".to_owned()
            ]
        );
    }

    #[test]
    fn openldap_entries_use_entry_uuid_and_member_uid() {
        let users = vec![
            entry(
                "uid=ada,ou=people,dc=acme,dc=test",
                &[
                    ("entryUUID", &["u-ada"]),
                    ("uid", &["ada"]),
                    ("cn", &["Ada Lovelace"]),
                    ("mail", &["ada@acme.test", "countess@acme.test"]),
                ],
            ),
            entry(
                "uid=grace,ou=people,dc=acme,dc=test",
                &[
                    ("entryUUID", &["u-grace"]),
                    ("uid", &["grace"]),
                    ("pwdAccountLockedTime", &["000001010000Z"]),
                ],
            ),
        ];
        let groups = vec![
            entry(
                "cn=engineering,ou=groups,dc=acme,dc=test",
                &[
                    ("entryUUID", &["g-eng"]),
                    ("cn", &["engineering"]),
                    ("uniqueMember", &["uid=ada,ou=people,dc=acme,dc=test"]),
                ],
            ),
            entry(
                "cn=ops,ou=groups,dc=acme,dc=test",
                &[
                    ("entryUUID", &["g-ops"]),
                    ("cn", &["ops"]),
                    ("memberUid", &["GRACE"]),
                ],
            ),
        ];
        let pulled = map_entries(&settings("openldap"), &users, &groups);
        let ada = &pulled.users[0];
        assert_eq!(ada.external_id, "u-ada");
        assert_eq!(ada.user_name, "ada");
        assert_eq!(ada.display_name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(ada.emails.len(), 2);
        assert!(!pulled.users[1].active);
        assert_eq!(pulled.groups[0].members, vec!["u-ada"]);
        assert_eq!(pulled.groups[1].members, vec!["u-grace"]);
    }
}
