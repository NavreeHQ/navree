//! Settings groups the panel edits under System, Settings: defaults for
//! tasks and the branding of mail and pages. Each is stored globally and
//! can be overridden per workspace (branding also per organization); a
//! field a level does not set inherits from the level above.

use navree::settings::Settings;
use navree::validation::Validate;
use serde::{Deserialize, Serialize};

/// What a new task starts with, and when finished work is archived.
#[derive(Settings, Serialize, Deserialize, Validate, Clone, Debug, PartialEq)]
#[settings(key = "task-defaults", scopes(global, workspace))]
pub struct TaskDefaults {
    /// The priority of a task created without one, from 1 (urgent) to 4.
    #[validate(range(min = 1, max = 4))]
    pub default_priority: i32,
    /// How many days after creation a task falls due when no date is set.
    #[validate(range(min = 1, max = 90))]
    pub default_due_days: i32,
    /// Finished tasks are archived after this many days.
    #[validate(range(min = 7, max = 365))]
    pub archive_after_days: i32,
    /// Whether a task needs an assignee before it can move to "doing".
    #[validate(skip)]
    pub require_assignee: bool,
}

impl Default for TaskDefaults {
    fn default() -> Self {
        Self {
            default_priority: 3,
            default_due_days: 14,
            archive_after_days: 90,
            require_assignee: false,
        }
    }
}

/// How the product presents itself in mail and on pages.
#[derive(Settings, Serialize, Deserialize, Validate, Clone, Debug, PartialEq)]
#[settings(key = "branding", scopes(global, organization, workspace))]
pub struct Branding {
    /// The name in the header of mail and pages.
    #[validate(length(min = 1, max = 60))]
    pub product_name: String,
    /// Where replies to mail and support requests go.
    #[validate(email)]
    pub support_email: String,
    /// The accent color of buttons and links, `#rrggbb`.
    #[validate(length(min = 7, max = 7))]
    pub accent_color: String,
    /// A line shown at the bottom of every mail.
    #[validate(length(max = 200))]
    pub mail_footer: String,
    /// Whether a "beta" badge shows next to the name.
    #[validate(skip)]
    pub show_beta_badge: bool,
}

impl Default for Branding {
    fn default() -> Self {
        Self {
            product_name: "Navree Evidence".to_owned(),
            support_email: "support@evidence.test".to_owned(),
            accent_color: "#3dff8f".to_owned(),
            mail_footer: "Sent by Navree Evidence.".to_owned(),
            show_beta_badge: false,
        }
    }
}
