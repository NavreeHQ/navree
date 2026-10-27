//! [`MailScope`], [`SendReport`], [`DeliveryStatus`] and [`TransportTier`].

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Who a message is sent for: the tenant whose transport, suppressions and
/// delivery log apply.
///
/// [`Mailer::for_scope`](crate::Mailer::for_scope) takes one; with
/// navree-workspace, `CurrentWorkspace::mail_scope()` builds it from the
/// request's workspace and its organization.
///
/// ```
/// use navree_mail::MailScope;
/// use uuid::Uuid;
///
/// let workspace = Uuid::now_v7();
/// let organization = Uuid::now_v7();
/// let scope = MailScope::workspace(workspace).with_organization(organization);
/// assert_eq!(scope.workspace_id, Some(workspace));
/// assert!(MailScope::platform().is_platform());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct MailScope {
    /// The workspace the mail is sent for.
    pub workspace_id: Option<Uuid>,
    /// The workspace's organization.
    pub organization_id: Option<Uuid>,
}

impl MailScope {
    /// The platform itself: no tenant.
    pub fn platform() -> Self {
        Self::default()
    }

    /// Mail sent for `workspace_id`.
    pub fn workspace(workspace_id: Uuid) -> Self {
        Self {
            workspace_id: Some(workspace_id),
            organization_id: None,
        }
    }

    /// Mail sent for `organization_id` (and none of its workspaces).
    pub fn organization(organization_id: Uuid) -> Self {
        Self {
            workspace_id: None,
            organization_id: Some(organization_id),
        }
    }

    /// This scope inside `organization_id`.
    #[must_use]
    pub fn with_organization(mut self, organization_id: Uuid) -> Self {
        self.organization_id = Some(organization_id);
        self
    }

    /// Whether this is the platform scope (no workspace, no organization).
    pub fn is_platform(&self) -> bool {
        self.workspace_id.is_none() && self.organization_id.is_none()
    }
}

/// Where a transport came from, most specific first: the tiers
/// [`TransportResolver`](crate::TransportResolver) tries in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransportTier {
    /// A transport for the sender's domain (`acme.test` in
    /// `hello@acme.test`).
    Domain,
    /// The workspace's transport.
    Workspace,
    /// The organization's transport.
    Organization,
    /// The platform's transport, stored like a tenant's.
    Platform,
    /// The configured transport (`mail.transport`, or a named one).
    Config,
}

impl TransportTier {
    /// The stored name: `domain`, `workspace`, `organization`, `platform` or
    /// `config`.
    pub fn as_str(self) -> &'static str {
        match self {
            TransportTier::Domain => "domain",
            TransportTier::Workspace => "workspace",
            TransportTier::Organization => "organization",
            TransportTier::Platform => "platform",
            TransportTier::Config => "config",
        }
    }
}

impl fmt::Display for TransportTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TransportTier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "domain" => TransportTier::Domain,
            "workspace" => TransportTier::Workspace,
            "organization" => TransportTier::Organization,
            "platform" => TransportTier::Platform,
            "config" => TransportTier::Config,
            other => return Err(format!("unknown transport tier `{other}`")),
        })
    }
}

/// The state of a delivery in the log (`mail_deliveries.status`), and what
/// a send came to in a [`SendReport`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    /// Handed to the transport, no answer yet.
    Sending,
    /// The transport accepted it.
    Sent,
    /// The provider reported it delivered.
    Delivered,
    /// The provider reported a bounce.
    Bounced,
    /// The recipient marked it as spam.
    Complained,
    /// The transport refused it.
    Failed,
    /// Every recipient is suppressed: nothing was sent.
    Suppressed,
    /// Waiting in the outbox for approval.
    Held,
    /// Discarded from the outbox.
    Discarded,
}

impl DeliveryStatus {
    /// The stored name (`sent`, `bounced`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryStatus::Sending => "sending",
            DeliveryStatus::Sent => "sent",
            DeliveryStatus::Delivered => "delivered",
            DeliveryStatus::Bounced => "bounced",
            DeliveryStatus::Complained => "complained",
            DeliveryStatus::Failed => "failed",
            DeliveryStatus::Suppressed => "suppressed",
            DeliveryStatus::Held => "held",
            DeliveryStatus::Discarded => "discarded",
        }
    }
}

impl fmt::Display for DeliveryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DeliveryStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "sending" => DeliveryStatus::Sending,
            "sent" => DeliveryStatus::Sent,
            "delivered" => DeliveryStatus::Delivered,
            "bounced" => DeliveryStatus::Bounced,
            "complained" => DeliveryStatus::Complained,
            "failed" => DeliveryStatus::Failed,
            "suppressed" => DeliveryStatus::Suppressed,
            "held" => DeliveryStatus::Held,
            "discarded" => DeliveryStatus::Discarded,
            other => return Err(format!("unknown delivery status `{other}`")),
        })
    }
}

/// What [`Mailer::deliver`](crate::Mailer::deliver) did with a message.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SendReport {
    /// `Sent`, `Held` (in the outbox) or `Suppressed` (every recipient is
    /// suppressed, nothing was sent).
    pub status: DeliveryStatus,
    /// The delivery log row (with `mail.delivery.enabled`).
    pub delivery_id: Option<Uuid>,
    /// The outbox row of a held message.
    pub outbox_id: Option<Uuid>,
    /// The transport that delivered it (`smtp`, `ses`, ...).
    pub transport: Option<&'static str>,
    /// Where that transport came from.
    pub tier: Option<TransportTier>,
    /// The provider's message id.
    pub provider_id: Option<String>,
    /// The recipients left out because they are suppressed.
    pub suppressed: Vec<String>,
}

impl SendReport {
    /// A report with `status` and nothing else.
    pub(crate) fn new(status: DeliveryStatus) -> Self {
        Self {
            status,
            delivery_id: None,
            outbox_id: None,
            transport: None,
            tier: None,
            provider_id: None,
            suppressed: Vec::new(),
        }
    }
}
