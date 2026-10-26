//! [`ActivitiesMcp`]: the activities part of the CRM's MCP server (`/mcp`,
//! with `McpModule` in the app): the `log_activity` tool.

use chrono::{DateTime, Utc};
use crm_core::{Activity, ActivityKind, SubjectType};
use garde::Validate;
use navree::mcp::{McpContext, mcp_server};
use navree::prelude::*;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::trimmed;
use crate::{ActivityResource, ActivityService, CreateActivity};

/// The arguments of `log_activity`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct LogActivity {
    /// The deal it is about.
    #[garde(skip)]
    pub deal_id: Uuid,
    /// What happened: a note, call, email or meeting, or a task to do.
    #[garde(skip)]
    pub kind: ActivityKind,
    /// The one-line summary.
    #[serde(deserialize_with = "trimmed")]
    #[garde(length(min = 1, max = 255))]
    pub subject: String,
    /// Details.
    #[serde(default)]
    #[garde(length(max = 10000))]
    pub body: Option<String>,
    /// When a task is due (RFC 3339).
    #[serde(default)]
    #[garde(skip)]
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
