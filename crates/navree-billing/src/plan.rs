//! [`Plan`] and [`Interval`]: the configured plans.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::config::PlanOptions;

/// How often a subscription renews.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Interval {
    /// Every month (the default).
    #[default]
    Monthly,
    /// Every year.
    Yearly,
}

impl Interval {
    /// `monthly` or `yearly`.
    pub fn as_str(self) -> &'static str {
        match self {
            Interval::Monthly => "monthly",
            Interval::Yearly => "yearly",
        }
    }

    /// Parses `monthly` or `yearly`.
    pub fn parse(value: &str) -> Option<Interval> {
        match value {
            "monthly" => Some(Interval::Monthly),
            "yearly" => Some(Interval::Yearly),
            _ => None,
        }
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A configured plan (`billing.plans.<key>`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Plan {
    /// The key you use in code (`pro`).
    pub key: String,
    /// The display name (the key when none is configured).
    pub name: String,
    /// The provider's monthly price id.
    pub monthly_price: Option<String>,
    /// The provider's yearly price id.
    pub yearly_price: Option<String>,
    /// The trial on checkout, in days (0: none).
    pub trial_days: u32,
    /// Whether the quantity follows the workspace's member count.
    pub seats: bool,
    /// Meter key → the provider's metered price id.
    pub meters: BTreeMap<String, String>,
}

impl Plan {
    pub(crate) fn from_options(key: &str, options: &PlanOptions) -> Plan {
        Plan {
            key: key.to_owned(),
            name: options.name.clone().unwrap_or_else(|| key.to_owned()),
            monthly_price: options.prices.monthly.clone(),
            yearly_price: options.prices.yearly.clone(),
            trial_days: options.trial_days,
            seats: options.seats,
            meters: options.meters.clone(),
        }
    }

    /// The price id for `interval`, if the plan has one.
    pub fn price(&self, interval: Interval) -> Option<&str> {
        match interval {
            Interval::Monthly => self.monthly_price.as_deref(),
            Interval::Yearly => self.yearly_price.as_deref(),
        }
    }

    /// The intervals the plan has a price for.
    pub fn intervals(&self) -> Vec<Interval> {
        [Interval::Monthly, Interval::Yearly]
            .into_iter()
            .filter(|interval| self.price(*interval).is_some())
            .collect()
    }

    /// The metered price id of `meter`, if the plan has that meter.
    pub fn meter_price(&self, meter: &str) -> Option<&str> {
        self.meters.get(meter).map(String::as_str)
    }

    /// The metered price ids, in meter key order.
    pub fn meter_prices(&self) -> Vec<String> {
        self.meters.values().cloned().collect()
    }
}

/// What a price id stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PriceRole {
    /// The base (licensed) price of a plan.
    Base {
        /// The plan key.
        plan: String,
        /// Monthly or yearly.
        interval: Interval,
    },
    /// A metered price.
    Meter {
        /// The meter key.
        meter: String,
    },
}

/// Every configured plan, sorted by key.
#[derive(Clone, Debug, Default)]
pub(crate) struct Plans {
    plans: Vec<Plan>,
}

impl Plans {
    pub(crate) fn new(options: &BTreeMap<String, PlanOptions>) -> Plans {
        Plans {
            plans: options
                .iter()
                .map(|(key, plan)| Plan::from_options(key, plan))
                .collect(),
        }
    }

    pub(crate) fn all(&self) -> &[Plan] {
        &self.plans
    }

    pub(crate) fn find(&self, key: &str) -> Option<&Plan> {
        self.plans.iter().find(|plan| plan.key == key)
    }

    /// What `price` is: a plan's base price, a meter's price, or unknown.
    pub(crate) fn role_of(&self, price: &str) -> Option<PriceRole> {
        for plan in &self.plans {
            for interval in [Interval::Monthly, Interval::Yearly] {
                if plan.price(interval) == Some(price) {
                    return Some(PriceRole::Base {
                        plan: plan.key.clone(),
                        interval,
                    });
                }
            }
        }
        self.plans.iter().find_map(|plan| {
            plan.meters
                .iter()
                .find(|(_, id)| id.as_str() == price)
                .map(|(meter, _)| PriceRole::Meter {
                    meter: meter.clone(),
                })
        })
    }

    /// Whether any plan has meter `meter`.
    pub(crate) fn has_meter(&self, meter: &str) -> bool {
        self.plans
            .iter()
            .any(|plan| plan.meters.contains_key(meter))
    }

    /// Every price id of every plan (base and metered).
    pub(crate) fn price_ids(&self) -> Vec<String> {
        let mut prices: Vec<String> = self
            .plans
            .iter()
            .flat_map(|plan| {
                plan.monthly_price
                    .iter()
                    .chain(plan.yearly_price.iter())
                    .chain(plan.meters.values())
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect();
        prices.sort();
        prices.dedup();
        prices
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PlanPrices;

    fn plans() -> Plans {
        let mut options = BTreeMap::new();
        options.insert(
            "pro".to_owned(),
            PlanOptions {
                name: Some("Pro".into()),
                prices: PlanPrices {
                    monthly: Some("price_pro_m".into()),
                    yearly: Some("price_pro_y".into()),
                },
                trial_days: 14,
                seats: true,
                meters: BTreeMap::from([("ai".to_owned(), "price_ai".to_owned())]),
            },
        );
        options.insert(
            "starter".to_owned(),
            PlanOptions {
                prices: PlanPrices {
                    monthly: Some("price_starter_m".into()),
                    yearly: None,
                },
                ..PlanOptions::default()
            },
        );
        Plans::new(&options)
    }

    #[test]
    fn plans_are_found_by_key_and_by_price() {
        let plans = plans();
        assert_eq!(plans.all().len(), 2);
        let pro = plans.find("pro").unwrap();
        assert_eq!(pro.name, "Pro");
        assert_eq!(pro.price(Interval::Yearly), Some("price_pro_y"));
        assert_eq!(pro.intervals(), [Interval::Monthly, Interval::Yearly]);
        assert_eq!(pro.meter_price("ai"), Some("price_ai"));
        let starter = plans.find("starter").unwrap();
        assert_eq!(starter.name, "starter");
        assert_eq!(starter.intervals(), [Interval::Monthly]);
        assert_eq!(starter.price(Interval::Yearly), None);

        assert_eq!(
            plans.role_of("price_pro_y"),
            Some(PriceRole::Base {
                plan: "pro".into(),
                interval: Interval::Yearly
            })
        );
        assert_eq!(
            plans.role_of("price_ai"),
            Some(PriceRole::Meter { meter: "ai".into() })
        );
        assert_eq!(plans.role_of("price_other"), None);
        assert!(plans.has_meter("ai"));
        assert!(!plans.has_meter("emails"));
        assert_eq!(
            plans.price_ids(),
            ["price_ai", "price_pro_m", "price_pro_y", "price_starter_m"]
        );
    }

    #[test]
    fn intervals_parse_and_print() {
        assert_eq!(Interval::parse("yearly"), Some(Interval::Yearly));
        assert_eq!(Interval::parse("weekly"), None);
        assert_eq!(Interval::Monthly.to_string(), "monthly");
        assert_eq!(Interval::default(), Interval::Monthly);
    }
}
