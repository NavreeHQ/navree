//! # crm-activities
//!
//! [`ActivitiesModule`] and [`DashboardModule`] (plan Task 35; spec §6, §7
//! and §9).
//!
//! | Route | Access | Result |
//! |---|---|---|
//! | `GET /api/v1/activities?subject_type=&subject_id=&kind=&assignee_id=&due=&completed=&page=&per_page=` | `read Activity` | page of [`ActivityResource`] ([`ActivityFilter`]) |
//! | `POST /api/v1/activities` [`CreateActivity`] | `create Activity` | 201 [`ActivityResource`], owned by the caller |
//! | `PATCH /api/v1/activities/{id}` [`UpdateActivity`] | `update Activity` (owner; managers) | [`ActivityResource`] |
//! | `POST /api/v1/activities/{id}/complete` | `complete Activity` (assignee or owner; managers) | [`ActivityResource`] |
//! | `DELETE /api/v1/activities/{id}` | `delete Activity` (owner; managers) | 204 |
//! | `GET /api/v1/dashboard` | `read Deal` | [`Dashboard`] |
//!
//! - The record an activity is attached to (`subject_type` + `subject_id`)
//!   must exist, not soft-deleted, and be readable; the assignee must be a
//!   user (else 400 `subject_id does not exist` / `assignee_id does not
//!   exist`). Only tasks are completed (else 400); completing twice keeps
//!   the first time.
//! - `due` follows spec §9.5 in UTC days ([`Due`], [`Today`]): `overdue` =
//!   open tasks due before today, `today` = due today, `upcoming` = due after
//!   today. Times come from the app's `dyn Clock`.
//! - The dashboard follows spec §9.6 (see [`Dashboard`]).
//! - The `crm:task-reminders` command ([`TaskReminders`]) notifies everyone
//!   with open tasks due today, the assignee or else the owner, once each
//!   ([`TasksDueToday`]: database, broadcast and mail; data `{title,
//!   message, url}`). `ActivitiesModule` schedules it daily at 08:00 UTC and
//!   `audit:prune` daily at 03:00 UTC (both without overlapping).
#![forbid(unsafe_code)]

mod controller;
mod dashboard;
mod dto;
mod due;
mod reminders;
mod resource;
mod service;

use crm_core::CrmCoreModule;
use keel::prelude::*;

pub use controller::ActivitiesController;
pub use dashboard::{
    Dashboard, DashboardController, DashboardService, DealTotals, OpenDeals, PipelineStage,
    RECENT_ACTIVITIES, TaskCounts,
};
pub use dto::{ActivityFilter, CreateActivity, UpdateActivity};
pub use due::{Due, Today};
pub use reminders::{
    DueTask, REMINDER_TEMPLATE, RemindersSent, TASK_REMINDERS_AT, TASK_REMINDERS_COMMAND,
    TASKS_TODAY_URL, TaskReminders, TasksDueToday,
};
pub use resource::{
    ACTIVITY_TYPE, ActivityPage, ActivityPageMeta, ActivityResource, MISSING_SUBJECT_LABEL, present,
};
pub use service::ActivityService;

/// When `audit:prune` runs every day (UTC).
pub const AUDIT_PRUNE_AT: &str = "03:00";

/// `/api/v1/activities`, the `crm:task-reminders` command and the daily
/// schedule: task reminders at 08:00 UTC, `audit:prune` at 03:00 UTC.
#[module(
    imports = [CrmCoreModule],
    providers = [ActivityService, TaskReminders],
    controllers = [ActivitiesController],
    exports = [ActivityService, TaskReminders],
    register = Self::wire,
)]
pub struct ActivitiesModule;

impl ActivitiesModule {
    /// The reminder command, its mail templates and the schedule.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.command(reminders::reminders_command());
        m.templates(&reminders::TEMPLATES);
        m.schedule(|s| {
            s.command(TASK_REMINDERS_COMMAND, &[])
                .daily_at(TASK_REMINDERS_AT)
                .without_overlapping();
            s.command("audit:prune", &[])
                .daily_at(AUDIT_PRUNE_AT)
                .without_overlapping();
        });
    }
}

/// `/api/v1/dashboard`.
#[module(
    imports = [CrmCoreModule],
    providers = [DashboardService],
    controllers = [DashboardController],
)]
pub struct DashboardModule;
