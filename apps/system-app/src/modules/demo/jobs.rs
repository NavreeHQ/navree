//! Queue work for the demo: job types that succeed, run long, fail on
//! their first attempts, fail every time, send mail or notify, and the
//! route that queues them.
//!
//! `POST /api/demo/jobs` takes `{"kind": "...", "count": N}` and queues N
//! jobs of that kind:
//!
//! | kind | job | queue | outcome |
//! |---|---|---|---|
//! | `ok` | `demo.ok` | default | finishes in a moment |
//! | `slow` | `demo.slow` | default | works for 2 to 4 seconds |
//! | `flaky` | `demo.flaky` | default | fails twice, then succeeds |
//! | `mail-refused` | `mail.relay` | mail | `connection refused: smtp.internal:587`, 3 attempts |
//! | `export` | `contacts.export` | exports | `timed out after 120 s`, 3 attempts |
//! | `webhook-gone` | `webhooks.redeliver` | default | `endpoint answered 410 Gone`, 4 attempts |
//! | `newsletter` | `newsletter.send` | mail | sends an HTML mail |
//! | `notify` | `notifications.push` | notifications | stores and broadcasts a notification |
//!
//! Retries wait a few seconds, so a short run shows attempts and failures.

use std::time::Duration;

use navree::contracts::{Backoff, Job, JobContext, JobOptions};
use navree::http::{IntoResponse, Response};
use navree::mail::Mailer;
use navree::notify::{Channel, Notifiable, Notification, Notifier, USER_NOTIFIABLE_TYPE};
use navree::openapi::ToSchema;
use navree::prelude::*;
use navree::queue::Queue;
use navree::validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::random::between;

/// The seconds between the attempts of a job that fails.
const RETRY_SECS: u64 = 4;

/// Options for a job on `queue` that retries `attempts` times in all.
fn options(queue: &str, attempts: u32) -> JobOptions {
    JobOptions {
        queue: queue.to_owned(),
        max_attempts: attempts,
        backoff: Backoff::Fixed { secs: RETRY_SECS },
        timeout_secs: 60,
        ..JobOptions::default()
    }
}

/// `demo.ok`: a job that finishes in a moment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuickJob {
    /// Its number in the batch it was queued with.
    pub sequence: u32,
    /// The workspace that queued it.
    pub workspace: String,
}

impl Job for QuickJob {
    const NAME: &'static str = "demo.ok";

    fn options() -> JobOptions {
        options("default", 3)
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(between(15, 120))).await;
        Ok(())
    }
}

/// `demo.slow`: a job that works for two to four seconds.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlowJob {
    /// Its number in the batch it was queued with.
    pub sequence: u32,
    /// The workspace that queued it.
    pub workspace: String,
}

impl Job for SlowJob {
    const NAME: &'static str = "demo.slow";

    fn options() -> JobOptions {
        options("default", 3)
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(between(2_000, 4_000))).await;
        Ok(())
    }
}

/// `demo.flaky`: fails its first two attempts with a transient error and
/// succeeds on the third.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlakyJob {
    /// Its number in the batch it was queued with.
    pub sequence: u32,
    /// The workspace that queued it.
    pub workspace: String,
}

impl Job for FlakyJob {
    const NAME: &'static str = "demo.flaky";

    fn options() -> JobOptions {
        options("default", 3)
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(between(30, 200))).await;
        if ctx.attempt < 3 {
            return Err(format!(
                "temporarily unavailable: the sync service asked to retry (attempt {})",
                ctx.attempt
            )
            .into());
        }
        Ok(())
    }
}

/// `mail.relay`: hands a message to the internal relay, which refuses the
/// connection every time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelayMail {
    /// Who the message is for.
    pub to: String,
    /// Its subject.
    pub subject: String,
}

impl Job for RelayMail {
    const NAME: &'static str = "mail.relay";

    fn options() -> JobOptions {
        options("mail", 3)
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(between(20, 80))).await;
        Err("connection refused: smtp.internal:587".into())
    }
}

/// `contacts.export`: an export that times out every time. It reports the
/// timeout without waiting for it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportContacts {
    /// The workspace to export.
    pub workspace: String,
    /// The file format.
    pub format: String,
}

impl Job for ExportContacts {
    const NAME: &'static str = "contacts.export";

    fn options() -> JobOptions {
        options("exports", 3)
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(between(100, 400))).await;
        Err("timed out after 120 s".into())
    }
}

/// `webhooks.redeliver`: posts an event to an endpoint that answers 410
/// Gone every time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RedeliverWebhook {
    /// Where the event goes.
    pub endpoint: String,
    /// The event.
    pub event: String,
}

impl Job for RedeliverWebhook {
    const NAME: &'static str = "webhooks.redeliver";

    fn options() -> JobOptions {
        options("default", 4)
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(between(40, 160))).await;
        Err("endpoint answered 410 Gone".into())
    }
}

/// `newsletter.send`: sends one issue of the newsletter to one reader,
/// through the mail module.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendNewsletter {
    /// The reader's name.
    pub name: String,
    /// The reader's address.
    pub to: String,
    /// The issue number.
    pub issue: u32,
}

impl Job for SendNewsletter {
    const NAME: &'static str = "newsletter.send";

    fn options() -> JobOptions {
        options("mail", 3)
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let mailer = ctx.app.get::<Mailer>().await?;
        let mail = super::mail::newsletter(&self.name, &self.to, self.issue);
        mailer.send(&mail).await?;
        Ok(())
    }
}

/// The user a notification is for.
struct Recipient {
    id: Uuid,
}

impl Notifiable for Recipient {
    fn notifiable_type(&self) -> &'static str {
        USER_NOTIFIABLE_TYPE
    }

    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        None
    }
}

/// A task handed to someone, stored for them and broadcast to their
/// browser.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskAssigned {
    /// The task's title.
    pub task: String,
    /// The project it is part of.
    pub project: String,
    /// Who handed it over.
    pub assigned_by: String,
}

impl Notification for TaskAssigned {
    const TYPE: &'static str = "task_assigned";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Broadcast]
    }

    fn to_database(&self, _notifiable: &dyn Notifiable) -> Option<Value> {
        Some(json!({
            "title": "New task",
            "message": format!("{} assigned you \"{}\" in {}", self.assigned_by, self.task, self.project),
            "task": self.task,
            "project": self.project,
        }))
    }
}

/// `notifications.push`: notifies one user that a task is theirs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushNotification {
    /// The user to notify.
    pub user_id: Uuid,
    /// The notification.
    pub notification: TaskAssigned,
}

impl Job for PushNotification {
    const NAME: &'static str = "notifications.push";

    fn options() -> JobOptions {
        options("notifications", 3)
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let notifier = ctx.app.get::<Notifier>().await?;
        let recipient = Recipient { id: self.user_id };
        notifier.send_now(&recipient, self.notification).await?;
        Ok(())
    }
}

/// The kinds of job `POST /api/demo/jobs` queues.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum JobKind {
    /// `demo.ok`.
    Ok,
    /// `demo.slow`.
    Slow,
    /// `demo.flaky`.
    Flaky,
    /// `mail.relay`.
    MailRefused,
    /// `contacts.export`.
    Export,
    /// `webhooks.redeliver`.
    WebhookGone,
    /// `newsletter.send`.
    Newsletter,
    /// `notifications.push`.
    Notify,
}

/// The body of `POST /api/demo/jobs`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct DemoQueueJobs {
    /// Which job to queue.
    #[validate(skip)]
    pub kind: JobKind,
    /// How many, from 1 to 5000 (1 by default).
    #[validate(range(min = 1, max = 5000))]
    #[serde(default = "one")]
    pub count: u32,
}

fn one() -> u32 {
    1
}

/// What `POST /api/demo/jobs` answers.
#[derive(Debug, Serialize, ToSchema)]
pub struct DemoJobsQueued {
    /// How many jobs were queued.
    pub queued: u32,
    /// Which kind.
    pub kind: JobKind,
}

/// The route that queues demo jobs.
#[injectable]
pub struct JobsController {
    queue: Arc<Queue>,
}

#[controller("/demo")]
#[guard(InWorkspace::guard())]
#[authenticated]
impl JobsController {
    /// `POST /api/demo/jobs`: queues `count` jobs of `kind`; answers 202.
    #[post("/jobs")]
    #[audit(action = "demo.jobs_queued")]
    #[api(
        summary = "Queue demo jobs",
        tag = "Demo",
        request = DemoQueueJobs,
        response = DemoJobsQueued,
        status = 202,
        responses(400, 401, 403)
    )]
    async fn queue_jobs(
        &self,
        ws: CurrentWorkspace,
        CurrentUser(user): CurrentUser,
        Valid(Json(input)): Valid<Json<DemoQueueJobs>>,
    ) -> Result<Response> {
        let workspace = ws.workspace().slug.clone();
        let queue = &self.queue;
        let count = input.count;
        match input.kind {
            JobKind::Ok => {
                push(queue, count, |sequence| QuickJob {
                    sequence,
                    workspace: workspace.clone(),
                })
                .await?
            }
            JobKind::Slow => {
                push(queue, count, |sequence| SlowJob {
                    sequence,
                    workspace: workspace.clone(),
                })
                .await?
            }
            JobKind::Flaky => {
                push(queue, count, |sequence| FlakyJob {
                    sequence,
                    workspace: workspace.clone(),
                })
                .await?
            }
            JobKind::MailRefused => {
                push(queue, count, |sequence| {
                    let (to, _) = super::mail::recipient(sequence);
                    RelayMail {
                        to,
                        subject: "Your quarterly statement is ready".to_owned(),
                    }
                })
                .await?
            }
            JobKind::Export => {
                push(queue, count, |sequence| ExportContacts {
                    workspace: workspace.clone(),
                    format: if sequence % 3 == 0 { "xlsx" } else { "csv" }.to_owned(),
                })
                .await?
            }
            JobKind::WebhookGone => {
                push(queue, count, |sequence| RedeliverWebhook {
                    endpoint: "https://hooks.retired-partner.example/system".to_owned(),
                    event: if sequence % 2 == 0 {
                        "task.completed"
                    } else {
                        "project.archived"
                    }
                    .to_owned(),
                })
                .await?
            }
            JobKind::Newsletter => {
                let issue = 40 + u32::try_from(between(0, 3)).unwrap_or(0);
                push(queue, count, |sequence| {
                    let (to, name) = super::mail::recipient(sequence);
                    SendNewsletter { name, to, issue }
                })
                .await?
            }
            JobKind::Notify => {
                push(queue, count, |sequence| PushNotification {
                    user_id: user.id,
                    notification: super::mail::assignment(sequence),
                })
                .await?
            }
        }
        let mut response = Json(DemoJobsQueued {
            queued: count,
            kind: input.kind,
        })
        .into_response();
        *response.status_mut() = 202u16.try_into().expect("202 is a status code");
        Ok(response)
    }
}

/// Queues `count` jobs, each built from its number (from 1).
async fn push<J: Job>(queue: &Queue, count: u32, make: impl Fn(u32) -> J) -> Result<()> {
    for sequence in 1..=count {
        queue.dispatch(make(sequence)).await?;
    }
    Ok(())
}
