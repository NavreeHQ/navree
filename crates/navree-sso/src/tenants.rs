//! Connections owned by workspaces and organizations, and the
//! `allow_unverified_emails` setting: who it counts for, and how its
//! refusals are logged.
//!
//! The setting lets a connection create accounts for addresses outside its
//! verified domains. It counts for connections the app owns; for a
//! connection owned by a workspace or an organization it counts only when
//! the app sets `sso.tenants.unverified_emails = true`.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Mutex, PoisonError};

use chrono::{DateTime, TimeDelta, Utc};
use uuid::Uuid;

use crate::{Provisioning, SsoOwner};

/// Whether a connection of `owner` with `provisioning` creates accounts for
/// addresses outside its verified domains: its `allow_unverified_emails` is
/// on, and the app owns it or `tenants_allowed`
/// (`sso.tenants.unverified_emails`) is on.
pub(crate) fn honours_unverified_emails(
    owner: SsoOwner,
    provisioning: &Provisioning,
    tenants_allowed: bool,
) -> bool {
    provisioning.allow_unverified_emails && (owner == SsoOwner::App || tenants_allowed)
}

/// How often the refusal line of one connection is written, in seconds.
pub(crate) const REFUSAL_LINE_EVERY_SECS: i64 = 3_600;

/// How many connections the refusal log keeps counts for.
pub(crate) const MAX_TRACKED_CONNECTIONS: usize = 10_000;

/// Counts the first sign-ins refused per connection, so that their warning
/// is written at most once an hour per connection, with the count since
/// the last line.
#[derive(Debug, Default)]
pub(crate) struct RefusalLog {
    /// Per connection: when its last line was written, and the refusals
    /// since then.
    seen: Mutex<HashMap<Uuid, (DateTime<Utc>, u64)>>,
}

impl RefusalLog {
    /// An empty log.
    pub(crate) fn new() -> RefusalLog {
        RefusalLog::default()
    }

    /// Notes a refusal of `connection` at `now`. `Some(count)` when a line
    /// is due (the connection's first refusal, then the first one at least
    /// [`REFUSAL_LINE_EVERY_SECS`] after its last line), with the refusals
    /// since the last line, this one included; `None` otherwise. A log that
    /// already tracks [`MAX_TRACKED_CONNECTIONS`] connections forgets them
    /// all before it notes a new one.
    pub(crate) fn note(&self, connection: Uuid, now: DateTime<Utc>) -> Option<u64> {
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        if seen.len() >= MAX_TRACKED_CONNECTIONS && !seen.contains_key(&connection) {
            seen.clear();
        }
        match seen.entry(connection) {
            Entry::Vacant(entry) => {
                entry.insert((now, 0));
                Some(1)
            }
            Entry::Occupied(mut entry) => {
                let (last, since) = entry.get_mut();
                *since += 1;
                if now - *last >= TimeDelta::seconds(REFUSAL_LINE_EVERY_SECS) {
                    let refusals = *since;
                    *last = now;
                    *since = 0;
                    Some(refusals)
                } else {
                    None
                }
            }
        }
    }

    /// How many connections the log tracks.
    #[cfg(test)]
    fn tracked(&self) -> usize {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_app_connections_honour_the_flag_unless_tenants_are_allowed() {
        let open = Provisioning {
            allow_unverified_emails: true,
            ..Provisioning::default()
        };
        let workspace = SsoOwner::Workspace(Uuid::now_v7());
        let organization = SsoOwner::Organization(Uuid::now_v7());
        assert!(honours_unverified_emails(SsoOwner::App, &open, false));
        assert!(!honours_unverified_emails(workspace, &open, false));
        assert!(!honours_unverified_emails(organization, &open, false));
        assert!(honours_unverified_emails(workspace, &open, true));
        assert!(!honours_unverified_emails(
            SsoOwner::App,
            &Provisioning::default(),
            true
        ));
    }

    #[test]
    fn refusal_log_writes_one_line_an_hour_per_connection() {
        let log = RefusalLog::new();
        let (ludhiana, amritsar) = (Uuid::now_v7(), Uuid::now_v7());
        let t0 = Utc::now();
        assert_eq!(log.note(ludhiana, t0), Some(1));
        assert_eq!(log.note(ludhiana, t0 + TimeDelta::seconds(10)), None);
        assert_eq!(log.note(ludhiana, t0 + TimeDelta::seconds(20)), None);
        assert_eq!(log.note(amritsar, t0 + TimeDelta::seconds(30)), Some(1));
        assert_eq!(log.note(ludhiana, t0 + TimeDelta::seconds(3_599)), None);
        assert_eq!(log.note(ludhiana, t0 + TimeDelta::seconds(3_600)), Some(4));
        assert_eq!(log.note(ludhiana, t0 + TimeDelta::seconds(3_601)), None);
    }

    #[test]
    fn refusal_log_stays_bounded() {
        let log = RefusalLog::new();
        let now = Utc::now();
        for _ in 0..MAX_TRACKED_CONNECTIONS + 5 {
            assert_eq!(log.note(Uuid::now_v7(), now), Some(1));
        }
        assert!(log.tracked() <= MAX_TRACKED_CONNECTIONS);
    }
}
