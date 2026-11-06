//! The daily task reminders: the `crm:task-reminders` command,
//! scheduled at 08:00 UTC, notifies everyone with open tasks due today
//! (UTC) — the assignee, or the owner of an unassigned task — with one
//! [`TasksDueToday`] notification each: stored for `/api/notifications`,
//! broadcast on `private-users.{id}` and mailed.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use crm_core::models::{Activity, ActivityScopes, User};
use crm_core::{NotifiableUser, NotificationData};
use navree::config::Config;
use navree::contracts::Clock;
use navree::db::Db;
use navree::kernel::CommandSpec;
use navree::mail::TemplateMail;
use navree::notify::{Channel, Notifiable, Notification, Notifier};
use navree::orm::Record;
use navree::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::due::{Due, Today};

/// The command (and scheduled task) name.
pub const TASK_REMINDERS_COMMAND: &str = "crm:task-reminders";

/// When the reminders run every day (UTC).
pub const TASK_REMINDERS_AT: &str = "08:00";

/// Where a reminder takes the user in crm-web: today's tasks.
pub const TASKS_TODAY_URL: &str = "/tasks?tab=today";

/// The mail template (`.html` and `.txt`), embedded by `ActivitiesModule`;
/// a file of the same name in a template directory overrides it.
pub const REMINDER_TEMPLATE: &str = "mail/crm/tasks-due-today";

/// The embedded mail templates.
pub(crate) const TEMPLATES: [(&str, &str); 2] = [
    (
        "mail/crm/tasks-due-today.html",
        include_str!("../templates/tasks-due-today.html"),
    ),
    (
        "mail/crm/tasks-due-today.txt",
        include_str!("../templates/tasks-due-today.txt"),
    ),
];

/// How many task subjects a reminder's message names.
const NAMED_TASKS: usize = 3;

/// A task in a reminder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DueTask {
    /// The task's subject.
    pub subject: String,
    /// When it is due.
    pub due_at: DateTime<Utc>,
}

/// "You have N tasks due today": the reminder one user gets.
///
/// Its `data` (stored and broadcast) is the CRM's
/// [`NotificationData`]: `{title: "Tasks due today", message, url:
/// "/tasks?tab=today"}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasksDueToday {
    /// The recipient's name (the mail's greeting).
    pub name: String,
    /// The tasks, earliest due first.
    pub tasks: Vec<DueTask>,
    /// The absolute link to today's tasks (the mail's button).
    pub link: String,
}

impl TasksDueToday {
    /// `"N task(s) due today"`.
    pub fn headline(&self) -> String {
        match self.tasks.len() {
            1 => "1 task due today".to_owned(),
            n => format!("{n} tasks due today"),
        }
    }

    /// `"You have 2 tasks due today: A and B."`, naming at most three
    /// tasks (`"A, B, C and 2 more."`).
    pub fn message(&self) -> String {
        let mut names: Vec<String> = self
            .tasks
            .iter()
            .take(NAMED_TASKS)
            .map(|task| task.subject.clone())
            .collect();
        let more = self.tasks.len().saturating_sub(NAMED_TASKS);
        if more > 0 {
            names.push(format!("{more} more"));
        }
        let list = match names.split_last() {
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            None => String::new(),
        };
        format!("You have {}: {list}.", self.headline())
    }

    /// The notification's `data`.
    pub fn data(&self) -> NotificationData {
        NotificationData::new("Tasks due today", self.message(), TASKS_TODAY_URL)
    }
}

impl Notification for TasksDueToday {
    const TYPE: &'static str = "tasks_due_today";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Broadcast, Channel::Mail]
    }

    fn to_database(&self, _notifiable: &dyn Notifiable) -> Option<Value> {
        serde_json::to_value(self.data()).ok()
    }

    fn to_mail(&self, _notifiable: &dyn Notifiable) -> Option<TemplateMail> {
        let tasks: Vec<Value> = self
            .tasks
            .iter()
            .map(|task| {
                json!({
                    "subject": task.subject,
                    "due": task.due_at.format("%H:%M").to_string(),
                })
            })
            .collect();
        Some(TemplateMail {
            to: Vec::new(),
            subject: self.headline(),
            template: REMINDER_TEMPLATE.to_owned(),
            context: json!({
                "name": self.name,
                "summary": format!("You have {}:", self.headline()),
                "tasks": tasks,
                "url": self.link,
            }),
        })
    }
}

/// What a reminder run did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RemindersSent {
    /// The users notified.
    pub users: usize,
    /// The tasks they were reminded of.
    pub tasks: usize,
}

/// Sends the daily task reminders: one [`TasksDueToday`] notification to
/// each user with open tasks due today (UTC), as their assignee or as the
/// owner of an unassigned task.
#[injectable]
pub struct TaskReminders {
    db: Arc<Db>,
    notifier: Arc<Notifier>,
    clock: Arc<dyn Clock>,
    config: Arc<Config>,
}

impl TaskReminders {
    /// Reminds everyone of their open tasks due today (the app's clock).
    pub async fn send(&self) -> Result<RemindersSent, BoxError> {
        self.send_for(self.clock.now()).await
    }

    /// Reminds everyone of their open tasks due on `now`'s UTC day. Every
    /// user is attempted; the error names the ones whose notification
    /// failed (on some channel).
    pub async fn send_for(&self, now: DateTime<Utc>) -> Result<RemindersSent, BoxError> {
        let tasks = Due::Today
            .apply(Activity::query().open_tasks(), Today::of(now))
            .order_by(Activity::DUE_AT)
            .order_by(Activity::ID)
            .with_all(["owner", "assignee"])
            .get(&*self.db)
            .await?;
        // One reminder per recipient: the assignee, or the owner of an
        // unassigned task.
        let mut reminders: BTreeMap<Uuid, (Record<User>, Vec<DueTask>)> = BTreeMap::new();
        for task in tasks.iter() {
            let recipient = match task.assignee_id {
                Some(_) => task.loaded(Activity::assignee),
                None => task.loaded(Activity::owner),
            };
            let (Some(recipient), Some(due_at)) = (recipient.and_then(Option::as_ref), task.due_at)
            else {
                continue;
            };
            reminders
                .entry(recipient.id)
                .or_insert_with(|| (recipient.clone(), Vec::new()))
                .1
                .push(DueTask {
                    subject: task.subject.clone(),
                    due_at,
                });
        }
        let link = format!(
            "{}{TASKS_TODAY_URL}",
            self.config
                .get_string("app.url")
                .unwrap_or_default()
                .trim_end_matches('/')
        );
        let mut sent = RemindersSent::default();
        let mut failed = Vec::new();
        for (user, tasks) in reminders.into_values() {
            let count = tasks.len();
            let reminder = TasksDueToday {
                name: user.name.clone(),
                tasks,
                link: link.clone(),
            };
            match self
                .notifier
                .send(&NotifiableUser::from(&user), reminder)
                .await
            {
                Ok(_) => {
                    sent.users += 1;
                    sent.tasks += count;
                }
                Err(error) => {
                    tracing::warn!(
                        target: "crm::reminders",
                        user = %user.id,
                        %error,
                        "task reminder failed"
                    );
                    failed.push(user.email.clone());
                }
            }
        }
        tracing::info!(
            target: "crm::reminders",
            users = sent.users,
            tasks = sent.tasks,
            "task reminders sent"
        );
        if failed.is_empty() {
            Ok(sent)
        } else {
            Err(format!(
                "task reminders failed for {} user(s): {}",
                failed.len(),
                failed.join(", ")
            )
            .into())
        }
    }
}

/// `crm:task-reminders`: [`TaskReminders::send`], run by the scheduler at
/// 08:00 UTC or by hand.
pub(crate) fn reminders_command() -> CommandSpec {
    CommandSpec::new(
        TASK_REMINDERS_COMMAND,
        "Notify everyone of their open tasks due today (UTC)",
        |ctx, args| async move {
            if !args.is_empty() {
                return Err(format!("{TASK_REMINDERS_COMMAND} takes no arguments").into());
            }
            let sent = ctx.get::<TaskReminders>().await?.send().await?;
            println!(
                "Reminded {} user(s) of {} task(s) due today.",
                sent.users, sent.tasks
            );
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn reminder(subjects: &[&str]) -> TasksDueToday {
        TasksDueToday {
            name: "Priya Verma".to_owned(),
            tasks: subjects
                .iter()
                .map(|subject| DueTask {
                    subject: (*subject).to_owned(),
                    due_at: Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap(),
                })
                .collect(),
            link: "http://localhost:3000/tasks?tab=today".to_owned(),
        }
    }

    #[test]
    fn messages_name_up_to_three_tasks() {
        assert_eq!(reminder(&["A"]).message(), "You have 1 task due today: A.");
        assert_eq!(
            reminder(&["A", "B"]).message(),
            "You have 2 tasks due today: A and B."
        );
        assert_eq!(
            reminder(&["A", "B", "C"]).message(),
            "You have 3 tasks due today: A, B and C."
        );
        assert_eq!(
            reminder(&["A", "B", "C", "D"]).message(),
            "You have 4 tasks due today: A, B, C and 1 more."
        );
    }

    #[test]
    fn data_has_the_notification_shape() {
        assert_eq!(
            serde_json::to_value(reminder(&["A"]).data()).unwrap(),
            json!({
                "title": "Tasks due today",
                "message": "You have 1 task due today: A.",
                "url": "/tasks?tab=today",
            })
        );
    }

    #[test]
    fn the_templates_render_every_task() {
        let views = navree::view::Views::from_templates(TEMPLATES).expect("templates compile");
        let mail = reminder(&["Call <Agra>", "Send deck"])
            .to_mail(&NotifiableUser {
                id: Uuid::nil(),
                name: "Priya Verma".to_owned(),
                email: "rep1@navree.test".to_owned(),
            })
            .expect("a mail");
        assert_eq!(mail.subject, "2 tasks due today");
        let text = views
            .render("mail/crm/tasks-due-today.txt", &mail.context)
            .expect("text renders");
        assert_eq!(
            text,
            "Hello Priya Verma,\n\nYou have 2 tasks due today:\n\
             - Call <Agra>, due 09:00 UTC\n\
             - Send deck, due 09:00 UTC\n\n\
             Open your tasks: http://localhost:3000/tasks?tab=today"
        );
        let html = views
            .render("mail/crm/tasks-due-today.html", &mail.context)
            .expect("html renders");
        assert!(html.contains("Call &lt;Agra&gt;"), "{html}");
        // HTML escaping writes `/` as `&#x2f;` in HTML, which browsers decode.
        assert!(
            html.replace("&#x2f;", "/")
                .contains(r#"href="http://localhost:3000/tasks?tab=today""#),
            "{html}"
        );
    }
}
