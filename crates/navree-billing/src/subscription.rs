//! [`Subscription`], [`SubscriptionStatus`] and [`Proration`].

use std::fmt;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::entity::subscription;
use crate::plan::Interval;

/// The state of a subscription, as the provider reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    /// In its trial.
    Trialing,
    /// Paid and running.
    Active,
    /// A payment failed; the provider retries it.
    PastDue,
    /// Not charging, kept.
    Paused,
    /// Ended (or ending: see [`Subscription::ends_at`]).
    Canceled,
    /// The first payment has not gone through yet.
    Incomplete,
}

impl SubscriptionStatus {
    /// `trialing`, `active`, `past_due`, `paused`, `canceled` or
    /// `incomplete`.
    pub fn as_str(self) -> &'static str {
        match self {
            SubscriptionStatus::Trialing => "trialing",
            SubscriptionStatus::Active => "active",
            SubscriptionStatus::PastDue => "past_due",
            SubscriptionStatus::Paused => "paused",
            SubscriptionStatus::Canceled => "canceled",
            SubscriptionStatus::Incomplete => "incomplete",
        }
    }

    /// Parses [`as_str`](Self::as_str)'s output.
    pub fn parse(value: &str) -> Option<SubscriptionStatus> {
        Some(match value {
            "trialing" => SubscriptionStatus::Trialing,
            "active" => SubscriptionStatus::Active,
            "past_due" => SubscriptionStatus::PastDue,
            "paused" => SubscriptionStatus::Paused,
            "canceled" => SubscriptionStatus::Canceled,
            "incomplete" => SubscriptionStatus::Incomplete,
            _ => return None,
        })
    }
}

impl fmt::Display for SubscriptionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a plan or quantity change is charged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Proration {
    /// Prorate, and charge the difference on the next invoice (the
    /// default).
    #[default]
    NextInvoice,
    /// Prorate, and charge the difference at once.
    InvoiceNow,
    /// Don't prorate.
    None,
}

/// A billable's subscription: the local copy, read with one query.
///
/// The checks ([`valid`](Self::valid), [`on_trial`](Self::on_trial), ...)
/// are evaluated at the time the subscription was read (the app's
/// `dyn Clock`), with the configured `billing.past_due_grace_days`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Subscription {
    /// The local id (nil for a trial without a subscription).
    pub id: Uuid,
    /// `Billable::BILLABLE_TYPE` of the owner.
    pub billable_type: String,
    /// `Billable::billable_id` of the owner.
    pub billable_id: Uuid,
    /// The plan key.
    pub plan: String,
    /// `Monthly` or `Yearly`.
    pub interval: Interval,
    /// The provider's status.
    pub status: SubscriptionStatus,
    /// Seats.
    pub quantity: u32,
    /// When the trial ends (UTC).
    pub trial_ends_at: Option<DateTime<Utc>>,
    /// When a canceled subscription ends (UTC); set once it is canceled.
    pub ends_at: Option<DateTime<Utc>>,
    /// The start of the current period (UTC).
    pub current_period_start: Option<DateTime<Utc>>,
    /// The end of the current period (UTC).
    pub current_period_end: Option<DateTime<Utc>>,
    /// Since when it is past due.
    pub past_due_since: Option<DateTime<Utc>>,
    /// `stripe`, `paddle` or `fake`.
    pub provider: String,
    /// The provider's subscription id (none for a trial without a
    /// subscription).
    pub provider_id: Option<String>,
    /// The provider's customer id.
    pub provider_customer_id: Option<String>,
    /// When the local copy was created.
    pub created_at: DateTime<Utc>,
    #[serde(skip)]
    pub(crate) evaluated_at: DateTime<Utc>,
    #[serde(skip)]
    pub(crate) past_due_grace_days: u32,
}

impl Subscription {
    pub(crate) fn from_row(
        row: &subscription::Model,
        now: DateTime<Utc>,
        past_due_grace_days: u32,
    ) -> Subscription {
        Subscription {
            id: row.id,
            billable_type: row.billable_type.clone(),
            billable_id: row.billable_id,
            plan: row.plan.clone(),
            interval: Interval::parse(&row.interval).unwrap_or_default(),
            status: SubscriptionStatus::parse(&row.status)
                .unwrap_or(SubscriptionStatus::Incomplete),
            quantity: u32::try_from(row.quantity.max(0)).unwrap_or(u32::MAX),
            trial_ends_at: row.trial_ends_at,
            ends_at: row.ends_at,
            current_period_start: row.current_period_start,
            current_period_end: row.current_period_end,
            past_due_since: row.past_due_since,
            provider: row.provider.clone(),
            provider_id: Some(row.provider_id.clone()),
            provider_customer_id: Some(row.provider_customer_id.clone()),
            created_at: row.created_at,
            evaluated_at: now,
            past_due_grace_days,
        }
    }

    /// The time the checks are evaluated at.
    pub fn evaluated_at(&self) -> DateTime<Utc> {
        self.evaluated_at
    }

    /// Whether it is on plan `plan` (a plan key).
    pub fn on_plan(&self, plan: &str) -> bool {
        self.plan == plan
    }

    /// Trialing, with the trial not over yet.
    pub fn on_trial(&self) -> bool {
        self.status == SubscriptionStatus::Trialing
            && self
                .trial_ends_at
                .is_none_or(|ends| ends > self.evaluated_at)
            && !self.ended()
    }

    /// Canceled (now, or at the end of the period).
    pub fn canceled(&self) -> bool {
        self.ends_at.is_some() || self.status == SubscriptionStatus::Canceled
    }

    /// Canceled at the end of the period, which has not come yet: still
    /// usable, and [`resume`](crate::Billing::resume) undoes the cancel.
    pub fn on_grace_period(&self) -> bool {
        self.ends_at.is_some_and(|ends| ends > self.evaluated_at)
    }

    /// A payment failed and the provider is retrying it.
    pub fn past_due(&self) -> bool {
        self.status == SubscriptionStatus::PastDue
    }

    /// Paused: not charging.
    pub fn paused(&self) -> bool {
        self.status == SubscriptionStatus::Paused
    }

    /// Whether it has ended: canceled with `ends_at` passed (or no
    /// `ends_at`).
    pub fn ended(&self) -> bool {
        self.canceled() && !self.on_grace_period()
    }

    /// Whether it grants access: trialing, active, on its grace period
    /// after a cancellation, or past due for less than
    /// `billing.past_due_grace_days`.
    pub fn valid(&self) -> bool {
        if self.on_grace_period() {
            return !matches!(
                self.status,
                SubscriptionStatus::Paused | SubscriptionStatus::Incomplete
            );
        }
        if self.ended() {
            return false;
        }
        match self.status {
            SubscriptionStatus::Trialing => self.on_trial(),
            SubscriptionStatus::Active => true,
            SubscriptionStatus::PastDue => {
                let since = self.past_due_since.unwrap_or(self.evaluated_at);
                let grace = TimeDelta::days(i64::from(self.past_due_grace_days));
                since + grace > self.evaluated_at
            }
            SubscriptionStatus::Paused
            | SubscriptionStatus::Canceled
            | SubscriptionStatus::Incomplete => false,
        }
    }

    /// Whether this is a trial without a subscription
    /// ([`Billing::start_trial`](crate::Billing::start_trial)).
    pub fn is_trial_without_subscription(&self) -> bool {
        self.provider_id.is_none()
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap()
    }

    fn sub(status: SubscriptionStatus) -> Subscription {
        Subscription {
            id: Uuid::now_v7(),
            billable_type: "workspace".into(),
            billable_id: Uuid::now_v7(),
            plan: "pro".into(),
            interval: Interval::Monthly,
            status,
            quantity: 1,
            trial_ends_at: None,
            ends_at: None,
            current_period_start: Some(now() - TimeDelta::days(3)),
            current_period_end: Some(now() + TimeDelta::days(27)),
            past_due_since: None,
            provider: "fake".into(),
            provider_id: Some("sub_1".into()),
            provider_customer_id: Some("cus_1".into()),
            created_at: now(),
            evaluated_at: now(),
            past_due_grace_days: 7,
        }
    }

    #[test]
    fn active_is_valid() {
        let s = sub(SubscriptionStatus::Active);
        assert!(s.valid());
        assert!(!s.on_trial() && !s.canceled() && !s.on_grace_period() && !s.past_due());
        assert!(s.on_plan("pro") && !s.on_plan("starter"));
    }

    #[test]
    fn trials_are_valid_until_they_end() {
        let mut s = sub(SubscriptionStatus::Trialing);
        s.trial_ends_at = Some(now() + TimeDelta::days(14));
        assert!(s.on_trial() && s.valid());
        s.trial_ends_at = Some(now() - TimeDelta::seconds(1));
        assert!(!s.on_trial() && !s.valid());
    }

    #[test]
    fn a_cancel_keeps_it_valid_until_ends_at() {
        let mut s = sub(SubscriptionStatus::Active);
        s.ends_at = Some(now() + TimeDelta::days(10));
        assert!(s.canceled() && s.on_grace_period() && s.valid() && !s.ended());
        s.ends_at = Some(now() - TimeDelta::days(1));
        assert!(s.canceled() && !s.on_grace_period() && !s.valid() && s.ended());
        let mut s = sub(SubscriptionStatus::Canceled);
        assert!(s.canceled() && !s.valid());
        s.ends_at = Some(now() + TimeDelta::days(1));
        assert!(s.valid(), "canceled at period end, period not over");
    }

    #[test]
    fn past_due_is_valid_for_the_grace_days() {
        let mut s = sub(SubscriptionStatus::PastDue);
        s.past_due_since = Some(now() - TimeDelta::days(6));
        assert!(s.past_due() && s.valid());
        s.past_due_since = Some(now() - TimeDelta::days(7));
        assert!(!s.valid());
        s.past_due_grace_days = 0;
        s.past_due_since = Some(now());
        assert!(!s.valid());
    }

    #[test]
    fn paused_and_incomplete_are_not_valid() {
        assert!(!sub(SubscriptionStatus::Paused).valid());
        assert!(sub(SubscriptionStatus::Paused).paused());
        assert!(!sub(SubscriptionStatus::Incomplete).valid());
    }

    #[test]
    fn statuses_round_trip() {
        for status in [
            SubscriptionStatus::Trialing,
            SubscriptionStatus::Active,
            SubscriptionStatus::PastDue,
            SubscriptionStatus::Paused,
            SubscriptionStatus::Canceled,
            SubscriptionStatus::Incomplete,
        ] {
            assert_eq!(SubscriptionStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(SubscriptionStatus::parse("unpaid"), None);
    }
}
