//! [`OrgRole`], the permissions each organization role grants, the
//! workspace role it maps to, and [`Selector`]: all workspaces (or
//! partitions), or only some.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use uuid::Uuid;

use crate::WorkspaceRole;
use crate::config::WorkspaceConfig;
use crate::role::{ADMIN, MEMBER, OWNER, VIEWER};

/// A member's role in an organization: one of the four built-in roles or a
/// role the app defines under `workspace.organizations.roles.<name>`.
///
/// | Role | Built-in permissions (in the organization) | Workspace role it gives |
/// |---|---|---|
/// | `owner` | `all.*` | `owner` |
/// | `admin` | `organization.read`, `organization.update`, `organization.attach`, `organization.detach`, `organization.members.*`, `organization.access_requests.*` | `admin` |
/// | `member` | `organization.read`, `organization.members.read` | `member` |
/// | `viewer` | `organization.read`, `organization.members.read` | `viewer` |
///
/// An app-defined role gives the workspace role named by its
/// `workspace_role` setting, or the workspace role of the same name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OrgRole {
    /// Everything in the organization, including deleting and transferring
    /// it; always reaches every workspace.
    Owner,
    /// Runs the organization: its members, workspaces and access requests.
    Admin,
    /// A regular member.
    Member,
    /// Read-only access, as the app defines it.
    Viewer,
    /// An app-defined role.
    Custom(String),
}

impl OrgRole {
    /// The role named `name`; other names become [`Custom`](OrgRole::Custom).
    pub fn parse(name: &str) -> OrgRole {
        match name {
            OWNER => OrgRole::Owner,
            ADMIN => OrgRole::Admin,
            MEMBER => OrgRole::Member,
            VIEWER => OrgRole::Viewer,
            other => OrgRole::Custom(other.to_owned()),
        }
    }

    /// The stored name: `owner`, `admin`, `member`, `viewer` or the custom
    /// name.
    pub fn as_str(&self) -> &str {
        match self {
            OrgRole::Owner => OWNER,
            OrgRole::Admin => ADMIN,
            OrgRole::Member => MEMBER,
            OrgRole::Viewer => VIEWER,
            OrgRole::Custom(name) => name,
        }
    }

    /// Whether this is one of the four built-in roles.
    pub fn is_built_in(&self) -> bool {
        !matches!(self, OrgRole::Custom(_))
    }

    /// The permissions the role grants in the organization before
    /// configuration adds to them.
    pub fn built_in_permissions(&self) -> &'static [&'static str] {
        match self {
            OrgRole::Owner => &["all.*"],
            OrgRole::Admin => &[
                "organization.read",
                "organization.update",
                "organization.attach",
                "organization.detach",
                "organization.members.*",
                "organization.access_requests.*",
            ],
            OrgRole::Member | OrgRole::Viewer => {
                &["organization.read", "organization.members.read"]
            }
            OrgRole::Custom(_) => &[],
        }
    }
}

impl fmt::Display for OrgRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for OrgRole {
    fn from(name: &str) -> Self {
        OrgRole::parse(name)
    }
}

impl Serialize for OrgRole {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for OrgRole {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(OrgRole::parse(&name))
    }
}

/// Every organization role the app knows (the built-in four plus
/// `workspace.organizations.roles`), with its permissions and the workspace
/// role it gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrgRoleTable {
    roles: BTreeMap<String, (Vec<String>, WorkspaceRole)>,
}

impl OrgRoleTable {
    /// The roles of `config`.
    pub fn from_config(config: &WorkspaceConfig) -> OrgRoleTable {
        let mut roles: BTreeMap<String, (BTreeSet<String>, WorkspaceRole)> = [
            OrgRole::Owner,
            OrgRole::Admin,
            OrgRole::Member,
            OrgRole::Viewer,
        ]
        .iter()
        .map(|role| {
            let permissions = role
                .built_in_permissions()
                .iter()
                .map(|p| (*p).to_owned())
                .collect();
            (
                role.as_str().to_owned(),
                (permissions, WorkspaceRole::parse(role.as_str())),
            )
        })
        .collect();
        for (name, options) in &config.organizations.roles {
            let mapped = WorkspaceRole::parse(options.workspace_role.as_deref().unwrap_or(name));
            let entry = roles
                .entry(name.clone())
                .or_insert_with(|| (BTreeSet::new(), mapped.clone()));
            entry.0.extend(options.permissions.iter().cloned());
            if options.workspace_role.is_some() {
                entry.1 = mapped;
            }
        }
        OrgRoleTable {
            roles: roles
                .into_iter()
                .map(|(name, (permissions, role))| {
                    (name, (permissions.into_iter().collect(), role))
                })
                .collect(),
        }
    }

    /// Whether `role` is a built-in or configured organization role.
    pub fn contains(&self, role: &OrgRole) -> bool {
        self.roles.contains_key(role.as_str())
    }

    /// The permissions of `role` in the organization, sorted (empty for
    /// unknown roles).
    pub fn permissions(&self, role: &OrgRole) -> &[String] {
        self.roles
            .get(role.as_str())
            .map_or(&[], |(permissions, _)| permissions.as_slice())
    }

    /// The workspace role `role` gives in the workspaces it reaches (`None`
    /// for unknown roles).
    pub fn workspace_role(&self, role: &OrgRole) -> Option<&WorkspaceRole> {
        self.roles.get(role.as_str()).map(|(_, mapped)| mapped)
    }

    /// Every role name, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.roles.keys().map(String::as_str)
    }
}

/// Which workspaces an organization member reaches, or which partitions a
/// workspace member sees: all of them, or only the listed ones.
///
/// In JSON it is `"all"` or an array of ids:
///
/// ```
/// use navree_workspace::WorkspaceSelector;
///
/// let all: WorkspaceSelector = serde_json::from_str(r#""all""#).unwrap();
/// assert_eq!(all, WorkspaceSelector::All);
/// let id = uuid::Uuid::now_v7();
/// let only = WorkspaceSelector::only([id]);
/// assert!(only.includes(id));
/// assert_eq!(serde_json::to_value(&only).unwrap(), serde_json::json!([id]));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Selector {
    /// Every one, now and later.
    #[default]
    All,
    /// Only these (no repeats, in the order given).
    Only(Vec<Uuid>),
}

/// The workspaces of an organization a member reaches.
pub type WorkspaceSelector = Selector;
/// The partitions of a workspace a member sees.
pub type PartitionSelector = Selector;

impl Selector {
    /// [`Only`](Selector::Only) these ids, without repeats.
    pub fn only(ids: impl IntoIterator<Item = Uuid>) -> Selector {
        let mut seen = BTreeSet::new();
        Selector::Only(ids.into_iter().filter(|id| seen.insert(*id)).collect())
    }

    /// Whether this is [`All`](Selector::All).
    pub fn is_all(&self) -> bool {
        matches!(self, Selector::All)
    }

    /// Whether `id` is selected.
    pub fn includes(&self, id: Uuid) -> bool {
        match self {
            Selector::All => true,
            Selector::Only(ids) => ids.contains(&id),
        }
    }

    /// The listed ids (`None` for [`All`](Selector::All)).
    pub fn ids(&self) -> Option<&[Uuid]> {
        match self {
            Selector::All => None,
            Selector::Only(ids) => Some(ids),
        }
    }

    /// `all` or `only`, as stored.
    pub(crate) fn scope_name(&self) -> &'static str {
        match self {
            Selector::All => SCOPE_ALL,
            Selector::Only(_) => SCOPE_ONLY,
        }
    }
}

/// The stored scope of a member who reaches every workspace or partition.
pub(crate) const SCOPE_ALL: &str = "all";
/// The stored scope of a member limited to their grants.
pub(crate) const SCOPE_ONLY: &str = "only";

impl Serialize for Selector {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Selector::All => serializer.serialize_str(SCOPE_ALL),
            Selector::Only(ids) => ids.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Selector {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SelectorVisitor;

        impl<'de> Visitor<'de> for SelectorVisitor {
            type Value = Selector;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#""all" or a list of ids"#)
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Selector, E> {
                if value == SCOPE_ALL {
                    Ok(Selector::All)
                } else {
                    Err(E::invalid_value(de::Unexpected::Str(value), &self))
                }
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Selector, A::Error> {
                let mut ids = Vec::new();
                while let Some(id) = seq.next_element::<Uuid>()? {
                    ids.push(id);
                }
                Ok(Selector::only(ids))
            }
        }

        deserializer.deserialize_any(SelectorVisitor)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::config::{OrgRoleOptions, RoleOptions};

    #[test]
    fn org_roles_parse_and_print() {
        assert_eq!(OrgRole::parse("owner"), OrgRole::Owner);
        assert_eq!(OrgRole::parse("admin"), OrgRole::Admin);
        assert_eq!(OrgRole::parse("partner"), OrgRole::Custom("partner".into()));
        assert_eq!(OrgRole::Viewer.to_string(), "viewer");
        assert!(OrgRole::Member.is_built_in());
        assert!(!OrgRole::Custom("x".into()).is_built_in());
        assert_eq!(
            serde_json::to_value(OrgRole::Admin).unwrap(),
            json!("admin")
        );
        let role: OrgRole = serde_json::from_value(json!("partner")).unwrap();
        assert_eq!(role.as_str(), "partner");
    }

    #[test]
    fn the_table_maps_org_roles_to_workspace_roles() {
        let mut config = WorkspaceConfig::default();
        config.roles.insert(
            "auditor".into(),
            RoleOptions {
                permissions: vec!["reports.read".into()],
            },
        );
        config.organizations.roles.insert(
            "partner".into(),
            OrgRoleOptions {
                permissions: vec!["organization.read".into()],
                workspace_role: Some("auditor".into()),
            },
        );
        config.organizations.roles.insert(
            "member".into(),
            OrgRoleOptions {
                permissions: vec!["reports.read".into()],
                workspace_role: Some("viewer".into()),
            },
        );
        let table = OrgRoleTable::from_config(&config);
        assert_eq!(table.permissions(&OrgRole::Owner), ["all.*"]);
        assert_eq!(
            table.permissions(&OrgRole::Member),
            [
                "organization.members.read",
                "organization.read",
                "reports.read"
            ]
        );
        assert_eq!(
            table.workspace_role(&OrgRole::Owner),
            Some(&WorkspaceRole::Owner)
        );
        assert_eq!(
            table.workspace_role(&OrgRole::Admin),
            Some(&WorkspaceRole::Admin)
        );
        assert_eq!(
            table.workspace_role(&OrgRole::Member),
            Some(&WorkspaceRole::Viewer)
        );
        let partner = OrgRole::Custom("partner".into());
        assert!(table.contains(&partner));
        assert_eq!(table.permissions(&partner), ["organization.read"]);
        assert_eq!(
            table.workspace_role(&partner),
            Some(&WorkspaceRole::Custom("auditor".into()))
        );
        let unknown = OrgRole::Custom("ghost".into());
        assert!(!table.contains(&unknown));
        assert!(table.permissions(&unknown).is_empty());
        assert_eq!(table.workspace_role(&unknown), None);
        assert_eq!(
            table.names().collect::<Vec<_>>(),
            ["admin", "member", "owner", "partner", "viewer"]
        );
    }

    #[test]
    fn selectors_are_all_or_a_list() {
        let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
        assert!(Selector::All.includes(a));
        assert!(Selector::All.is_all());
        assert_eq!(Selector::All.ids(), None);
        let only = Selector::only([a, a, b]);
        assert_eq!(only, Selector::Only(vec![a, b]));
        assert!(only.includes(b));
        assert!(!only.includes(Uuid::now_v7()));
        assert_eq!(only.ids(), Some(&[a, b][..]));
        assert_eq!(serde_json::to_value(Selector::All).unwrap(), json!("all"));
        assert_eq!(serde_json::to_value(&only).unwrap(), json!([a, b]));
        let parsed: Selector = serde_json::from_value(json!([b, b])).unwrap();
        assert_eq!(parsed, Selector::Only(vec![b]));
        let all: Selector = serde_json::from_value(json!("all")).unwrap();
        assert_eq!(all, Selector::All);
        assert!(serde_json::from_value::<Selector>(json!("some")).is_err());
        assert!(serde_json::from_value::<Selector>(json!(["nope"])).is_err());
        assert_eq!(Selector::default(), Selector::All);
    }
}
