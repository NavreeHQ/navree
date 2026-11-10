//! [`WorkspaceRole`] and the permissions each role grants inside a
//! workspace.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::WorkspaceConfig;

/// A member's role in a workspace: one of the four built-in roles or a
/// role the app defines under `workspace.roles.<name>`.
///
/// | Role | Built-in permissions |
/// |---|---|
/// | `owner` | `all.*` (everything in the workspace) |
/// | `admin` | `workspace.read`, `workspace.update`, `workspace.members.*`, `workspace.invitations.*` |
/// | `member` | `workspace.read`, `workspace.members.read` |
/// | `viewer` | `workspace.read`, `workspace.members.read` |
///
/// The permissions configured for a role are added to its built-in ones;
/// `member` and `viewer` differ only in what the app grants them.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum WorkspaceRole {
    /// Everything, including deleting the workspace and transferring it.
    Owner,
    /// Manages the workspace, its members and its invitations.
    Admin,
    /// A regular member.
    Member,
    /// Read-only access, as the app defines it.
    Viewer,
    /// An app-defined role.
    Custom(String),
}

/// `owner`.
pub const OWNER: &str = "owner";
/// `admin`.
pub const ADMIN: &str = "admin";
/// `member`.
pub const MEMBER: &str = "member";
/// `viewer`.
pub const VIEWER: &str = "viewer";

impl WorkspaceRole {
    /// The role named `name` (lower case); app-defined names become
    /// [`Custom`](WorkspaceRole::Custom).
    pub fn parse(name: &str) -> WorkspaceRole {
        match name {
            OWNER => WorkspaceRole::Owner,
            ADMIN => WorkspaceRole::Admin,
            MEMBER => WorkspaceRole::Member,
            VIEWER => WorkspaceRole::Viewer,
            other => WorkspaceRole::Custom(other.to_owned()),
        }
    }

    /// The stored name: `owner`, `admin`, `member`, `viewer` or the custom
    /// name.
    pub fn as_str(&self) -> &str {
        match self {
            WorkspaceRole::Owner => OWNER,
            WorkspaceRole::Admin => ADMIN,
            WorkspaceRole::Member => MEMBER,
            WorkspaceRole::Viewer => VIEWER,
            WorkspaceRole::Custom(name) => name,
        }
    }

    /// Whether this is one of the four built-in roles.
    pub fn is_built_in(&self) -> bool {
        !matches!(self, WorkspaceRole::Custom(_))
    }

    /// The permissions the role grants before configuration adds to them.
    pub fn built_in_permissions(&self) -> &'static [&'static str] {
        match self {
            WorkspaceRole::Owner => &["all.*"],
            WorkspaceRole::Admin => &[
                "workspace.read",
                "workspace.update",
                "workspace.members.*",
                "workspace.invitations.*",
            ],
            WorkspaceRole::Member | WorkspaceRole::Viewer => {
                &["workspace.read", "workspace.members.read"]
            }
            WorkspaceRole::Custom(_) => &[],
        }
    }
}

impl fmt::Display for WorkspaceRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for WorkspaceRole {
    fn from(name: &str) -> Self {
        WorkspaceRole::parse(name)
    }
}

impl Serialize for WorkspaceRole {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for WorkspaceRole {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(WorkspaceRole::parse(&name))
    }
}

/// Every role the app knows (the built-in four plus `workspace.roles`) with
/// its permissions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleTable {
    roles: BTreeMap<String, Vec<String>>,
}

impl RoleTable {
    /// The roles of `config`.
    pub fn from_config(config: &WorkspaceConfig) -> RoleTable {
        let mut roles: BTreeMap<String, BTreeSet<String>> = [
            WorkspaceRole::Owner,
            WorkspaceRole::Admin,
            WorkspaceRole::Member,
            WorkspaceRole::Viewer,
        ]
        .iter()
        .map(|role| {
            let permissions = role
                .built_in_permissions()
                .iter()
                .map(|p| (*p).to_owned())
                .collect();
            (role.as_str().to_owned(), permissions)
        })
        .collect();
        if config.partitions.enabled {
            // Partitions are workspace data: admins manage them, everyone
            // reads them.
            for (role, permission) in [
                (ADMIN, "workspace.partitions.*"),
                (MEMBER, "workspace.partitions.read"),
                (VIEWER, "workspace.partitions.read"),
            ] {
                if let Some(permissions) = roles.get_mut(role) {
                    permissions.insert(permission.to_owned());
                }
            }
        }
        for (name, options) in &config.roles {
            roles
                .entry(name.clone())
                .or_default()
                .extend(options.permissions.iter().cloned());
        }
        RoleTable {
            roles: roles
                .into_iter()
                .map(|(name, permissions)| (name, permissions.into_iter().collect()))
                .collect(),
        }
    }

    /// Whether `role` is a built-in or configured role.
    pub fn contains(&self, role: &WorkspaceRole) -> bool {
        self.roles.contains_key(role.as_str())
    }

    /// The permissions of `role`, sorted (empty for unknown roles).
    pub fn permissions(&self, role: &WorkspaceRole) -> &[String] {
        self.roles.get(role.as_str()).map_or(&[], Vec::as_slice)
    }

    /// Every role name, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.roles.keys().map(String::as_str)
    }
}

/// Checks a role name: 1 to 64 lower-case ASCII letters, digits, `_`, `-`
/// or `.`, starting with a letter or digit.
pub(crate) fn check_role_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.len() > 64 {
        return Err("must be 1 to 64 characters long");
    }
    let allowed =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.');
    if !name.chars().all(allowed)
        || !name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return Err(
            "may only contain lower-case ASCII letters, digits, `_`, `-` and `.` and must start with a letter or digit",
        );
    }
    Ok(())
}

/// Checks a permission string: `<prefix>.<action>`, up to 128 ASCII
/// letters, digits, `_`, `-`, `.`, `:` or `*`.
pub(crate) fn check_permission(permission: &str) -> Result<(), &'static str> {
    if permission.is_empty() || permission.len() > 128 {
        return Err("must be 1 to 128 characters long");
    }
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '*');
    if !permission.chars().all(allowed) {
        return Err("may only contain ASCII letters, digits, `_`, `-`, `.`, `:` and `*`");
    }
    match permission.rsplit_once('.') {
        Some((prefix, action)) if !prefix.is_empty() && !action.is_empty() => Ok(()),
        _ => Err("must look like `<prefix>.<action>`, e.g. `deals.update`"),
    }
}

#[cfg(test)]
mod tests {
    use crate::config::RoleOptions;

    use super::*;

    #[test]
    fn roles_parse_and_print() {
        assert_eq!(WorkspaceRole::parse("owner"), WorkspaceRole::Owner);
        assert_eq!(WorkspaceRole::parse("viewer"), WorkspaceRole::Viewer);
        assert_eq!(
            WorkspaceRole::parse("billing"),
            WorkspaceRole::Custom("billing".into())
        );
        assert_eq!(WorkspaceRole::Admin.to_string(), "admin");
        assert!(WorkspaceRole::Member.is_built_in());
        assert!(!WorkspaceRole::Custom("x".into()).is_built_in());
        assert_eq!(
            serde_json::to_value(WorkspaceRole::Owner).unwrap(),
            serde_json::json!("owner")
        );
        let role: WorkspaceRole = serde_json::from_value(serde_json::json!("billing")).unwrap();
        assert_eq!(role.as_str(), "billing");
    }

    #[test]
    fn the_table_adds_configured_permissions_to_the_built_in_ones() {
        let mut config = WorkspaceConfig::default();
        config.roles.insert(
            "member".into(),
            RoleOptions {
                permissions: vec!["deals.*".into(), "workspace.read".into()],
            },
        );
        config.roles.insert(
            "billing".into(),
            RoleOptions {
                permissions: vec!["invoices.*".into()],
            },
        );
        let table = RoleTable::from_config(&config);
        assert_eq!(
            table.permissions(&WorkspaceRole::Member),
            ["deals.*", "workspace.members.read", "workspace.read"]
        );
        assert_eq!(table.permissions(&WorkspaceRole::Owner), ["all.*"]);
        assert_eq!(
            table.permissions(&WorkspaceRole::Custom("billing".into())),
            ["invoices.*"]
        );
        assert!(table.contains(&WorkspaceRole::Custom("billing".into())));
        assert!(!table.contains(&WorkspaceRole::Custom("auditor".into())));
        assert!(
            table
                .permissions(&WorkspaceRole::Custom("auditor".into()))
                .is_empty()
        );
        assert_eq!(
            table.names().collect::<Vec<_>>(),
            ["admin", "billing", "member", "owner", "viewer"]
        );
    }

    #[test]
    fn partitions_add_their_permissions_when_enabled() {
        let mut config = WorkspaceConfig::default();
        let off = RoleTable::from_config(&config);
        assert!(
            !off.permissions(&WorkspaceRole::Admin)
                .iter()
                .any(|p| p.starts_with("workspace.partitions"))
        );
        config.partitions.enabled = true;
        let on = RoleTable::from_config(&config);
        assert!(
            on.permissions(&WorkspaceRole::Admin)
                .contains(&"workspace.partitions.*".to_owned())
        );
        assert!(
            on.permissions(&WorkspaceRole::Viewer)
                .contains(&"workspace.partitions.read".to_owned())
        );
        assert_eq!(on.permissions(&WorkspaceRole::Owner), ["all.*"]);
    }

    #[test]
    fn names_and_permissions_are_checked() {
        assert!(check_role_name("billing-admin").is_ok());
        assert!(check_role_name("Billing").is_err());
        assert!(check_role_name("").is_err());
        assert!(check_role_name("-x").is_err());
        assert!(check_permission("deals.*").is_ok());
        assert!(check_permission("workspace.members.update").is_ok());
        assert!(check_permission("deals").is_err());
        assert!(check_permission("deals publish").is_err());
    }
}
