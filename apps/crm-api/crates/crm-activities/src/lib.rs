//! # crm-activities
//!
//! [`ActivitiesModule`] and [`DashboardModule`].
//!
//! | Route | Access | Result |
//! |---|---|---|
//! | `GET /api/v1/activities?filter[subject_type]=&filter[subject_id]=&filter[kind]=&filter[assignee_id]=&filter[due]=&filter[completed]=&page=&per_page=` | `read Activity` | page of [`ActivityResource`] ([`ActivityList`]) |
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
//! - `filter[due]` works in UTC days ([`Due`], [`Today`]): `overdue` =
//!   open tasks due before today, `today` = due today, `upcoming` = due after
//!   today. Times come from the app's `dyn Clock`.
//! - For the dashboard, see [`Dashboard`].
//! - The `crm:task-reminders` command ([`TaskReminders`]) notifies everyone
//!   with open tasks due today, the assignee or else the owner, once each
//!   ([`TasksDueToday`]: database, broadcast and mail; data `{title,
//!   message, url}`). `ActivitiesModule` schedules it daily at 08:00 UTC and
//!   `audit:prune` daily at 03:00 UTC (both without overlapping).
//! - MCP ([`ActivitiesMcp`], served at `/mcp` when the app has
//!   `McpModule`): the tool `log_activity` (`create Activity`;
//!   [`LogActivity`]) logs an activity on a deal, as the user calling it.
#![forbid(unsafe_code)]

mod controller;
mod dashboard;
mod dto;
mod due;
mod mcp;
mod reminders;
mod resource;
mod service;

use crm_core::CrmCoreModule;
use navree::api::{ModuleBuilderApiExt, RouteDoc};
use navree::prelude::*;

pub use controller::ActivitiesController;
pub use dashboard::{
    Dashboard, DashboardController, DashboardService, DealTotals, OpenDeals, PipelineStage,
    RECENT_ACTIVITIES, TaskCounts,
};
pub use dto::{ActivityList, CreateActivity, UpdateActivity};
pub use due::{Due, Today};
pub use mcp::{ActivitiesMcp, LogActivity};
pub use reminders::{
    DueTask, REMINDER_TEMPLATE, RemindersSent, TASK_REMINDERS_AT, TASK_REMINDERS_COMMAND,
    TASKS_TODAY_URL, TaskReminders, TasksDueToday,
};
pub use resource::{
    ACTIVITY_TYPE, ActivityPage, ActivityPageMeta, ActivityResource, MISSING_SUBJECT_LABEL,
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
    /// The reminder command, its mail templates, the schedule and the
    /// activities part of the MCP server.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.api_route(RouteDoc::get("/api/v1/activities").list::<ActivityList>());
        m.mcp_server::<ActivitiesMcp>();
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
