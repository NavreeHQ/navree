//! The demo module: routes and work that give the monitoring and queue
//! tools real material when traffic is thrown at the app.
//!
//! ```text
//! demo/
//! ├── mod.rs        DemoModule: what the module provides
//! ├── options.rs    the `demo` configuration section
//! ├── faults.rs     the typed errors the failure routes raise
//! ├── failures.rs   /api/demo/errors, slow-request, slow-query, n-plus-one, outgoing
//! ├── stubs.rs      /api/demo/stub and /api/demo/sink (no sign-in)
//! ├── jobs.rs       job types and POST /api/demo/jobs
//! ├── mail.rs       mail templates and POST /api/demo/mail
//! ├── schedule.rs   scheduled tasks
//! └── random.rs     random durations and chances
//! ```

mod failures;
mod faults;
mod jobs;
mod mail;
mod options;
mod random;
mod schedule;
mod stubs;

use navree::prelude::*;

pub use failures::FailuresController;
pub use jobs::{JobKind, JobsController};
pub use mail::MailController;
pub use options::DemoOptions;
pub use stubs::StubController;

/// The webhook event types the app announces, with what each means.
pub const WEBHOOK_TYPES: &[(&str, &str)] = &[
    ("task.completed", "A task was marked as done."),
    ("project.archived", "A project was archived."),
    ("report.requested", "A report was asked for."),
];

/// The demo routes, jobs, mail and scheduled tasks.
#[module(
    controllers = [FailuresController, StubController, JobsController, MailController],
    register = Self::wire,
)]
pub struct DemoModule;

impl DemoModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.config::<DemoOptions>();
        m.job::<jobs::QuickJob>()
            .job::<jobs::SlowJob>()
            .job::<jobs::FlakyJob>()
            .job::<jobs::RelayMail>()
            .job::<jobs::ExportContacts>()
            .job::<jobs::RedeliverWebhook>()
            .job::<jobs::SendNewsletter>()
            .job::<jobs::PushNotification>();
        m.templates(mail::TEMPLATES);
        for (name, description) in WEBHOOK_TYPES {
            m.webhook_type(*name, *description);
        }
        m.schedule(schedule::schedule);
    }
}
