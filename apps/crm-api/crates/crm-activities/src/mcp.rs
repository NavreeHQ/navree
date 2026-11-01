//! [`ActivitiesMcp`]: the activities part of the CRM's MCP server (`/mcp`,
//! with `McpModule` in the app): the `log_activity` tool.

use chrono::{DateTime, Utc};
use crm_core::{Activity, ActivityKind, SubjectType};
use navree::mcp::{McpContext, mcp_server};
use navree::openapi::ToSchema;
use navree::prelude::*;
use navree::validation::Validate;
use serde::Deserialize;
use uuid::Uuid;

use crate::dto::trimmed;
use crate::{ActivityResource, ActivityService, CreateActivity};

/// The arguments of `log_activity`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct LogActivity {
    /// The deal it is about.
    #[validate(skip)]
    pub deal_id: Uuid,
    /// What happened: a note, call, email or meeting, or a task to do.
    #[validate(skip)]
    pub kind: ActivityKind,
    /// The one-line summary.
    #[serde(deserialize_with = "trimmed")]
    #[validate(length(min = 1, max = 255))]
    pub subject: String,
    /// Details.
    #[serde(default)]
    #[validate(length(max = 10000))]
    pub body: Option<String>,
    /// When a task is due (RFC 3339).
    #[serde(default)]
    #[validate(skip)]
    pub due_at: Option<DateTime<Utc>>,
}

/// The activities part of the CRM's MCP server.
#[injectable]
pub struct ActivitiesMcp {
    activities: Arc<ActivityService>,
}

#[mcp_server]
impl ActivitiesMcp {
    /// Logs a call, email, meeting or note on a deal, or adds a task with a due date.
    #[mcp_tool]
    #[can(Create, Activity)]
    async fn log_activity(&self, cx: &McpContext, args: LogActivity) -> Result<ActivityResource> {
        self.activities
            .create(
                cx.uow(),
                cx.access(),
                CreateActivity {
                    kind: args.kind,
                    subject: args.subject,
                    body: args.body,
                    due_at: args.due_at,
                    subject_type: SubjectType::Deal,
                    subject_id: args.deal_id,
                    assignee_id: None,
                },
            )
            .await
    }
}
