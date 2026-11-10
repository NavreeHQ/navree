//! [`Pulled`]: what a source gives a run, and [`MemberChange`].

use serde::{Deserialize, Serialize};

use crate::user::{DirectoryGroup, DirectoryUser};

/// What a [`DirectorySource`](crate::DirectorySource) pulled: everything
/// (a snapshot) or the changes since the last pull (a delta).
///
/// In a snapshot, users missing from it are deactivated and groups
/// missing from it are deleted, and each group's `members` is the
/// complete list. In a delta, only what it names changes: removed users
/// and groups are listed, and members change only through
/// `member_changes` (the `members` of a delta's groups are ignored).
///
/// ```
/// use navree_directory::{DirectoryGroup, DirectoryUser, Pulled};
///
/// let pulled = Pulled::snapshot(
///     vec![DirectoryUser::new("u1", "anu@acme.test")],
///     vec![DirectoryGroup::new("g1", "Engineering").members(["u1"])],
/// )
/// .cursor("opaque-cursor");
/// assert!(pulled.complete);
///
/// let delta = Pulled::delta(vec![], vec![])
///     .removed_user("u2")
///     .member_added("g1", "u3");
/// assert!(!delta.complete);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pulled {
    /// Users: all of them (snapshot), or the new and changed ones (delta).
    pub users: Vec<DirectoryUser>,
    /// Groups: all of them (snapshot), or the new and changed ones (delta).
    pub groups: Vec<DirectoryGroup>,
    /// Whether this is everything.
    pub complete: bool,
    /// Users removed from the directory (delta).
    pub removed_users: Vec<String>,
    /// Groups removed from the directory (delta).
    pub removed_groups: Vec<String>,
    /// Members added to or removed from groups (delta).
    pub member_changes: Vec<MemberChange>,
    /// Where the next pull continues from (a delta link, a timestamp). It
    /// is stored once the run applied every change, and handed to the
    /// next pull.
    pub cursor: Option<String>,
}

impl Pulled {
    /// Everything in the directory.
    pub fn snapshot(users: Vec<DirectoryUser>, groups: Vec<DirectoryGroup>) -> Self {
        Self {
            users,
            groups,
            complete: true,
            ..Self::default()
        }
    }

    /// The changes since the last pull.
    pub fn delta(users: Vec<DirectoryUser>, groups: Vec<DirectoryGroup>) -> Self {
        Self {
            users,
            groups,
            complete: false,
            ..Self::default()
        }
    }

    /// Sets the cursor for the next pull.
    #[must_use]
    pub fn cursor(mut self, cursor: impl Into<String>) -> Self {
        self.cursor = Some(cursor.into());
        self
    }

    /// Lists a user removed from the directory.
    #[must_use]
    pub fn removed_user(mut self, external_id: impl Into<String>) -> Self {
        self.removed_users.push(external_id.into());
        self
    }

    /// Lists a group removed from the directory.
    #[must_use]
    pub fn removed_group(mut self, external_id: impl Into<String>) -> Self {
        self.removed_groups.push(external_id.into());
        self
    }

    /// Lists a member added to a group.
    #[must_use]
    pub fn member_added(mut self, group: impl Into<String>, user: impl Into<String>) -> Self {
        self.member_changes.push(MemberChange {
            group: group.into(),
            user: user.into(),
            added: true,
        });
        self
    }

    /// Lists a member removed from a group.
    #[must_use]
    pub fn member_removed(mut self, group: impl Into<String>, user: impl Into<String>) -> Self {
        self.member_changes.push(MemberChange {
            group: group.into(),
            user: user.into(),
            added: false,
        });
        self
    }
}

/// A member added to or removed from a group, in a delta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberChange {
    /// The group's `external_id`.
    pub group: String,
    /// The user's `external_id`.
    pub user: String,
    /// Added (`true`) or removed (`false`).
    pub added: bool,
}
