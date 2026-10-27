//! Edits of `harbor.toml` that keep its comments and layout: secrets and
//! pinned host keys.

use std::collections::BTreeMap;

use toml_edit::{DocumentMut, Item, Table, value};

use crate::error::{HarborError, Result};

fn parse(text: &str) -> Result<DocumentMut> {
    text.parse::<DocumentMut>()
        .map_err(|e| HarborError::msg(format!("harbor.toml is not valid TOML: {e}")))
}

fn secrets_table(document: &mut DocumentMut) -> Result<&mut Table> {
    let item = document
        .entry("secrets")
        .or_insert_with(|| Item::Table(Table::new()));
    item.as_table_mut()
        .ok_or_else(|| HarborError::msg("`secrets` in harbor.toml must be a [secrets] table"))
}

/// Sets `[secrets] name = payload`, adding the table when it is missing.
pub fn set_secret(text: &str, name: &str, payload: &str) -> Result<String> {
    let mut document = parse(text)?;
    secrets_table(&mut document)?.insert(name, value(payload));
    Ok(document.to_string())
}

/// Removes `name` from `[secrets]`.
pub fn remove_secret(text: &str, name: &str) -> Result<String> {
    let mut document = parse(text)?;
    let removed = document
        .get_mut("secrets")
        .and_then(Item::as_table_mut)
        .and_then(|t| t.remove(name));
    if removed.is_none() {
        return Err(HarborError::UnknownSecret(name.to_owned()));
    }
    Ok(document.to_string())
}

/// Replaces the value of every secret in `payloads` (they must exist).
pub fn replace_secrets(text: &str, payloads: &BTreeMap<String, String>) -> Result<String> {
    let mut document = parse(text)?;
    let table = secrets_table(&mut document)?;
    for (name, payload) in payloads {
        let Some(item) = table.get_mut(name) else {
            return Err(HarborError::UnknownSecret(name.clone()));
        };
        let decor = item.as_value().map(|v| v.decor().clone());
        *item = value(payload.as_str());
        if let (Some(decor), Some(new)) = (decor, item.as_value_mut()) {
            *new.decor_mut() = decor;
        }
    }
    Ok(document.to_string())
}

/// Sets `[servers.<server>] host_key`.
pub fn set_host_key(text: &str, server: &str, key: &str) -> Result<String> {
    let mut document = parse(text)?;
    let table = document
        .get_mut("servers")
        .and_then(|s| s.get_mut(server))
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| HarborError::UnknownServer(server.to_owned()))?;
    table.insert("host_key", value(key));
    Ok(document.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"# The CRM.
app = "crm-api"

[servers.app-1]
host = "203.0.113.10"   # the VPS
roles = ["web"]

[env]
APP_LOG__LEVEL = "info"
"#;

    #[test]
    fn a_secret_adds_the_table_and_keeps_comments() {
        let text = set_secret(FILE, "APP_KEY", "harbor:v1:abc").unwrap();
        assert_eq!(
            text,
            format!("{FILE}\n[secrets]\nAPP_KEY = \"harbor:v1:abc\"\n")
        );
        let text = set_secret(&text, "STRIPE", "harbor:v1:def").unwrap();
        assert!(text.ends_with("APP_KEY = \"harbor:v1:abc\"\nSTRIPE = \"harbor:v1:def\"\n"));
        let text = set_secret(&text, "APP_KEY", "harbor:v1:new").unwrap();
        assert!(text.contains("APP_KEY = \"harbor:v1:new\""), "{text}");
        assert!(text.starts_with("# The CRM."));
        assert!(text.contains("# the VPS"));
    }

    #[test]
    fn unset_removes_one_secret() {
        let text = set_secret(FILE, "A", "harbor:v1:a").unwrap();
        let text = set_secret(&text, "B", "harbor:v1:b").unwrap();
        let text = remove_secret(&text, "A").unwrap();
        assert!(!text.contains("A = "), "{text}");
        assert!(text.contains("B = \"harbor:v1:b\""));
        assert!(matches!(
            remove_secret(&text, "A"),
            Err(HarborError::UnknownSecret(name)) if name == "A"
        ));
        assert!(remove_secret(FILE, "A").is_err());
    }

    #[test]
    fn replace_keeps_order_and_comments() {
        let text = "[secrets]\nB = \"harbor:v1:b\" # bee\nA = \"harbor:v1:a\"\n";
        let payloads = BTreeMap::from([
            ("A".to_owned(), "harbor:v1:A2".to_owned()),
            ("B".to_owned(), "harbor:v1:B2".to_owned()),
        ]);
        let text = replace_secrets(text, &payloads).unwrap();
        assert_eq!(
            text,
            "[secrets]\nB = \"harbor:v1:B2\" # bee\nA = \"harbor:v1:A2\"\n"
        );
    }

    #[test]
    fn host_keys_are_pinned_in_the_server_table() {
        let text = set_host_key(FILE, "app-1", "ssh-ed25519 AAAAC3Nz").unwrap();
        assert!(
            text.contains("roles = [\"web\"]\nhost_key = \"ssh-ed25519 AAAAC3Nz\"\n\n[env]"),
            "{text}"
        );
        assert!(matches!(
            set_host_key(FILE, "app-9", "x"),
            Err(HarborError::UnknownServer(_))
        ));
    }
}
