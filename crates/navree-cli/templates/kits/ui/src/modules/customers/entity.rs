//! The `customers` table as an ORM model, and the two choices a customer
//! carries: a plan and a status.

use chrono::{DateTime, NaiveDate, Utc};
use navree::orm::Model;
use navree::ui::form::Options as _;
use navree::ui::table::option_label;
use navree::ui::{Options, Tone};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A customer. Query it with `Customer::query()`; create one with
/// `Customer::create(&uow, Customer { .. })`. The ORM sets the id (a UUID v7)
/// and the timestamps.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "customers", timestamps)]
pub struct Customer {
    /// The id (UUID v7, generated when the customer is created).
    pub id: Uuid,
    /// The person's name.
    pub name: String,
    /// The email address, lower case and unique.
    pub email: String,
    /// The phone number in E.164: `+919876501001`.
    pub phone: String,
    /// The city, one of [`CITIES`].
    pub city: String,
    /// The plan: the key of a [`Plan`].
    pub plan: String,
    /// Where the customer stands: the key of a [`Status`].
    pub status: String,
    /// What the customer pays each month, in paise (100 paise to the rupee).
    pub monthly_paise: i64,
    /// The day the customer joined.
    pub since: NaiveDate,
    /// Whether the customer pays by auto-pay.
    pub autopay: bool,
    /// Anything worth remembering.
    pub notes: Option<String>,
    /// When the customer was added.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// What a customer pays for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Options)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    /// Starter
    Starter,
    /// Growth
    #[default]
    Growth,
    /// Scale
    Scale,
}

impl Plan {
    /// Every plan.
    pub const ALL: [Plan; 3] = [Plan::Starter, Plan::Growth, Plan::Scale];

    /// The value stored in the `plan` column and posted by forms: `growth`.
    pub fn key(self) -> &'static str {
        self.option_value()
    }

    /// The plan with this key; Growth for a key nobody knows.
    pub fn parse(key: &str) -> Plan {
        Plan::ALL
            .into_iter()
            .find(|plan| plan.key() == key)
            .unwrap_or_default()
    }

    /// The name shown to people.
    pub fn label(self) -> &'static str {
        option_label(Plan::OPTIONS, self.key())
    }
}

/// Where a customer stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Options)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Active
    #[default]
    Active,
    /// On trial
    Trial,
    /// Paused
    Paused,
    /// Churned
    Churned,
}

impl Status {
    /// Every status.
    pub const ALL: [Status; 4] = [
        Status::Active,
        Status::Trial,
        Status::Paused,
        Status::Churned,
    ];

    /// The value stored in the `status` column and posted by forms: `trial`.
    pub fn key(self) -> &'static str {
        self.option_value()
    }

    /// The status with this key; Active for a key nobody knows.
    pub fn parse(key: &str) -> Status {
        Status::ALL
            .into_iter()
            .find(|status| status.key() == key)
            .unwrap_or_default()
    }

    /// The name shown to people.
    pub fn label(self) -> &'static str {
        option_label(Status::OPTIONS, self.key())
    }

    /// The colour of its badge.
    pub fn tone(self) -> Tone {
        match self {
            Status::Active => Tone::Success,
            Status::Trial => Tone::Info,
            Status::Paused => Tone::Warning,
            Status::Churned => Tone::Danger,
        }
    }
}

/// The plans a table filter offers: `(key, label)`.
pub const PLANS: &[(&str, &str)] = <Plan as navree::ui::form::Options>::OPTIONS;

/// The statuses a table filter offers: `(key, label)`.
pub const STATUSES: &[(&str, &str)] = <Status as navree::ui::form::Options>::OPTIONS;

/// The cities the demo customers live in, as a table filter offers them.
pub const CITIES: &[(&str, &str)] = &[
    ("Amritsar", "Amritsar"),
    ("Chandigarh", "Chandigarh"),
    ("Delhi", "Delhi"),
    ("Gurugram", "Gurugram"),
    ("Jaipur", "Jaipur"),
    ("Jalandhar", "Jalandhar"),
    ("Lucknow", "Lucknow"),
    ("Ludhiana", "Ludhiana"),
];
