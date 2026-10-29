//! Held deliveries: [`NotificationDeferral`] rows, what the
//! [`Notifier`] holds when it sends (preferences, digests, quiet hours,
//! send windows) and how `notify:release` sends them later.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::sync::atomic::Ordering;

use chrono::{DateTime, TimeDelta, Utc};
use navree_contracts::ZoneLevel;
use navree_kernel::AppContext;
use navree_orm::{Model, Record};
use navree_time::{SendWindow, Zone, Zones};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::notifier::{has_database, now};
use crate::store::{Exec, on};
use crate::timing::next_open;
use crate::{
    Channel, DIGEST_TYPE, Delivery, Digest, DigestItem, Notifiable, Notification, Notifier,
    NotifyError, PreferenceOwner, ResolvedPreferences, SendNotificationJob, USER_NOTIFIABLE_TYPE,
};

/// Why a delivery is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeferralReason {
    /// Its mail waits for the recipient's digest.
    Digest,
    /// The recipient's quiet hours.
    QuietHours,
    /// The notification's send window.
    SendWindow,
}

impl DeferralReason {
    /// `digest`, `quiet_hours` or `send_window`.
    pub fn as_str(self) -> &'static str {
        match self {
            DeferralReason::Digest => "digest",
            DeferralReason::QuietHours => "quiet_hours",
            DeferralReason::SendWindow => "send_window",
        }
    }
}

impl fmt::Display for DeferralReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DeferralReason {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "digest" => DeferralReason::Digest,
            "quiet_hours" => DeferralReason::QuietHours,
            "send_window" => DeferralReason::SendWindow,
            other => return Err(format!("unknown deferral reason `{other}`")),
        })
    }
}

/// A row of `notification_deferrals`: one rendered channel of a
/// notification, held until `release_at` (the recipient's next digest,
/// the end of their quiet hours, or the notification's next send window).
/// `notify:release` sends the rows that are due: digest rows of one
/// recipient as one mail, the others as they were rendered.
///
/// ```no_run
/// use navree_notify::NotificationDeferral;
/// use navree_orm::Model as _;
/// # async fn demo(db: navree_db::Db, user: uuid::Uuid) -> navree_orm::Result<()> {
/// let waiting = NotificationDeferral::query()
///     .where_(NotificationDeferral::NOTIFIABLE_ID.eq(user.to_string()))
///     .oldest_by(NotificationDeferral::RELEASE_AT)
///     .get(&db)
///     .await?;
/// # Ok(()) }
/// ```
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "notification_deferrals")]
pub struct NotificationDeferral {
    /// The id (a UUID v7).
    pub id: Uuid,
    /// The notification ([`Notifier::send`]'s id).
    pub notification_id: Uuid,
    /// [`Notification::TYPE`].
    pub notification_type: String,
    /// [`Notification::CATEGORY`].
    pub category: Option<String>,
    /// The notifiable's type.
    pub notifiable_type: String,
    /// The notifiable's id.
    pub notifiable_id: String,
    /// The channel held (`mail`, `sms`...).
    pub channel: String,
    /// [`DeferralReason`] as text.
    pub reason: String,
    /// The [`Digest`] of a digest row (`daily`).
    pub digest: Option<String>,
    /// The rendered [`SendNotificationJob`], as JSON.
    #[serde(skip_serializing)]
    pub job: Value,
    /// When it goes out.
    pub release_at: DateTime<Utc>,
    /// How many releases were attempted.
    pub attempts: i32,
    /// When it was held.
    pub created_at: DateTime<Utc>,
}

impl NotificationDeferral {
    /// The reason, parsed.
    pub fn deferral_reason(&self) -> Option<DeferralReason> {
        self.reason.parse().ok()
    }

    /// The held job, parsed.
    pub fn send_job(&self) -> Result<SendNotificationJob, serde_json::Error> {
        serde_json::from_value(self.job.clone())
    }
}

/// What one [`Notifier::release_due`] run did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Released {
    /// Held deliveries sent (or queued) on their own.
    pub sent: usize,
    /// Digest mails sent (or queued).
    pub digests: usize,
    /// Mails batched into those digests.
    pub batched: usize,
    /// Held deliveries dropped: their channel was turned off since, they
    /// no longer read, or they failed `notify.deferred.max_attempts` times.
    pub dropped: usize,
    /// Releases that failed and will be tried again.
    pub failed: usize,
}

/// What [`Notifier::plan`] needs to know of the notification.
pub(crate) struct Meta {
    pub(crate) category: &'static str,
    pub(crate) digest: Digest,
    pub(crate) urgent: bool,
    pub(crate) window: Option<SendWindow>,
}

impl Meta {
    pub(crate) fn of<N: Notification>(notification: &N, notifiable: &dyn Notifiable) -> Meta {
        Meta {
            category: N::CATEGORY,
            digest: N::DIGEST,
            urgent: N::URGENT,
            window: notification.send_window(notifiable),
        }
    }
}

/// The rendered channels split into the ones to send now and the held
/// ones.
#[derive(Default)]
pub(crate) struct Planned {
    pub(crate) now: Vec<SendNotificationJob>,
    pub(crate) deferred: Vec<NotificationDeferral>,
}

impl Notifier {
    /// Applies the recipient's preferences and timing to `jobs`: channels
    /// turned off are dropped; the mail of a digest is held for the next
    /// digest; the channels `notify.deferred.channels` lists are held
    /// through quiet hours and outside the notification's send window.
    pub(crate) async fn plan(
        &self,
        ctx: &AppContext,
        notifiable: &dyn Notifiable,
        workspace: Option<Uuid>,
        meta: Meta,
        mut jobs: Vec<SendNotificationJob>,
    ) -> Result<Planned, NotifyError> {
        let Some(first) = jobs.first() else {
            return Ok(Planned::default());
        };
        let sent_at = first.created_at;
        let has_db = has_database(ctx).await;
        let options = &self.options;
        let resolved = if options.preferences.enabled && has_db {
            self.preferences.resolve(notifiable, workspace).await?
        } else {
            ResolvedPreferences {
                required: options.preferences.required.clone(),
                ..ResolvedPreferences::default()
            }
        };
        jobs.retain(|job| {
            let allowed = resolved.allows(meta.category, job.delivery.channel());
            if !allowed {
                tracing::debug!(
                    target: "navree::notify",
                    notification = %job.id,
                    channel = %job.delivery.channel(),
                    category = meta.category,
                    "skipped a channel the recipient turned off"
                );
            }
            allowed
        });
        let digest = if meta.urgent {
            Digest::Off
        } else {
            resolved.digest(meta.category, meta.digest)
        };
        let quiet = if meta.urgent {
            None
        } else {
            resolved.quiet_hours()
        };
        let digests_mail = digest.is_on()
            && jobs
                .iter()
                .any(|job| job.delivery.channel() == Channel::Mail);
        let times_held = (quiet.is_some() || meta.window.is_some())
            && jobs
                .iter()
                .any(|job| options.deferred.holds(job.delivery.channel()));
        if !digests_mail && !times_held {
            return Ok(Planned {
                now: jobs,
                deferred: Vec::new(),
            });
        }
        if !has_db {
            if !self.warned_no_hold.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    target: "navree::notify",
                    "holding notifications for digests, quiet hours and send windows needs DatabaseModule: they are sent now"
                );
            }
            return Ok(Planned {
                now: jobs,
                deferred: Vec::new(),
            });
        }
        let zone = recipient_zone(ctx, notifiable, workspace).await;
        let window = meta.window.as_ref();
        let quiet = quiet.as_ref();
        let open = next_open(sent_at, &zone, window, quiet);
        let window_reason = match window {
            Some(window) if window.next_slot(sent_at, &zone) > sent_at => {
                DeferralReason::SendWindow
            }
            _ => DeferralReason::QuietHours,
        };
        let mut planned = Planned::default();
        for job in jobs {
            let channel = job.delivery.channel();
            if channel == Channel::Mail && digest.is_on() {
                let at = digest
                    .next_at(
                        sent_at,
                        &zone,
                        options.digest.time(),
                        options.digest.weekday(),
                    )
                    .unwrap_or(sent_at);
                let at = next_open(at, &zone, window, quiet);
                planned
                    .deferred
                    .push(deferral(&job, DeferralReason::Digest, Some(digest), at)?);
            } else if options.deferred.holds(channel) && open > sent_at {
                planned
                    .deferred
                    .push(deferral(&job, window_reason, None, open)?);
            } else {
                planned.now.push(job);
            }
        }
        Ok(planned)
    }

    /// Stores held deliveries (in the current unit of work, if any).
    pub(crate) async fn hold(&self, rows: Vec<NotificationDeferral>) -> Result<(), NotifyError> {
        if rows.is_empty() {
            return Ok(());
        }
        let exec = Exec::current(&self.app).await?;
        on!(&exec, e => NotificationDeferral::insert_or_ignore(e, rows).await)?;
        exec.wrote();
        Ok(())
    }

    /// Sends the held deliveries that are due: the digest rows of one
    /// recipient and frequency as one digest mail, the others as they were
    /// rendered; queued as jobs `notify.send` with `notify.queue`. A
    /// channel the recipient has turned off since is dropped. A release
    /// that fails is tried again `notify.deferred.retry_secs` later, up to
    /// `notify.deferred.max_attempts` times. Several workers may run it at
    /// once: each row is claimed by one.
    ///
    /// `notify:release` runs it, every minute on `ScheduleModule`'s
    /// scheduler (`notify.deferred.schedule`).
    pub async fn release_due(&self) -> Result<Released, NotifyError> {
        let ctx = self.app()?;
        let exec = Exec::pool(&self.app).await?;
        let now = now(&ctx).await;
        let deferred = &self.options.deferred;
        let due = on!(&exec, e => NotificationDeferral::query()
            .where_(NotificationDeferral::RELEASE_AT.lte(now))
            .oldest_by(NotificationDeferral::RELEASE_AT)
            .limit(deferred.batch)
            .get(e)
            .await)?;
        let retry_at = now + TimeDelta::seconds(i64::try_from(deferred.retry_secs).unwrap_or(300));
        let mut claimed = Vec::new();
        for row in due.into_iter().map(Record::into_inner) {
            let attempts = row.attempts.saturating_add(1);
            let won = on!(&exec, e => NotificationDeferral::query()
                .where_(NotificationDeferral::ID.eq(row.id))
                .where_(NotificationDeferral::RELEASE_AT.lte(now))
                .update(e, [
                    NotificationDeferral::RELEASE_AT.set(retry_at),
                    NotificationDeferral::ATTEMPTS.set(attempts),
                ])
                .await)?;
            if won == 1 {
                claimed.push(NotificationDeferral { attempts, ..row });
            }
        }
        let mut released = Released::default();
        let mut units: Vec<Vec<(NotificationDeferral, SendNotificationJob)>> = Vec::new();
        let mut digests: BTreeMap<(String, String, String, Vec<String>), usize> = BTreeMap::new();
        for row in claimed {
            let job = match row.send_job() {
                Ok(job) => job,
                Err(error) => {
                    tracing::warn!(
                        target: "navree::notify",
                        deferral = %row.id,
                        %error,
                        "dropped a held notification that no longer reads"
                    );
                    self.forget_rows(&exec, &[row.id]).await?;
                    released.dropped += 1;
                    continue;
                }
            };
            let digest_key = match (&row.digest, &job.delivery) {
                (Some(digest), Delivery::Mail { mail })
                    if row.deferral_reason() == Some(DeferralReason::Digest) =>
                {
                    let mut to = mail.to.clone();
                    to.sort();
                    Some((
                        row.notifiable_type.clone(),
                        row.notifiable_id.clone(),
                        digest.clone(),
                        to,
                    ))
                }
                _ => None,
            };
            match digest_key {
                Some(key) => match digests.get(&key) {
                    Some(index) => units[*index].push((row, job)),
                    None => {
                        digests.insert(key, units.len());
                        units.push(vec![(row, job)]);
                    }
                },
                None => units.push(vec![(row, job)]),
            }
        }
        for unit in units {
            self.release_unit(&ctx, &exec, now, unit, &mut released)
                .await?;
        }
        Ok(released)
    }

    /// Sends one unit (a held delivery, or a digest's rows).
    async fn release_unit(
        &self,
        ctx: &AppContext,
        exec: &Exec,
        now: DateTime<Utc>,
        unit: Vec<(NotificationDeferral, SendNotificationJob)>,
        released: &mut Released,
    ) -> Result<(), NotifyError> {
        let Some((first, first_job)) = unit.first() else {
            return Ok(());
        };
        let workspace = first_job.workspace_id;
        let owner = PreferenceOwner {
            owner_type: first.notifiable_type.clone(),
            owner_id: first.notifiable_id.clone(),
        };
        let resolved = if self.options.preferences.enabled {
            Some(self.preferences.resolve_owner(&owner, workspace).await?)
        } else {
            None
        };
        let mut kept = Vec::new();
        let mut dropped = Vec::new();
        for (row, job) in unit {
            let channel = job.delivery.channel();
            let category = job.category.as_deref().unwrap_or(&job.notification_type);
            if resolved
                .as_ref()
                .is_some_and(|resolved| !resolved.allows(category, channel))
            {
                dropped.push(row.id);
            } else {
                kept.push((row, job));
            }
        }
        if !dropped.is_empty() {
            released.dropped += dropped.len();
            self.forget_rows(exec, &dropped).await?;
        }
        if kept.is_empty() {
            return Ok(());
        }
        let is_digest = kept.first().is_some_and(|(row, _)| {
            row.deferral_reason() == Some(DeferralReason::Digest) && row.digest.is_some()
        });
        let job = if is_digest {
            digest_job(&kept, now)
        } else {
            kept[0].1.clone()
        };
        let ids: Vec<Uuid> = kept.iter().map(|(row, _)| row.id).collect();
        let errors = self.send_jobs(ctx, &[job], self.options.queue).await;
        if errors.is_empty() {
            self.forget_rows(exec, &ids).await?;
            if is_digest {
                released.digests += 1;
                released.batched += kept.len();
            } else {
                released.sent += 1;
            }
            return Ok(());
        }
        let attempts = kept.iter().map(|(row, _)| row.attempts).max().unwrap_or(0);
        let max = i32::try_from(self.options.deferred.max_attempts).unwrap_or(i32::MAX);
        for error in &errors {
            tracing::warn!(
                target: "navree::notify",
                notification = %kept[0].0.notification_id,
                attempts,
                %error,
                "releasing a held notification failed"
            );
        }
        if attempts >= max {
            tracing::error!(
                target: "navree::notify",
                notification = %kept[0].0.notification_id,
                attempts,
                "dropped a held notification after its last attempt"
            );
            released.dropped += kept.len();
            self.forget_rows(exec, &ids).await?;
        } else {
            released.failed += 1;
        }
        Ok(())
    }

    /// Deletes held rows.
    async fn forget_rows(&self, exec: &Exec, ids: &[Uuid]) -> Result<(), NotifyError> {
        on!(exec, e => NotificationDeferral::query()
            .where_(NotificationDeferral::ID.is_in(ids.iter().copied()))
            .delete(e)
            .await)?;
        Ok(())
    }
}

/// The digest mail job of a recipient's held mails.
fn digest_job(
    rows: &[(NotificationDeferral, SendNotificationJob)],
    now: DateTime<Utc>,
) -> SendNotificationJob {
    let (first, first_job) = &rows[0];
    let digest = first
        .digest
        .as_deref()
        .and_then(|digest| digest.parse().ok())
        .unwrap_or(Digest::Daily);
    let mut to = Vec::new();
    let items = rows
        .iter()
        .filter_map(|(row, job)| match &job.delivery {
            Delivery::Mail { mail } => {
                for address in &mail.to {
                    if !to.contains(address) {
                        to.push(address.clone());
                    }
                }
                Some(DigestItem {
                    id: job.id,
                    notification_type: job.notification_type.clone(),
                    category: row.category.clone().or_else(|| job.category.clone()),
                    mail: mail.clone(),
                    created_at: job.created_at,
                })
            }
            _ => None,
        })
        .collect();
    SendNotificationJob {
        id: Uuid::now_v7(),
        notification_type: DIGEST_TYPE.to_owned(),
        notifiable_type: first.notifiable_type.clone(),
        notifiable_id: first.notifiable_id.clone(),
        created_at: now,
        category: None,
        workspace_id: first_job.workspace_id,
        delivery: Delivery::Digest { digest, to, items },
    }
}

/// A held row for `job`.
fn deferral(
    job: &SendNotificationJob,
    reason: DeferralReason,
    digest: Option<Digest>,
    release_at: DateTime<Utc>,
) -> Result<NotificationDeferral, NotifyError> {
    let payload = serde_json::to_value(job).map_err(|error| NotifyError::Queue(Box::new(error)))?;
    Ok(NotificationDeferral {
        id: Uuid::now_v7(),
        notification_id: job.id,
        notification_type: job.notification_type.clone(),
        category: job.category.clone(),
        notifiable_type: job.notifiable_type.clone(),
        notifiable_id: job.notifiable_id.clone(),
        channel: job.delivery.channel().as_str().to_owned(),
        reason: reason.as_str().to_owned(),
        digest: digest.map(|digest| digest.as_str().to_owned()),
        job: payload,
        release_at,
        attempts: 0,
        created_at: job.created_at,
    })
}

/// The zone the recipient's quiet hours, send windows and digests are
/// counted in: the notifiable's own ([`Notifiable::time_zone`]), else the
/// zone `TimeModule` gives the user, else the workspace's, else the app's
/// default zone; UTC without `TimeModule`.
pub(crate) async fn recipient_zone(
    ctx: &AppContext,
    notifiable: &dyn Notifiable,
    workspace: Option<Uuid>,
) -> Zone {
    if let Some(name) = notifiable.time_zone() {
        match Zone::parse(&name) {
            Ok(zone) => return zone,
            Err(error) => tracing::warn!(
                target: "navree::notify",
                %error,
                "ignoring a recipient's time zone that is not a zone"
            ),
        }
    }
    let Some(zones) = ctx.try_get::<Zones>().await else {
        return Zone::UTC;
    };
    let mut levels = Vec::new();
    if notifiable.notifiable_type() == USER_NOTIFIABLE_TYPE
        && let Ok(user) = Uuid::parse_str(&notifiable.notifiable_id())
    {
        levels.push((ZoneLevel::User, user));
    }
    if let Some(workspace) = workspace {
        levels.push((ZoneLevel::Workspace, workspace));
    }
    for (level, owner) in levels {
        match zones.stored_zone_of(level, owner).await {
            Ok(Some(zone)) => return zone,
            Ok(None) => {}
            Err(error) => tracing::warn!(
                target: "navree::notify",
                %level,
                %error,
                "looking up a recipient's time zone failed"
            ),
        }
    }
    zones.default_zone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_round_trip() {
        for reason in [
            DeferralReason::Digest,
            DeferralReason::QuietHours,
            DeferralReason::SendWindow,
        ] {
            assert_eq!(reason.as_str().parse::<DeferralReason>(), Ok(reason));
            assert_eq!(serde_json::to_value(reason).unwrap(), reason.as_str());
        }
        assert!("later".parse::<DeferralReason>().is_err());
    }
}
