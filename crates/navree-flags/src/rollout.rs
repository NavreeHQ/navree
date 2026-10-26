//! [`Rollout`]: a flag on for a share of users or workspaces, optionally
//! limited to workspaces on some plans.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::Flag;

/// What a percentage rollout counts: users (the default) or workspaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RolloutUnit {
    /// A share of users: each user is bucketed by its id.
    #[default]
    Users,
    /// A share of workspaces: each workspace is bucketed by its id.
    Workspaces,
}

impl RolloutUnit {
    /// `users` or `workspaces`.
    pub fn as_str(self) -> &'static str {
        match self {
            RolloutUnit::Users => "users",
            RolloutUnit::Workspaces => "workspaces",
        }
    }
}

/// A rollout: the flag is on for `percent`% of users or workspaces,
/// optionally only for workspaces on some `plans`.
///
/// ```
/// use navree_flags::{Rollout, RolloutUnit};
///
/// let ten = Rollout::percent(10); // of users
/// assert_eq!(ten.of, RolloutUnit::Users);
///
/// let pro = Rollout::plans(["pro", "business"]); // every Pro and Business workspace
/// assert_eq!((pro.percent, pro.of), (100, RolloutUnit::Workspaces));
///
/// let half = Rollout::percent(50).of_workspaces().on_plans(["pro"]);
/// assert_eq!(half.to_string(), "50% of workspaces on pro");
/// ```
///
/// Each user or workspace falls in a bucket from 0 to 99 ([`bucket`]),
/// computed from the flag's name and its id; it is inside the rollout when
/// its bucket is below `percent`. The bucket never changes, so raising the
/// percentage only adds users or workspaces.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct Rollout {
    /// The share, from 0 to 100.
    pub percent: u8,
    /// Users or workspaces.
    #[serde(default)]
    pub of: RolloutUnit,
    /// When not empty, only workspaces on one of these plans (read through
    /// the app's `dyn PlanResolver`) are inside.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plans: Vec<String>,
}

impl Rollout {
    /// `percent`% of users (`of_workspaces` switches to workspaces).
    pub fn percent(percent: u8) -> Rollout {
        Rollout {
            percent,
            of: RolloutUnit::Users,
            plans: Vec::new(),
        }
    }

    /// Every workspace on one of `plans`.
    pub fn plans<I, S>(plans: I) -> Rollout
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Rollout::percent(100).of_workspaces().on_plans(plans)
    }

    /// Counts users.
    pub fn of_users(self) -> Rollout {
        Rollout {
            of: RolloutUnit::Users,
            ..self
        }
    }

    /// Counts workspaces.
    pub fn of_workspaces(self) -> Rollout {
        Rollout {
            of: RolloutUnit::Workspaces,
            ..self
        }
    }

    /// Only workspaces on one of `plans`.
    pub fn on_plans<I, S>(self, plans: I) -> Rollout
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Rollout {
            plans: plans.into_iter().map(Into::into).collect(),
            ..self
        }
    }

    /// Whether the user or workspace `id` falls inside the percentage for
    /// flag `F` (plans are not checked: they need the app).
    ///
    /// ```
    /// use navree_flags::{Flag, Rollout};
    /// use uuid::Uuid;
    ///
    /// struct NewPipeline;
    /// impl Flag for NewPipeline {
    ///     const NAME: &'static str = "new-pipeline";
    /// }
    ///
    /// let acme = Uuid::now_v7();
    /// let at_25 = Rollout::percent(25).of_workspaces().includes::<NewPipeline>(acme);
    /// let at_50 = Rollout::percent(50).of_workspaces().includes::<NewPipeline>(acme);
    /// assert!(!at_25 || at_50); // raising the percentage never removes anyone
    /// assert!(Rollout::percent(100).includes::<NewPipeline>(acme));
    /// assert!(!Rollout::percent(0).includes::<NewPipeline>(acme));
    /// ```
    pub fn includes<F: Flag>(&self, id: Uuid) -> bool {
        self.includes_named(F::NAME, id)
    }

    /// [`includes`](Rollout::includes) by flag name.
    pub fn includes_named(&self, flag: &str, id: Uuid) -> bool {
        bucket(flag, id) < self.percent
    }

    /// Checks the rollout: a percentage from 0 to 100 and non-blank plan
    /// names.
    pub fn check(&self) -> Result<(), String> {
        if self.percent > 100 {
            return Err(format!(
                "percent must not be greater than 100 (got {})",
                self.percent
            ));
        }
        if self.plans.iter().any(|plan| plan.trim().is_empty()) {
            return Err("plans must not contain empty names".to_owned());
        }
        Ok(())
    }
}

/// `25% of workspaces`, `plans pro,business`, `50% of workspaces on pro`.
impl fmt::Display for Rollout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plans = self.plans.join(",");
        if self.plans.is_empty() {
            write!(f, "{}% of {}", self.percent, self.of.as_str())
        } else if self.percent == 100 && self.of == RolloutUnit::Workspaces {
            write!(f, "plans {plans}")
        } else {
            write!(f, "{}% of {} on {plans}", self.percent, self.of.as_str())
        }
    }
}

/// The rollout bucket (0 to 99) of user or workspace `id` for flag `flag`:
/// the first eight bytes of SHA-256(`<flag>:<id>`), big-endian, modulo
/// 100. Stable across processes, platforms and releases; different flags
/// bucket the same id differently.
pub fn bucket(flag: &str, id: Uuid) -> u8 {
    let digest = Sha256::digest(format!("{flag}:{id}").as_bytes());
    let mut head = [0_u8; 8];
    head.copy_from_slice(&digest[..8]);
    // The remainder is below 100, so it fits in a u8.
    u8::try_from(u64::from_be_bytes(head) % 100).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_are_stable_and_spread() {
        let id = Uuid::parse_str("01966b54-4a3e-7c5e-8d3c-2f1d3c4b5a69").unwrap();
        let first = bucket("new-pipeline", id);
        assert_eq!(bucket("new-pipeline", id), first, "the same input, the same bucket");
        assert!(first < 100);

        // Different flags bucket the same ids differently, and ids spread
        // over the buckets.
        let ids: Vec<Uuid> = (0..2000_u128).map(Uuid::from_u128).collect();
        let a: Vec<u8> = ids.iter().map(|id| bucket("new-pipeline", *id)).collect();
        let b: Vec<u8> = ids.iter().map(|id| bucket("ai-summaries", *id)).collect();
        assert_ne!(a, b);
        let under_ten = a.iter().filter(|bucket| **bucket < 10).count();
        assert!(
            (120..=280).contains(&under_ten),
            "about 10% of 2000 ids are in buckets 0-9, got {under_ten}"
        );
    }

    #[test]
    fn bucket_matches_the_documented_formula() {
        let id = Uuid::from_u128(42);
        let digest = Sha256::digest(format!("deal-export:{id}").as_bytes());
        let expected = u64::from_be_bytes(digest[..8].try_into().unwrap()) % 100;
        assert_eq!(u64::from(bucket("deal-export", id)), expected);
    }

    #[test]
    fn builders_and_display() {
        assert_eq!(Rollout::percent(10).to_string(), "10% of users");
        assert_eq!(Rollout::percent(25).of_workspaces().to_string(), "25% of workspaces");
        assert_eq!(Rollout::plans(["pro", "business"]).to_string(), "plans pro,business");
        assert_eq!(
            Rollout::percent(50).of_workspaces().on_plans(["pro", "business"]).to_string(),
            "50% of workspaces on pro,business"
        );
        assert_eq!(Rollout::percent(5).of_workspaces().of_users().of, RolloutUnit::Users);
    }

    #[test]
    fn rollouts_are_checked() {
        assert!(Rollout::percent(100).check().is_ok());
        assert!(Rollout::percent(0).check().is_ok());
        assert!(Rollout::percent(101).check().is_err());
        assert!(Rollout::plans([" "]).check().is_err());
    }

    #[test]
    fn rollouts_serialize_compactly() {
        assert_eq!(
            serde_json::to_value(Rollout::percent(25).of_workspaces()).unwrap(),
            serde_json::json!({"percent": 25, "of": "workspaces"})
        );
        let plans: Rollout =
            serde_json::from_value(serde_json::json!({"percent": 50, "plans": ["pro"]})).unwrap();
        assert_eq!(plans, Rollout::percent(50).on_plans(["pro"]));
    }
}
