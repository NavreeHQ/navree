//! Connections owned by workspaces and organizations, and the
//! `allow_unverified_emails` setting: who it counts for, how its refusals
//! are logged, and the boot's lines about the connections that have it on.
//!
//! The setting lets a connection create accounts for addresses outside its
//! verified domains. It counts for connections the app owns; for a
//! connection owned by a workspace or an organization it counts outside the
//! verified domains only when the app sets
//! `sso.tenants.unverified_emails = true`. Inside the verified domains it
//! counts for every connection.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, TimeDelta, Utc};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Lifecycle, ProviderKey, Resolver,
};
use uuid::Uuid;

use crate::{Provisioning, Sso, SsoError, SsoOwner};

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

/// A connection with `allow_unverified_emails` on, as the boot names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Flagged {
    /// Its slug.
    pub(crate) slug: String,
    /// Who owns it.
    pub(crate) owner: SsoOwner,
}

/// A line to log: its level and its message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    /// The level.
    pub(crate) level: tracing::Level,
    /// The message.
    pub(crate) message: String,
}

/// How many connections a boot line names before `and N more`.
pub(crate) const MAX_NAMED: usize = 20;

/// A connection as the lines name it: `{slug}` for one the app owns,
/// `{slug} ({kind} {id})` for a workspace's or an organization's.
fn named(slug: &str, owner: SsoOwner) -> String {
    match owner.id() {
        Some(id) => format!("{slug} ({} {id})", owner.kind()),
        None => slug.to_owned(),
    }
}

/// The names of `connections`, joined with `, `: at most [`MAX_NAMED`],
/// then `, and {n} more`.
fn names(connections: &[&Flagged]) -> String {
    let list = connections
        .iter()
        .take(MAX_NAMED)
        .map(|connection| named(&connection.slug, connection.owner))
        .collect::<Vec<_>>()
        .join(", ");
    let more = connections.len().saturating_sub(MAX_NAMED);
    if more > 0 {
        format!("{list}, and {more} more")
    } else {
        list
    }
}

/// The boot's lines about the connections with `allow_unverified_emails`
/// on: one for those owned by workspaces and organizations (a warning
/// unless `tenants_allowed`, `sso.tenants.unverified_emails`), one for the
/// app's, and none when there are none.
pub(crate) fn boot_lines(flagged: &[Flagged], tenants_allowed: bool) -> Vec<Line> {
    let (app, tenants): (Vec<&Flagged>, Vec<&Flagged>) = flagged
        .iter()
        .partition(|connection| connection.owner == SsoOwner::App);
    let mut lines = Vec::new();
    if !tenants.is_empty() {
        let list = names(&tenants);
        let line = if tenants_allowed {
            Line {
                level: tracing::Level::INFO,
                message: format!(
                    "single sign-on connections {list} owned by workspaces or organizations create accounts outside their verified domains (sso.tenants.unverified_emails = true)"
                ),
            }
        } else {
            Line {
                level: tracing::Level::WARN,
                message: format!(
                    "single sign-on connections {list} have allow_unverified_emails on; connections not owned by the app no longer create accounts outside their verified domains. Set sso.tenants.unverified_emails = true to allow it."
                ),
            }
        };
        lines.push(line);
    }
    if !app.is_empty() {
        let list = names(&app);
        lines.push(Line {
            level: tracing::Level::INFO,
            message: format!(
                "single sign-on connections {list} owned by the app create accounts outside their verified domains (allow_unverified_emails)"
            ),
        });
    }
    lines
}

/// The line written after connection `slug` of `owner` is saved with
/// `allow_unverified_emails` on: information for a connection the app owns
/// or when `tenants_allowed` (`sso.tenants.unverified_emails`), otherwise a
/// warning that the setting has no effect outside the verified domains.
pub(crate) fn change_line(slug: &str, owner: SsoOwner, tenants_allowed: bool) -> Line {
    let name = named(slug, owner);
    if owner == SsoOwner::App {
        Line {
            level: tracing::Level::INFO,
            message: format!(
                "single sign-on connection {name} owned by the app creates accounts outside its verified domains (allow_unverified_emails)"
            ),
        }
    } else if tenants_allowed {
        Line {
            level: tracing::Level::INFO,
            message: format!(
                "single sign-on connection {name} creates accounts outside its verified domains (sso.tenants.unverified_emails = true)"
            ),
        }
    } else {
        Line {
            level: tracing::Level::WARN,
            message: format!(
                "single sign-on connection {name} has allow_unverified_emails on, which has no effect outside its verified domains for a connection the app doesn't own; set sso.tenants.unverified_emails = true to allow it"
            ),
        }
    }
}

/// The boot's line when the connections could not be read.
pub(crate) fn scan_failed_line(error: &SsoError) -> Line {
    Line {
        level: tracing::Level::WARN,
        message: format!("could not check the single sign-on connections at boot: {error}"),
    }
}

/// Writes `line` at its level, under the target `navree::sso`.
pub(crate) fn emit(line: &Line) {
    match line.level {
        tracing::Level::ERROR => tracing::error!(target: "navree::sso", "{}", line.message),
        tracing::Level::WARN => tracing::warn!(target: "navree::sso", "{}", line.message),
        tracing::Level::INFO => tracing::info!(target: "navree::sso", "{}", line.message),
        tracing::Level::DEBUG => tracing::debug!(target: "navree::sso", "{}", line.message),
        _ => tracing::trace!(target: "navree::sso", "{}", line.message),
    }
}

/// Names, when the app boots, the connections that have
/// `allow_unverified_emails` on ([`boot_lines`]). A scan that fails is a
/// warning ([`scan_failed_line`]): the boot goes on either way.
pub(crate) struct TenantScan {
    sso: Arc<Sso>,
}

impl Injectable for TenantScan {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Sso>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(TenantScan {
                sso: r.get::<Sso>().await?,
            })
        })
    }
}

impl Lifecycle for TenantScan {
    fn on_bootstrap(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            let tenants_allowed = self.sso.tenants_allow_unverified_emails();
            let lines = match self.sso.flagged_connections().await {
                Ok(flagged) => boot_lines(&flagged, tenants_allowed),
                Err(error) => vec![scan_failed_line(&error)],
            };
            for line in &lines {
                emit(line);
            }
            Ok(())
        })
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

    #[test]
    fn boot_lines_name_each_kind_of_connection() {
        let ws = Uuid::now_v7();
        let flagged = [
            Flagged {
                slug: "ludhiana-okta".into(),
                owner: SsoOwner::Workspace(ws),
            },
            Flagged {
                slug: "staff-okta".into(),
                owner: SsoOwner::App,
            },
        ];
        let off = boot_lines(&flagged, false);
        assert_eq!(off.len(), 2);
        assert_eq!(off[0].level, tracing::Level::WARN);
        let warning = &off[0].message;
        let workspace = format!("ludhiana-okta (workspace {ws})");
        assert!(warning.contains(&workspace));
        assert!(warning.contains("Set sso.tenants.unverified_emails = true to allow it."));
        assert_eq!(off[1].level, tracing::Level::INFO);
        assert!(off[1].message.contains("staff-okta owned by the app"));
        let on = boot_lines(&flagged, true);
        assert_eq!(on[0].level, tracing::Level::INFO);
        let allowed = &on[0].message;
        assert!(allowed.contains("(sso.tenants.unverified_emails = true)"));
        assert!(boot_lines(&[], false).is_empty());
    }

    #[test]
    fn boot_lines_name_at_most_twenty() {
        // Ids of their own, so no id can read as one of the slugs.
        let many: Vec<Flagged> = (0..25)
            .map(|n| Flagged {
                slug: format!("c-{n:02}"),
                owner: SsoOwner::Workspace(Uuid::from_u128(n)),
            })
            .collect();
        let lines = boot_lines(&many, false);
        let listed = &lines[0].message;
        assert!(listed.contains("c-19") && !listed.contains("c-20"));
        assert!(listed.contains("and 5 more"));
    }

    #[test]
    fn change_lines_by_owner() {
        let ws = Uuid::now_v7();
        let app = change_line("staff-okta", SsoOwner::App, false);
        assert_eq!(app.level, tracing::Level::INFO);
        let off = change_line("ludhiana-okta", SsoOwner::Workspace(ws), false);
        assert_eq!(off.level, tracing::Level::WARN);
        let expected = format!(
            "ludhiana-okta (workspace {ws}) has allow_unverified_emails on, which has no effect"
        );
        assert!(off.message.contains(&expected));
        let on = change_line("ludhiana-okta", SsoOwner::Workspace(ws), true);
        assert_eq!(on.level, tracing::Level::INFO);
    }

    #[test]
    fn a_failed_scan_is_a_warning() {
        let line = scan_failed_line(&SsoError::Config("no such table: sso_connections".into()));
        assert_eq!(line.level, tracing::Level::WARN);
        let message = &line.message;
        assert!(message.starts_with("could not check the single sign-on connections at boot: "));
    }
}
