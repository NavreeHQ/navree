//! Mail for the demo: HTML templates for a welcome, an invoice, a password
//! reset, a weekly digest and a newsletter, and `POST /api/demo/mail`,
//! which queues N varied messages (1 to 500) with distinct recipients
//! through the mail module.
//!
//! The messages go out through the configured transport, which in the
//! evidence stack is the mail catcher.

use std::sync::atomic::{AtomicU32, Ordering};

use chrono::{Datelike, Duration, Utc};

use navree::http::{IntoResponse, Response};
use navree::mail::{Mailer, TemplateMail};
use navree::openapi::ToSchema;
use navree::prelude::*;
use navree::validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::jobs::TaskAssigned;
use super::random::between;

/// The templates, by name.
pub const TEMPLATES: &[(&str, &str)] = &[
    (
        "mail/demo/layout.html",
        include_str!("templates/layout.html"),
    ),
    (
        "mail/demo/welcome.html",
        include_str!("templates/welcome.html"),
    ),
    (
        "mail/demo/invoice.html",
        include_str!("templates/invoice.html"),
    ),
    (
        "mail/demo/password-reset.html",
        include_str!("templates/password-reset.html"),
    ),
    (
        "mail/demo/weekly-digest.html",
        include_str!("templates/weekly-digest.html"),
    ),
    (
        "mail/demo/newsletter.html",
        include_str!("templates/newsletter.html"),
    ),
];

const FIRST_NAMES: [&str; 30] = [
    "Aarav",
    "Balwinder",
    "Charanjit",
    "Davinder",
    "Esha",
    "Fateh",
    "Gauri",
    "Harpreet",
    "Inderjit",
    "Jaspreet",
    "Kavya",
    "Lakhvir",
    "Manpreet",
    "Navdeep",
    "Onkar",
    "Priya",
    "Rajveer",
    "Simran",
    "Tarun",
    "Uma",
    "Vikram",
    "Yashpal",
    "Zorawar",
    "Amandeep",
    "Bhavna",
    "Chetan",
    "Deepika",
    "Gurleen",
    "Harleen",
    "Ishaan",
];

const LAST_NAMES: [&str; 30] = [
    "Ahluwalia",
    "Bhatia",
    "Chaudhry",
    "Dhillon",
    "Grewal",
    "Gill",
    "Hundal",
    "Johal",
    "Kapoor",
    "Khanna",
    "Lamba",
    "Malhotra",
    "Nanda",
    "Oberoi",
    "Puri",
    "Randhawa",
    "Sandhu",
    "Sethi",
    "Sharma",
    "Sidhu",
    "Talwar",
    "Uppal",
    "Verma",
    "Walia",
    "Arora",
    "Brar",
    "Chopra",
    "Dua",
    "Mehra",
    "Bajwa",
];

const WORKSPACES: [&str; 3] = ["Agra Logistics", "Gulati Research", "Indrani Software"];

const SERVICES: [&str; 4] = [
    "Platform seats",
    "Extra storage",
    "Priority support",
    "Audit retention",
];

const PROJECTS: [&str; 8] = [
    "Website relaunch",
    "Warehouse scanners",
    "Billing migration",
    "Partner portal",
    "Route optimisation",
    "Security review",
    "Support handbook",
    "Quarterly planning",
];

const TASKS: [&str; 8] = [
    "Review the pricing page copy",
    "Test the new scanner firmware",
    "Reconcile March invoices",
    "Draft the reseller onboarding guide",
    "Compare two routes for the north depot",
    "Rotate the staging credentials",
    "Answer the ten most asked questions",
    "Agree next quarter's budget",
];

/// A recipient number that moves on with every request, so two requests
/// do not mail the same people.
static NEXT_RECIPIENT: AtomicU32 = AtomicU32::new(0);

/// The `n`th recipient: an address and a display name. Different for
/// every `n` below 900.
pub fn recipient(n: u32) -> (String, String) {
    let first = FIRST_NAMES[(n % 30) as usize];
    let last = LAST_NAMES[((n / 30) % 30) as usize];
    (
        format!(
            "{}.{}@evidence.test",
            first.to_lowercase(),
            last.to_lowercase()
        ),
        format!("{first} {last}"),
    )
}

/// A task handed to a person, for the `n`th notification.
pub fn assignment(n: u32) -> TaskAssigned {
    let at = (n % 8) as usize;
    let (_, by) = recipient(n + 7);
    TaskAssigned {
        task: TASKS[at].to_owned(),
        project: PROJECTS[at].to_owned(),
        assigned_by: by,
    }
}

/// The newsletter for `name` at `to`.
pub fn newsletter(name: &str, to: &str, issue: u32) -> TemplateMail {
    TemplateMail::new(
        to,
        format!("The Evidence Dispatch, issue {issue}"),
        "mail/demo/newsletter",
    )
    .context(json!({
        "name": name,
        "issue": issue,
        "headline": "Shorter routes, fewer surprises",
        "articles": [
            {
                "title": "How the north depot cut its pick time",
                "summary": "Handheld scanners and a new shelf layout took eleven minutes off the average order.",
                "url": "https://evidence.test/dispatch/north-depot"
            },
            {
                "title": "Five checks before you archive a project",
                "summary": "What to confirm with the owner, the finance team and the people who still have tasks open.",
                "url": "https://evidence.test/dispatch/archive-checklist"
            },
            {
                "title": "Release notes for the autumn update",
                "summary": "Bulk actions on tasks, a faster board and clearer invoice emails.",
                "url": "https://evidence.test/dispatch/autumn-update"
            }
        ],
        "unsubscribe_url": format!("https://evidence.test/unsubscribe?address={to}")
    }))
}

/// The `n`th varied message: a welcome, an invoice, a password reset or a
/// weekly digest, by turns, each for its own recipient.
pub fn varied(n: u32) -> TemplateMail {
    let (to, name) = recipient(n);
    let today = Utc::now().date_naive();
    let due = today + Duration::days(5 + i64::from(n % 20));
    let monday = today
        - Duration::days(i64::from(today.weekday().num_days_from_monday()) + 7 * i64::from(n % 3));
    match n % 4 {
        0 => {
            let workspace = WORKSPACES[(n % 3) as usize];
            TemplateMail::new(&to, "Welcome to Navree Evidence", "mail/demo/welcome").context(
                json!({
                    "name": name,
                    "workspace": workspace,
                    "start_url": "https://evidence.test/start"
                }),
            )
        }
        1 => {
            let lines = u32::try_from(between(2, 4)).unwrap_or(2);
            let items: Vec<_> = (0..lines)
                .map(|line| {
                    let quantity = 1 + (n + line) % 5;
                    let unit = 4_900 + 1_250 * ((n + line) % 7);
                    let description = SERVICES[(line % 4) as usize];
                    json!({
                        "description": description,
                        "quantity": quantity,
                        "amount_cents": quantity * unit
                    })
                })
                .collect();
            let total: u32 = items
                .iter()
                .filter_map(|item| item["amount_cents"].as_u64())
                .filter_map(|cents| u32::try_from(cents).ok())
                .sum();
            let number = 20_400 + n;
            TemplateMail::new(
                &to,
                format!("Invoice INV-{number} is ready"),
                "mail/demo/invoice",
            )
            .context(json!({
                "name": name,
                "number": format!("INV-{number}"),
                "currency": "INR",
                "total_cents": total,
                "due": due.format("%-d %B %Y").to_string(),
                "items": items
            }))
        }
        2 => TemplateMail::new(&to, "Reset your password", "mail/demo/password-reset").context(
            json!({
                "name": name,
                "reset_url": format!("https://evidence.test/reset?token=demo-{n:06}"),
                "expires_minutes": 60,
                "ip": format!("203.0.113.{}", 10 + n % 200)
            }),
        ),
        _ => TemplateMail::new(&to, "Your week in Evidence", "mail/demo/weekly-digest").context(
            json!({
                "name": name,
                "week": format!("Week of {}", monday.format("%-d %B")),
                "done": 6 + n % 17,
                "created": 9 + n % 13,
                "active_projects": 4 + n % 9,
                "highlights": [
                    TASKS[(n % 8) as usize],
                    TASKS[((n + 3) % 8) as usize],
                    TASKS[((n + 5) % 8) as usize]
                ]
            }),
        ),
    }
}

/// The body of `POST /api/demo/mail`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct DemoQueueMail {
    /// How many messages, from 1 to 500 (1 by default).
    #[validate(range(min = 1, max = 500))]
    #[serde(default = "one")]
    pub count: u32,
}

fn one() -> u32 {
    1
}

/// What `POST /api/demo/mail` answers.
#[derive(Debug, Serialize, ToSchema)]
pub struct DemoMailQueued {
    /// How many messages were queued.
    pub queued: u32,
    /// Always `mail`.
    pub kind: &'static str,
}

/// The route that queues varied mail.
#[injectable]
pub struct MailController {
    mailer: Arc<Mailer>,
}

#[controller("/demo")]
#[guard(InWorkspace::guard())]
#[authenticated]
impl MailController {
    /// `POST /api/demo/mail`: renders and queues `count` messages; answers
    /// 202.
    #[post("/mail")]
    #[audit(action = "demo.mail_queued")]
    #[api(
        summary = "Queue demo mail",
        tag = "Demo",
        request = DemoQueueMail,
        response = DemoMailQueued,
        status = 202,
        responses(400, 401, 403)
    )]
    async fn queue_mail(&self, Valid(Json(input)): Valid<Json<DemoQueueMail>>) -> Result<Response> {
        let first = NEXT_RECIPIENT.fetch_add(input.count, Ordering::Relaxed);
        for n in 0..input.count {
            self.mailer.queue(&varied(first + n)).await?;
        }
        let mut response = Json(DemoMailQueued {
            queued: input.count,
            kind: "mail",
        })
        .into_response();
        *response.status_mut() = 202u16.try_into().expect("202 is a status code");
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipients_are_distinct() {
        let mut seen = std::collections::BTreeSet::new();
        for n in 0..900 {
            assert!(seen.insert(recipient(n).0), "recipient {n} repeats");
        }
    }

    #[test]
    fn every_template_compiles_and_renders() {
        let views = navree::view::Views::from_templates(TEMPLATES.iter().copied()).unwrap();
        for n in 0..8 {
            let mail = varied(n);
            let rendered = views.render(&mail.template, &mail.context).unwrap();
            assert!(rendered.contains("<html"), "{}", mail.template);
        }
        let issue = newsletter("Priya Raman", "priya.raman@evidence.test", 41);
        assert!(
            views
                .render(&issue.template, &issue.context)
                .unwrap()
                .contains("issue 41")
        );
    }
}
