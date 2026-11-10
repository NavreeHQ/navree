//! [`FlagScope`]: who a flag is resolved for.

use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use navree_contracts::Principal;
use navree_openapi::ToSchema;
use navree_workspace::{CurrentWorkspace, Workspace};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Who a flag is resolved for: a user, a workspace, both, or neither
/// ([`everyone`](FlagScope::everyone)).
///
/// ```
/// use navree_flags::FlagScope;
/// use uuid::Uuid;
///
/// let (ada, agra) = (Uuid::now_v7(), Uuid::now_v7());
/// let scope: FlagScope = FlagScope::user(ada).workspace(agra);
/// assert_eq!(scope.user_id(), Some(ada));
/// assert_eq!(scope.workspace_id(), Some(agra));
/// assert!(FlagScope::everyone().is_everyone());
/// ```
///
/// Checks and writes take `impl Into<FlagScope>`, so a `&FlagScope`, the
/// [`UserScope`] of `FlagScope::user(id)`, a `&Workspace`, a
/// `&CurrentWorkspace` (its user and workspace), a `&Principal` and, with
/// the `auth` feature, a `&User` or `&CurrentUser` all work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct FlagScope {
    /// The user, if any.
    user_id: Option<Uuid>,
    /// The workspace, if any.
    workspace_id: Option<Uuid>,
}

impl FlagScope {
    /// No user and no workspace: as a write target, the global state
    /// ("everyone"); as a check, one that only the global state, the kill
    /// switch and the default can decide (see `flags.missing_scope`).
    pub fn everyone() -> FlagScope {
        FlagScope::default()
    }

    /// One user. Narrow it to a workspace with
    /// [`UserScope::workspace`]: `FlagScope::user(id).workspace(id)`.
    pub fn user(id: Uuid) -> UserScope {
        UserScope(FlagScope {
            user_id: Some(id),
            workspace_id: None,
        })
    }

    /// One workspace.
    pub fn workspace(id: Uuid) -> FlagScope {
        FlagScope {
            user_id: None,
            workspace_id: Some(id),
        }
    }

    /// This scope with user `id`.
    pub fn with_user(self, id: Uuid) -> FlagScope {
        FlagScope {
            user_id: Some(id),
            ..self
        }
    }

    /// This scope with workspace `id`.
    pub fn with_workspace(self, id: Uuid) -> FlagScope {
        FlagScope {
            workspace_id: Some(id),
            ..self
        }
    }

    /// The user, if any.
    pub fn user_id(&self) -> Option<Uuid> {
        self.user_id
    }

    /// The workspace, if any.
    pub fn workspace_id(&self) -> Option<Uuid> {
        self.workspace_id
    }

    /// Whether the scope names neither a user nor a workspace.
    pub fn is_everyone(&self) -> bool {
        self.user_id.is_none() && self.workspace_id.is_none()
    }

    /// The single target of an override write: the user or the workspace
    /// (`None` for [`everyone`](FlagScope::everyone)). A scope with both is
    /// not a write target.
    pub(crate) fn target(&self) -> Result<Option<(ScopeType, Uuid)>, ()> {
        match (self.user_id, self.workspace_id) {
            (None, None) => Ok(None),
            (Some(user), None) => Ok(Some((ScopeType::User, user))),
            (None, Some(workspace)) => Ok(Some((ScopeType::Workspace, workspace))),
            (Some(_), Some(_)) => Err(()),
        }
    }
}

impl fmt::Display for FlagScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.user_id, self.workspace_id) {
            (None, None) => f.write_str("everyone"),
            (Some(user), None) => write!(f, "user {user}"),
            (None, Some(workspace)) => write!(f, "workspace {workspace}"),
            (Some(user), Some(workspace)) => write!(f, "user {user} in workspace {workspace}"),
        }
    }
}

/// `FlagScope::user(id)`: one user, which converts into a [`FlagScope`]
/// (and derefs to one), or narrows to a workspace with
/// [`workspace`](UserScope::workspace).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UserScope(FlagScope);

impl UserScope {
    /// The user in workspace `id`.
    pub fn workspace(self, id: Uuid) -> FlagScope {
        self.0.with_workspace(id)
    }

    /// The scope.
    pub fn scope(self) -> FlagScope {
        self.0
    }
}

impl Deref for UserScope {
    type Target = FlagScope;

    fn deref(&self) -> &FlagScope {
        &self.0
    }
}

impl From<UserScope> for FlagScope {
    fn from(scope: UserScope) -> FlagScope {
        scope.0
    }
}

impl From<&UserScope> for FlagScope {
    fn from(scope: &UserScope) -> FlagScope {
        scope.0
    }
}

impl From<&FlagScope> for FlagScope {
    fn from(scope: &FlagScope) -> FlagScope {
        *scope
    }
}

/// A workspace: the workspace scope.
impl From<&Workspace> for FlagScope {
    fn from(workspace: &Workspace) -> FlagScope {
        FlagScope::workspace(workspace.id)
    }
}

/// The request's workspace: its signed-in member in that workspace.
impl From<&CurrentWorkspace> for FlagScope {
    fn from(current: &CurrentWorkspace) -> FlagScope {
        FlagScope::user(current.member().user_id).workspace(current.id())
    }
}

/// A signed-in principal: its user.
impl From<&Principal> for FlagScope {
    fn from(principal: &Principal) -> FlagScope {
        FlagScope::user(principal.user_id).scope()
    }
}

/// A user account (navree-auth): its user.
#[cfg(feature = "auth")]
impl From<&navree_auth::User> for FlagScope {
    fn from(user: &navree_auth::User) -> FlagScope {
        FlagScope::user(user.id).scope()
    }
}

/// The request's signed-in user (navree-auth): its user.
#[cfg(feature = "auth")]
impl From<&navree_auth::CurrentUser> for FlagScope {
    fn from(user: &navree_auth::CurrentUser) -> FlagScope {
        FlagScope::user(user.0.id).scope()
    }
}

/// The kind of an override: for one user or for one workspace (the
/// `scope_type` column of `feature_flag_overrides`).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ScopeType {
    /// A user.
    User,
    /// A workspace.
    Workspace,
}

impl ScopeType {
    /// `user` or `workspace`.
    pub fn as_str(self) -> &'static str {
        match self {
            ScopeType::User => "user",
            ScopeType::Workspace => "workspace",
        }
    }
}

impl fmt::Display for ScopeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ScopeType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(ScopeType::User),
            "workspace" => Ok(ScopeType::Workspace),
            other => Err(format!(
                "unknown scope type `{other}`: expected `user` or `workspace`"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_are_built_from_users_and_workspaces() {
        let (user, workspace) = (Uuid::now_v7(), Uuid::now_v7());
        assert!(FlagScope::everyone().is_everyone());
        assert_eq!(FlagScope::everyone().target(), Ok(None));

        let user_scope = FlagScope::user(user);
        assert_eq!(user_scope.user_id(), Some(user));
        assert_eq!(user_scope.workspace_id(), None);
        assert_eq!(
            FlagScope::from(user_scope).target(),
            Ok(Some((ScopeType::User, user)))
        );

        let workspace_scope = FlagScope::workspace(workspace);
        assert_eq!(
            workspace_scope.target(),
            Ok(Some((ScopeType::Workspace, workspace)))
        );

        let both = FlagScope::user(user).workspace(workspace);
        assert_eq!(
            (both.user_id(), both.workspace_id()),
            (Some(user), Some(workspace))
        );
        assert_eq!(both.target(), Err(()));
        assert_eq!(FlagScope::workspace(workspace).with_user(user), both);
        assert_eq!(FlagScope::from(&both), both);
        assert_eq!(
            both.to_string(),
            format!("user {user} in workspace {workspace}")
        );
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
        assert_eq!(FlagScope::from(&principal), FlagScope::user(user).scope());
    }

    #[test]
    fn scope_types_parse_and_print() {
        assert_eq!("user".parse::<ScopeType>(), Ok(ScopeType::User));
        assert_eq!("workspace".parse::<ScopeType>(), Ok(ScopeType::Workspace));
        assert!("team".parse::<ScopeType>().is_err());
        assert_eq!(ScopeType::Workspace.to_string(), "workspace");
        assert_eq!(serde_json::to_value(ScopeType::User).unwrap(), "user");
    }
}
