//! [`Invitations`]: invite by email, accept by token, revoke, expire.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use navree_crypto::{random_token, sha256_hex};
use navree_db::Uow;
use navree_mail::{Mailable, Message, TemplateMail};
use navree_orm::{Model as _, Record};
use serde_json::json;
use uuid::Uuid;

use crate::core::Core;
use crate::entity::{Invitation, Member, Workspace};
use crate::events::{InvitationAccepted, InvitationSent};
use crate::members::Members;
use crate::store;
use crate::{InvitationOptions, WorkspaceError, WorkspaceRole};

/// The length of invitation tokens (URL-safe characters).
pub const TOKEN_LEN: usize = 48;

/// A new invitation and its token. The token is stored only as a SHA-256
/// digest: this is the one chance to deliver it (the module mails it when
/// `MailModule` is in the app).
#[derive(Clone, PartialEq, Eq)]
pub struct SentInvitation {
    /// The invitation.
    pub invitation: Invitation,
    /// The plain token.
    pub token: String,
}

/// The token is a credential: never in debug output.
impl fmt::Debug for SentInvitation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SentInvitation")
            .field("invitation", &self.invitation)
            .field("token", &"[redacted]")
            .finish()
    }
}

/// What accepting an invitation gives: the workspace and the new
/// membership.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedInvitation {
    /// The workspace joined.
    pub workspace: Workspace,
    /// The new membership.
    pub member: Member,
}

/// Invites people by email, accepts invitations by token, revokes and
/// expires them. Provided and exported by the workspace module.
///
/// Tokens are random, 48 characters long and stored as SHA-256 digests.
/// An invitation lasts `workspace.invitations.ttl_secs` (seven days by
/// default); inviting the same address again replaces its pending
/// invitation. With `workspace.invitations.require_matching_email` (the
/// default), only the account with the invited address may accept, as the
/// [`UserDirectory`](crate::UserDirectory) reports it.
pub struct Invitations {
    core: Arc<Core>,
    members: Arc<Members>,
}

impl Invitations {
    pub(crate) fn new(core: Arc<Core>, members: Arc<Members>) -> Self {
        Self { core, members }
    }

    /// The invitation settings.
    pub fn options(&self) -> &InvitationOptions {
        &self.core.config.invitations
    }

    /// Invites `email` to `workspace` with `role` (any built-in or
    /// configured role except `owner`). The mail goes out once `uow`
    /// commits, through the app's `Mailer` when there is one, and
    /// `InvitationSent` is emitted. A 409 when the address belongs to a
    /// member already; a 400 on `email` or `role` for bad input.
    pub async fn invite(
        &self,
        uow: &Uow,
        workspace: Uuid,
        email: &str,
        role: WorkspaceRole,
        invited_by: Option<Uuid>,
    ) -> Result<SentInvitation, WorkspaceError> {
        let email = normalize_email(email)?;
        if role == WorkspaceRole::Owner {
            return Err(WorkspaceError::invalid(
                "role",
                "role must not be owner: invite as admin, then transfer ownership",
            ));
        }
        self.core.check_role(&role)?;
        let target = self.core.workspace_in(uow, workspace).await?;
        if let Some(user) = self
            .core
            .directory
            .user_by_email(&email)
            .await
            .map_err(WorkspaceError::Other)?
            && self.members.find(workspace, user.id).await?.is_some()
        {
            return Err(WorkspaceError::AlreadyMember);
        }
        // A new invitation replaces the address's pending one.
        let pending = Invitation::query()
            .where_(Invitation::WORKSPACE_ID.eq(workspace))
            .where_(Invitation::EMAIL.eq(email.as_str()))
            .where_null(Invitation::ACCEPTED_AT)
            .get(uow)
            .await?
            .into_models();
        for old in pending {
            store::delete(uow, old).await?;
        }
        let now = self.core.now();
        let token = random_token(TOKEN_LEN);
        let options = &self.core.config.invitations;
        let created = store::insert(
            uow,
            Invitation {
                id: Uuid::now_v7(),
                workspace_id: workspace,
                email: email.clone(),
                role: role.as_str().to_owned(),
                token_hash: sha256_hex(token.as_bytes()),
                invited_by,
                expires_at: after(now, options.ttl_secs),
                accepted_at: None,
                accepted_by: None,
                created_at: now,
            },
        )
        .await?;
        match self.core.mailer().await {
            Some(mailer) => {
                let inviter = match invited_by {
                    Some(id) => self
                        .core
                        .directory
                        .users(&[id])
                        .await
                        .map_err(WorkspaceError::Other)?
                        .into_iter()
                        .next()
                        .map(|user| user.name),
                    None => None,
                };
                let mail = invitation_mail(&target, &created, &token, inviter.as_deref(), options);
                self.core.mail_after_commit(uow, mailer, mail);
            }
            None => tracing::warn!(
                target: "navree::workspace",
                "no Mailer in the app (MailModule): the invitation was not mailed"
            ),
        }
        self.core
            .emit_after_commit(
                uow,
                InvitationSent {
                    workspace_id: workspace,
                    invitation_id: created.id,
                    email,
                    role: created.role.clone(),
                    invited_by,
                    expires_at: created.expires_at,
                },
            )
            .await;
        Ok(SentInvitation {
            invitation: created,
            token,
        })
    }

    /// Accepts the invitation with `token` for `user`, who joins its
    /// workspace with its role. A 404 for an unknown, revoked or used
    /// token, 410 when it has expired, 403 when it was sent to another
    /// address, 409 when `user` is a member already.
    pub async fn accept(
        &self,
        uow: &Uow,
        token: &str,
        user: Uuid,
    ) -> Result<AcceptedInvitation, WorkspaceError> {
        let found = Invitation::query()
            .where_(Invitation::TOKEN_HASH.eq(sha256_hex(token.trim().as_bytes())))
            .first(uow)
            .await?
            .map(Record::into_inner)
            .filter(|invitation| invitation.accepted_at.is_none())
            .ok_or(WorkspaceError::InvitationNotFound)?;
        let now = self.core.now();
        if found.expires_at <= now {
            return Err(WorkspaceError::InvitationExpired);
        }
        let workspace = self.core.workspace_in(uow, found.workspace_id).await?;
        if self.core.config.invitations.require_matching_email {
            let accepting = self
                .core
                .directory
                .users(&[user])
                .await
                .map_err(WorkspaceError::Other)?
                .into_iter()
                .next();
            match accepting {
                Some(account) if account.email.trim().eq_ignore_ascii_case(&found.email) => {}
                Some(_) => return Err(WorkspaceError::InvitationEmailMismatch),
                None => {
                    tracing::warn!(
                        target: "navree::workspace",
                        "the user directory does not know the accepting user, so the invitation's \
                         address cannot be checked; set workspace.users_table, register a \
                         UserDirectory, or turn off workspace.invitations.require_matching_email"
                    );
                    return Err(WorkspaceError::InvitationEmailMismatch);
                }
            }
        }
        if self
            .core
            .member_in(uow, workspace.id, user)
            .await?
            .is_some()
        {
            return Err(WorkspaceError::AlreadyMember);
        }
        let role = found.role();
        let member = self.members.insert(uow, workspace.id, user, role).await?;
        let invitation_id = found.id;
        let accepted = store::update(uow, found, |row| {
            row.accepted_at = Some(now);
            row.accepted_by = Some(user);
        })
        .await?;
        self.core
            .emit_after_commit(
                uow,
                InvitationAccepted {
                    workspace_id: workspace.id,
                    invitation_id,
                    user_id: user,
                    role: accepted.role,
                },
            )
            .await;
        Ok(AcceptedInvitation { workspace, member })
    }

    /// The pending invitation with `token` (not accepted, not expired), to
    /// show before accepting.
    pub async fn find_by_token(&self, token: &str) -> Result<Option<Invitation>, WorkspaceError> {
        let now = self.core.now();
        Ok(Invitation::query()
            .where_(Invitation::TOKEN_HASH.eq(sha256_hex(token.trim().as_bytes())))
            .first(&*self.core.db)
            .await?
            .map(Record::into_inner)
            .filter(|invitation| invitation.is_pending(now)))
    }

    /// The pending invitations of `workspace`, newest first.
    pub async fn pending(&self, workspace: Uuid) -> Result<Vec<Invitation>, WorkspaceError> {
        Ok(Invitation::query()
            .where_(Invitation::WORKSPACE_ID.eq(workspace))
            .where_null(Invitation::ACCEPTED_AT)
            .where_(Invitation::EXPIRES_AT.gt(self.core.now()))
            .order_by_desc(Invitation::CREATED_AT)
            .order_by_desc(Invitation::ID)
            .get(&*self.core.db)
            .await?
            .into_models())
    }

    /// Revokes the pending invitation `id` of `workspace` (its token stops
    /// working). [`WorkspaceError::InvitationNotFound`] if there is none.
    pub async fn revoke(&self, uow: &Uow, workspace: Uuid, id: Uuid) -> Result<(), WorkspaceError> {
        let found = Invitation::query()
            .where_key(id)
            .where_(Invitation::WORKSPACE_ID.eq(workspace))
            .where_null(Invitation::ACCEPTED_AT)
            .first(uow)
            .await?
            .ok_or(WorkspaceError::InvitationNotFound)?
            .into_inner();
        store::delete(uow, found).await
    }

    /// Deletes every invitation that expired without being accepted;
    /// returns how many (`workspace:prune-invitations`).
    pub async fn prune_expired(&self, uow: &Uow) -> Result<u64, WorkspaceError> {
        Ok(Invitation::query()
            .where_null(Invitation::ACCEPTED_AT)
            .where_(Invitation::EXPIRES_AT.lte(self.core.now()))
            .delete(uow)
            .await?)
    }
}

impl fmt::Debug for Invitations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Invitations").finish_non_exhaustive()
    }
}

/// A trimmed, lower-cased, checked email address.
fn normalize_email(email: &str) -> Result<String, WorkspaceError> {
    let email = email.trim().to_lowercase();
    let valid = email.len() <= 254
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.is_empty()
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && domain.contains('.')
                && !email.contains(char::is_whitespace)
                && email.matches('@').count() == 1
        });
    if valid {
        Ok(email)
    } else {
        Err(WorkspaceError::invalid(
            "email",
            "email must be a valid email address",
        ))
    }
}

/// `at` plus `secs` seconds (saturating).
fn after(at: DateTime<Utc>, secs: u64) -> DateTime<Utc> {
    let secs = i64::try_from(secs).unwrap_or(i64::MAX);
    at.checked_add_signed(TimeDelta::try_seconds(secs).unwrap_or(TimeDelta::MAX))
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

/// The invitation mail: from the configured template, or built-in text.
fn invitation_mail(
    workspace: &Workspace,
    invitation: &Invitation,
    token: &str,
    inviter: Option<&str>,
    options: &InvitationOptions,
) -> Box<dyn Mailable> {
    let days = options.ttl_secs.div_ceil(86_400);
    let url = options.url.as_deref().map(|url| with_token(url, token));
    let subject = format!("You're invited to join {}", workspace.name);
    if let Some(template) = &options.mail_template {
        return Box::new(
            TemplateMail::new(invitation.email.clone(), subject, template.clone()).context(json!({
                "workspace": workspace.name,
                "email": invitation.email,
                "role": invitation.role,
                "inviter": inviter.unwrap_or_default(),
                "token": token,
                "url": url.clone().unwrap_or_default(),
                "expires_in_days": days,
            })),
        );
    }
    let who = match inviter {
        Some(name) => format!("{name} invited you"),
        None => "You are invited".to_owned(),
    };
    let intro = format!(
        "{who} to join {workspace} as {role}.",
        workspace = workspace.name,
        role = invitation.role
    );
    let expiry = format!(
        "The invitation expires in {days} day{}.",
        if days == 1 { "" } else { "s" }
    );
    let outro = "If you did not expect this invitation, you can ignore this email.";
    let (text_action, html_action) = match &url {
        Some(url) => (
            format!("Accept the invitation: {url}"),
            format!(
                "<p><a href=\"{}\">Accept the invitation</a></p>",
                escape_html(url)
            ),
        ),
        None => (
            format!("Your invitation token: {token}"),
            format!(
                "<p>Your invitation token: <code>{}</code></p>",
                escape_html(token)
            ),
        ),
    };
    Box::new(
        Message::new()
            .to(invitation.email.clone())
            .subject(subject)
            .text(format!(
                "Hello,\n\n{intro}\n\n{text_action}\n\n{expiry}\n\n{outro}\n"
            ))
            .html(format!(
                "<p>Hello,</p>\n<p>{}</p>\n{html_action}\n<p>{expiry}</p>\n<p>{outro}</p>\n",
                escape_html(&intro)
            )),
    )
}

/// `url` with `token=<token>` appended to its query string.
fn with_token(url: &str, token: &str) -> String {
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("token", token)
        .finish();
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{query}")
}

/// `text` with the HTML special characters escaped.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_normalized_and_checked() {
        assert_eq!(
            normalize_email("  Anu@Example.COM ").unwrap(),
            "anu@example.com"
        );
        for bad in [
            "",
            "anu",
            "anu@",
            "@example.com",
            "anu@example",
            "a b@x.com",
            "a@b@c.com",
        ] {
            assert!(normalize_email(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn links_carry_the_token() {
        assert_eq!(
            with_token("https://app.test/accept", "a b"),
            "https://app.test/accept?token=a+b"
        );
        assert_eq!(
            with_token("https://app.test/accept?x=1", "t"),
            "https://app.test/accept?x=1&token=t"
        );
    }

    #[test]
    fn sent_invitations_hide_the_token_in_debug_output() {
        let now = Utc::now();
        let sent = SentInvitation {
            invitation: Invitation {
                id: Uuid::now_v7(),
                workspace_id: Uuid::now_v7(),
                email: "anu@example.com".into(),
                role: "member".into(),
                token_hash: "x".into(),
                invited_by: None,
                expires_at: now,
                accepted_at: None,
                accepted_by: None,
                created_at: now,
            },
            token: "s3cret-token".into(),
        };
        assert!(!format!("{sent:?}").contains("s3cret-token"));
    }
}
