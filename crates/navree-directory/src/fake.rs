//! [`DirectoryFake`]: a directory in memory that every pull connection of a
//! [`Directory::fake`](crate::Directory::fake) reads.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use navree_kernel::BoxFuture;
use uuid::Uuid;

use crate::error::DirectoryError;
use crate::pulled::Pulled;
use crate::source::{DirectorySource, PullRequest};
use crate::user::{DirectoryGroup, DirectoryUser};

/// A directory in memory for tests: the users and groups every pull
/// connection of a [`Directory::fake`](crate::Directory::fake) reads, the
/// pulls it answered, and pulls queued with a delta or a failure.
///
/// ```no_run
/// use navree_directory::{Directory, DirectoryGroup, DirectoryUser, Pulled};
///
/// let (directory, fake) = Directory::fake();
/// // TestApp::builder()...override_value(Arc::new(directory))
/// fake.user(DirectoryUser::new("u1", "ada@acme.test").email("ada@acme.test"))
///     .user(DirectoryUser::new("u2", "grace@acme.test"))
///     .group(DirectoryGroup::new("g1", "Engineering").members(["u1", "u2"]));
/// // The next pull answers with a delta instead of the snapshot.
/// fake.next_pull(Pulled::delta(vec![], vec![]).removed_user("u2"));
/// # let _ = directory;
/// ```
#[derive(Clone)]
pub struct DirectoryFake {
    state: Arc<FakeState>,
}

/// A pull the fake answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakePull {
    /// The connection.
    pub connection_id: Uuid,
    /// Its source.
    pub source: String,
    /// The cursor it was given.
    pub cursor: Option<String>,
}

#[derive(Default)]
pub(crate) struct FakeState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    users: Vec<DirectoryUser>,
    groups: Vec<DirectoryGroup>,
    queued: VecDeque<Result<Pulled, String>>,
    pulls: Vec<FakePull>,
}

impl FakeState {
    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        f(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl DirectoryFake {
    pub(crate) fn new() -> (DirectoryFake, Arc<FakeState>) {
        let state = Arc::new(FakeState::default());
        (
            DirectoryFake {
                state: state.clone(),
            },
            state,
        )
    }

    /// Adds a user, or replaces the one with the same `external_id`.
    pub fn user(&self, user: DirectoryUser) -> &Self {
        self.state.with(|inner| {
            match inner
                .users
                .iter_mut()
                .find(|u| u.external_id == user.external_id)
            {
                Some(existing) => *existing = user,
                None => inner.users.push(user),
            }
        });
        self
    }

    /// Replaces every user.
    pub fn users(&self, users: impl IntoIterator<Item = DirectoryUser>) -> &Self {
        let users: Vec<DirectoryUser> = users.into_iter().collect();
        self.state.with(|inner| inner.users = users);
        self
    }

    /// Removes a user (and their memberships).
    pub fn remove_user(&self, external_id: &str) -> &Self {
        self.state.with(|inner| {
            inner.users.retain(|u| u.external_id != external_id);
            for group in &mut inner.groups {
                group.members.retain(|m| m != external_id);
            }
        });
        self
    }

    /// Adds a group, or replaces the one with the same `external_id`.
    pub fn group(&self, group: DirectoryGroup) -> &Self {
        self.state.with(|inner| {
            match inner
                .groups
                .iter_mut()
                .find(|g| g.external_id == group.external_id)
            {
                Some(existing) => *existing = group,
                None => inner.groups.push(group),
            }
        });
        self
    }

    /// Replaces every group.
    pub fn groups(&self, groups: impl IntoIterator<Item = DirectoryGroup>) -> &Self {
        let groups: Vec<DirectoryGroup> = groups.into_iter().collect();
        self.state.with(|inner| inner.groups = groups);
        self
    }

    /// Removes a group.
    pub fn remove_group(&self, external_id: &str) -> &Self {
        self.state
            .with(|inner| inner.groups.retain(|g| g.external_id != external_id));
        self
    }

    /// The next pull answers with `pulled` (a delta, for instance) instead
    /// of the snapshot. Several queue up in order.
    pub fn next_pull(&self, pulled: Pulled) -> &Self {
        self.state.with(|inner| inner.queued.push_back(Ok(pulled)));
        self
    }

    /// The next pull fails with `message` (a source failure).
    pub fn fail_next_pull(&self, message: impl Into<String>) -> &Self {
        let message = message.into();
        self.state
            .with(|inner| inner.queued.push_back(Err(message)));
        self
    }

    /// The snapshot a pull answers with.
    pub fn snapshot(&self) -> Pulled {
        self.state
            .with(|inner| Pulled::snapshot(inner.users.clone(), inner.groups.clone()))
    }

    /// The pulls answered so far.
    pub fn pulls(&self) -> Vec<FakePull> {
        self.state.with(|inner| inner.pulls.clone())
    }
}

impl std::fmt::Debug for DirectoryFake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.state.with(|inner| {
            f.debug_struct("DirectoryFake")
                .field("users", &inner.users.len())
                .field("groups", &inner.groups.len())
                .field("pulls", &inner.pulls.len())
                .finish_non_exhaustive()
        })
    }
}

impl DirectorySource for FakeState {
    fn pull<'a>(&'a self, request: &'a PullRequest) -> BoxFuture<'a, Result<Pulled, DirectoryError>> {
        Box::pin(async move {
            let answer = self.with(|inner| {
                inner.pulls.push(FakePull {
                    connection_id: request.connection.id,
                    source: request.connection.source.clone(),
                    cursor: request.cursor.clone(),
                });
                let count = inner.pulls.len();
                inner.queued.pop_front().unwrap_or_else(|| {
                    Ok(Pulled::snapshot(inner.users.clone(), inner.groups.clone())
                        .cursor(format!("fake-{count}")))
                })
            });
            answer.map_err(|message| DirectoryError::source_failed("the fake directory", message))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_keeps_a_directory() {
        let (fake, _state) = DirectoryFake::new();
        fake.user(DirectoryUser::new("u1", "ada"))
            .user(DirectoryUser::new("u2", "grace"))
            .user(DirectoryUser::new("u1", "ada.lovelace"))
            .group(DirectoryGroup::new("g1", "Engineering").members(["u1", "u2"]));
        let snapshot = fake.snapshot();
        assert_eq!(snapshot.users.len(), 2);
        assert_eq!(snapshot.users[0].user_name, "ada.lovelace");
        fake.remove_user("u2");
        assert_eq!(fake.snapshot().groups[0].members, vec!["u1"]);
        fake.remove_group("g1");
        assert!(fake.snapshot().groups.is_empty());
        fake.users([DirectoryUser::new("u9", "alan")]);
        fake.groups([DirectoryGroup::new("g9", "Ops")]);
        assert_eq!(fake.snapshot().users[0].external_id, "u9");
        assert!(format!("{fake:?}").contains("users: 1"));
    }
}
