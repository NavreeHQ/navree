//! [`Scope`]: who settings are resolved for and written at; [`ScopeKind`],
//! the levels of the inheritance chain; [`Source`], where a value came
//! from.

use std::fmt;
use std::str::FromStr;

use navree_contracts::Principal;
use navree_openapi::ToSchema;
use navree_workspace::{CurrentOrganization, CurrentWorkspace, Organization, Workspace};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A level of the inheritance chain, from the most general to the most
/// specific: global, organization, workspace, user. The `scope_type` column
/// of the `settings` table.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    /// The whole app.
    Global,
    /// One organization (and, below it, its workspaces).
    Organization,
    /// One workspace.
    Workspace,
    /// One user (preferences).
    User,
}

impl ScopeKind {
    /// Every level, from the most general to the most specific.
    pub const ALL: [ScopeKind; 4] = [
        ScopeKind::Global,
        ScopeKind::Organization,
        ScopeKind::Workspace,
        ScopeKind::User,
    ];

    /// `global`, `organization`, `workspace` or `user`.
    pub fn as_str(self) -> &'static str {
        match self {
            ScopeKind::Global => "global",
            ScopeKind::Organization => "organization",
            ScopeKind::Workspace => "workspace",
            ScopeKind::User => "user",
        }
    }

    /// The [`Source`] a value stored at this level reports.
    pub fn source(self) -> Source {
        match self {
            ScopeKind::Global => Source::Global,
            ScopeKind::Organization => Source::Organization,
            ScopeKind::Workspace => Source::Workspace,
            ScopeKind::User => Source::User,
        }
    }
}

impl fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ScopeKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "global" => Ok(ScopeKind::Global),
            "organization" => Ok(ScopeKind::Organization),
            "workspace" => Ok(ScopeKind::Workspace),
            "user" => Ok(ScopeKind::User),
            other => Err(format!(
                "unknown scope `{other}`: expected `global`, `organization`, `workspace` or `user`"
            )),
        }
    }
}

/// Where a resolved value came from: the type's `Default`, or the level of
/// the chain that set it last.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Nothing is stored: the settings type's `Default`.
    Default,
    /// The global value.
    Global,
    /// The organization's value.
    Organization,
    /// The workspace's value.
    Workspace,
    /// The user's value.
    User,
}

impl Source {
    /// `default`, `global`, `organization`, `workspace` or `user`.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Default => "default",
            Source::Global => "global",
            Source::Organization => "organization",
            Source::Workspace => "workspace",
            Source::User => "user",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One level of one chain: a kind and, except for the global level, an
/// id. What a stored row is keyed by (with the settings key).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
pub struct ScopeTarget {
    /// The level.
    pub kind: ScopeKind,
    /// The organization's, workspace's or user's id; `None` for the global
    /// level.
    pub id: Option<Uuid>,
}

impl ScopeTarget {
    /// The global level.
    pub const GLOBAL: ScopeTarget = ScopeTarget {
        kind: ScopeKind::Global,
        id: None,
    };

    /// The level `kind` of `id`.
    pub fn new(kind: ScopeKind, id: Uuid) -> ScopeTarget {
        ScopeTarget { kind, id: Some(id) }
    }

    /// The id stored in the `scope_id` column: the nil UUID for the global
    /// level.
    pub fn stored_id(&self) -> Uuid {
        self.id.unwrap_or(Uuid::nil())
    }

    /// The target of a stored `(scope_type, scope_id)` pair.
    pub fn from_stored(kind: ScopeKind, id: Uuid) -> ScopeTarget {
        match kind {
            ScopeKind::Global => ScopeTarget::GLOBAL,
            kind => ScopeTarget::new(kind, id),
        }
    }
}

impl fmt::Display for ScopeTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.id {
            None => f.write_str(self.kind.as_str()),
            Some(id) => write!(f, "{} {id}", self.kind),
        }
    }
}

/// Who settings are resolved for, and where a write goes.
///
/// A scope names up to one user, one workspace and one organization. Its
/// **target**, where writes go, is the most specific of them (user, then
/// workspace, then organization, else global). Resolution reads the chain
/// from the target up: the user's values, the workspace's, its
/// organization's, the global ones, then the type's `Default`.
///
/// ```
/// use navree_settings::{Scope, ScopeKind};
/// use uuid::Uuid;
///
/// let (ada, agra, holding) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
///
/// assert_eq!(Scope::global().target().kind, ScopeKind::Global);
/// assert_eq!(Scope::organization(holding).target().kind, ScopeKind::Organization);
///
/// // The workspace's organization is looked up when the scope is resolved,
/// // unless you name it.
/// let workspace = Scope::workspace(agra).in_organization(holding);
/// assert_eq!(workspace.organization_id(), Some(holding));
///
/// // A user's preferences, on top of a workspace's values.
/// let user = Scope::user(ada).in_workspace(agra);
/// assert_eq!(user.target().kind, ScopeKind::User);
/// assert_eq!(user.workspace_id(), Some(agra));
/// ```
///
/// Methods taking `impl Into<Scope>` also accept a `&Workspace`, a
/// `&CurrentWorkspace` (the workspace with its organization), an
/// `&Organization`, a `&CurrentOrganization` and a `&Principal` (its user).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Scope {
    user: Option<Uuid>,
    workspace: Option<Uuid>,
    organization: Option<Uuid>,
    /// Whether `organization` is known for the workspace (named, possibly
    /// none), so resolution need not look it up.
    organization_known: bool,
}

impl Scope {
    /// The whole app.
    pub fn global() -> Scope {
        Scope::default()
    }

    /// Organization `id`.
    pub fn organization(id: Uuid) -> Scope {
        Scope {
            organization: Some(id),
            organization_known: true,
            ..Scope::default()
        }
    }

    /// Workspace `id`. Its organization is looked up when the scope is
    /// resolved (with `WorkspaceModule`), unless you name it with
    /// [`in_organization`](Scope::in_organization).
    pub fn workspace(id: Uuid) -> Scope {
        Scope {
            workspace: Some(id),
            ..Scope::default()
        }
    }

    /// User `id`: their preferences on top of the global values. Add a
    /// workspace with [`in_workspace`](Scope::in_workspace).
    pub fn user(id: Uuid) -> Scope {
        Scope {
            user: Some(id),
            ..Scope::default()
        }
    }

    /// This scope inside workspace `id` (for a user: the workspace's values
    /// under the user's).
    pub fn in_workspace(self, id: Uuid) -> Scope {
        Scope {
            workspace: Some(id),
            organization: if self.workspace == Some(id) {
                self.organization
            } else {
                None
            },
            organization_known: self.workspace == Some(id) && self.organization_known,
            ..self
        }
    }

    /// This scope inside organization `id`, or (with `None`) inside no
    /// organization: what a workspace's organization is, without a lookup.
    pub fn in_organization(self, id: impl Into<Option<Uuid>>) -> Scope {
        Scope {
            organization: id.into(),
            organization_known: true,
            ..self
        }
    }

    /// The user, if any.
    pub fn user_id(&self) -> Option<Uuid> {
        self.user
    }

    /// The workspace, if any.
    pub fn workspace_id(&self) -> Option<Uuid> {
        self.workspace
    }

    /// The organization, if named (or looked up).
    pub fn organization_id(&self) -> Option<Uuid> {
        self.organization
    }

    /// Whether the organization is known: named, or not needed (no
    /// workspace).
    pub fn organization_known(&self) -> bool {
        self.organization_known || self.workspace.is_none()
    }

    /// Where writes go: the most specific level the scope names.
    pub fn target(&self) -> ScopeTarget {
        if let Some(user) = self.user {
            ScopeTarget::new(ScopeKind::User, user)
        } else if let Some(workspace) = self.workspace {
            ScopeTarget::new(ScopeKind::Workspace, workspace)
        } else if let Some(organization) = self.organization {
            ScopeTarget::new(ScopeKind::Organization, organization)
        } else {
            ScopeTarget::GLOBAL
        }
    }

    /// Every level the scope names, from the most specific (the target) to
    /// the global level.
    pub fn chain(&self) -> Vec<ScopeTarget> {
        let mut chain = Vec::with_capacity(4);
        if let Some(user) = self.user {
            chain.push(ScopeTarget::new(ScopeKind::User, user));
        }
        if let Some(workspace) = self.workspace {
            chain.push(ScopeTarget::new(ScopeKind::Workspace, workspace));
        }
        if let Some(organization) = self.organization {
            chain.push(ScopeTarget::new(ScopeKind::Organization, organization));
        }
        chain.push(ScopeTarget::GLOBAL);
        chain
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let chain = self.chain();
        let names: Vec<String> = chain.iter().map(ToString::to_string).collect();
        f.write_str(&names.join(" < "))
    }
}

impl From<&Scope> for Scope {
    fn from(scope: &Scope) -> Scope {
        *scope
    }
}

/// A workspace: its scope, with its organization.
impl From<&Workspace> for Scope {
    fn from(workspace: &Workspace) -> Scope {
        Scope::workspace(workspace.id).in_organization(workspace.organization_id)
    }
}

/// The request's workspace: its scope, with its organization.
impl From<&CurrentWorkspace> for Scope {
    fn from(current: &CurrentWorkspace) -> Scope {
        Scope::from(current.workspace())
    }
}

/// An organization: its scope.
impl From<&Organization> for Scope {
    fn from(organization: &Organization) -> Scope {
        Scope::organization(organization.id)
    }
}

/// The request's organization: its scope.
impl From<&CurrentOrganization> for Scope {
    fn from(current: &CurrentOrganization) -> Scope {
        Scope::organization(current.id())
    }
}

/// A signed-in principal: its user.
impl From<&Principal> for Scope {
    fn from(principal: &Principal) -> Scope {
        Scope::user(principal.user_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_the_most_specific_level() {
        let (user, workspace, organization) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        assert_eq!(Scope::global().target(), ScopeTarget::GLOBAL);
        assert_eq!(
            Scope::organization(organization).target(),
            ScopeTarget::new(ScopeKind::Organization, organization)
        );
        assert_eq!(
            Scope::workspace(workspace)
                .in_organization(organization)
                .target(),
            ScopeTarget::new(ScopeKind::Workspace, workspace)
        );
        assert_eq!(
            Scope::user(user).in_workspace(workspace).target(),
            ScopeTarget::new(ScopeKind::User, user)
        );
    }

    #[test]
    fn chains_run_from_the_target_to_the_global_level() {
        let (user, workspace, organization) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let scope = Scope::user(user)
            .in_workspace(workspace)
            .in_organization(organization);
        assert_eq!(
            scope.chain(),
            [
                ScopeTarget::new(ScopeKind::User, user),
                ScopeTarget::new(ScopeKind::Workspace, workspace),
                ScopeTarget::new(ScopeKind::Organization, organization),
                ScopeTarget::GLOBAL,
            ]
        );
        assert_eq!(Scope::global().chain(), [ScopeTarget::GLOBAL]);
        assert_eq!(
            Scope::user(user).chain(),
            [ScopeTarget::new(ScopeKind::User, user), ScopeTarget::GLOBAL]
        );
    }

    #[test]
    fn the_organization_of_a_workspace_is_known_only_when_named() {
        let (workspace, organization) = (Uuid::now_v7(), Uuid::now_v7());
        assert!(!Scope::workspace(workspace).organization_known());
        assert!(
            Scope::workspace(workspace)
                .in_organization(None)
                .organization_known()
        );
        let named = Scope::workspace(workspace).in_organization(organization);
        assert!(named.organization_known());
        assert_eq!(named.organization_id(), Some(organization));
        // Moving to another workspace forgets the organization.
        let other = named.in_workspace(Uuid::now_v7());
        assert!(!other.organization_known());
        assert_eq!(other.organization_id(), None);
        // The same workspace keeps it.
        assert_eq!(named.in_workspace(workspace), named);
        assert!(Scope::global().organization_known());
        assert!(Scope::user(Uuid::now_v7()).organization_known());
    }

    #[test]
    fn kinds_and_sources_print_and_parse() {
        for kind in ScopeKind::ALL {
            assert_eq!(kind.as_str().parse::<ScopeKind>(), Ok(kind));
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
            assert_eq!(kind.source().as_str(), kind.as_str());
        }
        assert!("team".parse::<ScopeKind>().is_err());
        assert_eq!(serde_json::to_value(Source::Default).unwrap(), "default");
        assert!(ScopeKind::Global < ScopeKind::User);
    }

    #[test]
    fn targets_store_the_nil_id_for_the_global_level() {
        assert_eq!(ScopeTarget::GLOBAL.stored_id(), Uuid::nil());
        assert_eq!(
            ScopeTarget::from_stored(ScopeKind::Global, Uuid::nil()),
            ScopeTarget::GLOBAL
        );
        let id = Uuid::now_v7();
        assert_eq!(
            ScopeTarget::from_stored(ScopeKind::User, id),
            ScopeTarget::new(ScopeKind::User, id)
        );
        assert_eq!(
            ScopeTarget::new(ScopeKind::Workspace, id).to_string(),
            format!("workspace {id}")
        );
        assert_eq!(Scope::user(id).to_string(), format!("user {id} < global"));
    }

    #[test]
    fn principals_convert_to_their_user() {
        let user = Uuid::now_v7();
        let principal = Principal {
            user_id: user,
            via: navree_contracts::AuthVia::Test,
            token_abilities: None,
            session_id: None,
        };
        assert_eq!(Scope::from(&principal), Scope::user(user));
    }
}
