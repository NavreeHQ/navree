//! Authorization requests (`GET /oauth/authorize`) and the user's
//! decisions on them, plus the device codes users approve, and the
//! remembered approvals (`oauth_authorizations`).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_contracts::{AuditEntry, AuditKind};
use navree_db::Uow;
use navree_workspace::Members;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, Set,
};
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::clients::{GRANT_AUTHORIZATION_CODE, GRANT_DEVICE_CODE};
use crate::core::Core;
use crate::entity::{
    Authorization, Client, access_token, authorization, authorization_request, client, code,
    device_code, refresh_token,
};
use crate::error::OAuthError;
use crate::scope::{ScopeInfo, is_subset, join_scopes, parse_scope};
use crate::token::{
    CODE_PREFIX, DEVICE_CODE_PREFIX, digest, display_user_code, is_pkce_value, new_token,
    new_user_code, normalize_user_code, redirect_uri_matches, with_query,
};

/// The polling interval the device grant starts with, in seconds.
pub const DEVICE_INTERVAL_SECS: u64 = 5;

/// A client as the consent page shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ClientSummary {
    /// The client id.
    pub id: Uuid,
    /// Its name.
    pub name: String,
    /// Its logo.
    pub logo_url: Option<String>,
    /// Its website.
    pub website: Option<String>,
}

impl From<&Client> for ClientSummary {
    fn from(client: &Client) -> Self {
        Self {
            id: client.id,
            name: client.name.clone(),
            logo_url: client.logo_url.clone(),
            website: client.website.clone(),
        }
    }
}

/// An authorization request as the consent page reads it
/// (`GET /api/oauth/requests/{id}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AuthorizationRequestView {
    /// The request id.
    pub id: Uuid,
    /// The client asking.
    pub client: ClientSummary,
    /// The scopes it asks for, with their descriptions.
    pub scopes: Vec<ScopeInfo>,
    /// Whether the user already approved these scopes for this client (or
    /// the client is trusted): the page may approve at once.
    pub already_approved: bool,
    /// When the request expires.
    pub expires_at: DateTime<Utc>,
}

/// A device authorization as the device page reads it
/// (`GET /api/oauth/device/{user_code}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct DeviceRequestView {
    /// The user code (`WDJB-MJHT`).
    pub user_code: String,
    /// The client asking.
    pub client: ClientSummary,
    /// The scopes it asks for.
    pub scopes: Vec<ScopeInfo>,
    /// Whether the user already approved these scopes for this client.
    pub already_approved: bool,
    /// When the code expires.
    pub expires_at: DateTime<Utc>,
}

/// Where the browser goes after a decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Decision {
    /// The client's redirect URI with `code` and `state` (or `error`).
    pub redirect_to: String,
}

/// The device authorization response (RFC 8628 §3.2).
#[derive(Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DeviceAuthorization {
    /// The code the device polls with (`nvd_...`).
    pub device_code: String,
    /// The code the user types (`WDJB-MJHT`).
    pub user_code: String,
    /// The page where the user types it (`oauth.device_url`).
    pub verification_uri: String,
    /// The same page with the code filled in.
    pub verification_uri_complete: String,
    /// Seconds until the codes expire.
    pub expires_in: u64,
    /// Seconds between polls.
    pub interval: u64,
}

impl fmt::Debug for DeviceAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceAuthorization")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

/// A remembered approval as `GET /api/oauth/authorizations` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AuthorizationView {
    /// The client.
    pub client: ClientSummary,
    /// The approved scopes.
    pub scopes: Vec<ScopeInfo>,
    /// When the user first approved it.
    pub created_at: DateTime<Utc>,
    /// When the approval last changed.
    pub updated_at: DateTime<Utc>,
}

/// The parameters of `GET /oauth/authorize`.
#[derive(Clone, Debug, Default)]
pub(crate) struct AuthorizeParams {
    pub(crate) response_type: Option<String>,
    pub(crate) client_id: Option<String>,
    pub(crate) redirect_uri: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) state: Option<String>,
    pub(crate) code_challenge: Option<String>,
    pub(crate) code_challenge_method: Option<String>,
    pub(crate) nonce: Option<String>,
}

impl AuthorizeParams {
    pub(crate) fn from_query(query: &str) -> Self {
        let mut params: HashMap<String, String> = HashMap::new();
        for (name, value) in form_urlencoded::parse(query.as_bytes()) {
            params
                .entry(name.into_owned())
                .or_insert_with(|| value.into_owned());
        }
        let mut take = |name: &str| params.remove(name).filter(|v| !v.is_empty());
        Self {
            response_type: take("response_type"),
            client_id: take("client_id"),
            redirect_uri: take("redirect_uri"),
            scope: take("scope"),
            state: take("state"),
            code_challenge: take("code_challenge"),
            code_challenge_method: take("code_challenge_method"),
            nonce: take("nonce"),
        }
    }
}

/// What `GET /oauth/authorize` does.
#[derive(Debug)]
pub(crate) enum AuthorizeOutcome {
    /// Redirect the browser (to the consent page, or back to the client
    /// with an error).
    Redirect(String),
    /// The request can't be trusted to redirect: show an error page.
    ErrorPage(String),
}

/// Authorization requests, consent and device decisions.
pub(crate) struct Requests {
    pub(crate) core: Arc<Core>,
}

impl Requests {
    /// Checks a `GET /oauth/authorize` request and stores it: an unknown
    /// client or a redirect URI that isn't registered gives an error page;
    /// other problems go back to the client as `error` parameters.
    pub(crate) async fn authorize(
        &self,
        uow: &Uow,
        params: AuthorizeParams,
    ) -> Result<AuthorizeOutcome, OAuthError> {
        let core = &self.core;
        let Some(client) = self.client_named(params.client_id.as_deref()).await? else {
            return Ok(AuthorizeOutcome::ErrorPage(
                "The application asking for access is not registered (unknown client_id)."
                    .to_owned(),
            ));
        };
        let registered = client.redirect_uri_list();
        let redirect_uri = match &params.redirect_uri {
            Some(uri) => match registered.iter().find(|r| redirect_uri_matches(r, uri)) {
                Some(_) => uri.clone(),
                None => {
                    return Ok(AuthorizeOutcome::ErrorPage(
                        "The redirect_uri is not registered for this application.".to_owned(),
                    ));
                }
            },
            None if registered.len() == 1 => registered[0].clone(),
            None => {
                return Ok(AuthorizeOutcome::ErrorPage(
                    "The request must name its redirect_uri.".to_owned(),
                ));
            }
        };
        let state = params.state.clone();
        let fail = |error: &str, description: &str| {
            let mut query = vec![("error", error), ("error_description", description)];
            if let Some(state) = state.as_deref() {
                query.push(("state", state));
            }
            query.push(("iss", core.issuer.as_str()));
            Ok(AuthorizeOutcome::Redirect(with_query(
                &redirect_uri,
                &query,
            )))
        };
        if params.response_type.as_deref() != Some("code") {
            return fail(
                "unsupported_response_type",
                "Only response_type=code is supported",
            );
        }
        if !client.allows(GRANT_AUTHORIZATION_CODE) {
            return fail(
                "unauthorized_client",
                "This client may not use the authorization code grant",
            );
        }
        let Some(challenge) = params.code_challenge.as_deref() else {
            return fail("invalid_request", "PKCE is required: send code_challenge");
        };
        if params.code_challenge_method.as_deref() != Some("S256") {
            return fail("invalid_request", "code_challenge_method must be S256");
        }
        if !is_pkce_value(challenge) {
            return fail("invalid_request", "code_challenge is malformed");
        }
        let allowed = client.scope_list();
        let scopes = match params.scope.as_deref() {
            Some(scope) => parse_scope(scope),
            None => allowed.clone(),
        };
        if scopes.is_empty() {
            return fail("invalid_scope", "The request names no scope");
        }
        if core.scopes.unknown(&scopes).is_some() || !is_subset(&scopes, &allowed) {
            return fail(
                "invalid_scope",
                "The client may not ask for one of these scopes",
            );
        }
        let Some(consent_url) = core.options.consent_url.clone() else {
            tracing::error!(target: "navree::oauth", "oauth.consent_url is not set: the authorization code flow can't show a consent page");
            return fail(
                "temporarily_unavailable",
                "The server has no consent page configured",
            );
        };
        let id = Uuid::new_v4();
        let now = core.now();
        authorization_request::ActiveModel {
            id: Set(id),
            client_id: Set(client.id),
            redirect_uri: Set(redirect_uri),
            scopes: Set(join_scopes(&scopes)),
            state: Set(params.state),
            code_challenge: Set(challenge.to_owned()),
            nonce: Set(params.nonce),
            user_id: Set(None),
            status: Set("pending".to_owned()),
            expires_at: Set(core.after(core.options.request_ttl_secs)),
            created_at: Set(now),
        }
        .insert(&*uow.tx().await?)
        .await?;
        Ok(AuthorizeOutcome::Redirect(with_query(
            &consent_url,
            &[("request", &id.to_string())],
        )))
    }

    async fn client_named(&self, id: Option<&str>) -> Result<Option<Client>, OAuthError> {
        let Some(id) = id.and_then(|id| Uuid::parse_str(id).ok()) else {
            return Ok(None);
        };
        Ok(client::Entity::find_by_id(id)
            .one(&self.core.db.reader().await)
            .await?
            .filter(|client| !client.is_revoked()))
    }

    /// The pending request `id`: 404 when unknown or decided, 410 once
    /// expired.
    async fn pending<C: ConnectionTrait>(
        &self,
        conn: &C,
        id: Uuid,
    ) -> Result<(authorization_request::Model, Client), OAuthError> {
        let not_found = OAuthError::NotFound("Authorization request not found.");
        let request = authorization_request::Entity::find_by_id(id)
            .one(conn)
            .await?
            .filter(|request| request.status == "pending")
            .ok_or(not_found)?;
        if request.expires_at <= self.core.now() {
            return Err(OAuthError::Expired(
                "The authorization request has expired.",
            ));
        }
        let client = client::Entity::find_by_id(request.client_id)
            .one(conn)
            .await?
            .filter(|client| !client.is_revoked())
            .ok_or(OAuthError::NotFound("Authorization request not found."))?;
        Ok((request, client))
    }

    /// The request `id` for `user_id`'s consent page.
    pub(crate) async fn show(
        &self,
        user_id: Uuid,
        id: Uuid,
    ) -> Result<AuthorizationRequestView, OAuthError> {
        let conn = self.core.db.reader().await;
        let (request, client) = self.pending(&conn, id).await?;
        let scopes = parse_scope(&request.scopes);
        let already_approved =
            client.trusted || self.approved(&conn, user_id, &client, &scopes).await?;
        Ok(AuthorizationRequestView {
            id: request.id,
            client: ClientSummary::from(&client),
            scopes: self.core.scopes.describe(&scopes),
            already_approved,
            expires_at: request.expires_at,
        })
    }

    /// Whether `user_id` already approved `scopes` for `client`.
    async fn approved<C: ConnectionTrait>(
        &self,
        conn: &C,
        user_id: Uuid,
        client: &Client,
        scopes: &[String],
    ) -> Result<bool, OAuthError> {
        Ok(authorization::Entity::find()
            .filter(authorization::Column::UserId.eq(user_id))
            .filter(authorization::Column::ClientId.eq(client.id))
            .one(conn)
            .await?
            .is_some_and(|row| is_subset(scopes, &row.scope_list())))
    }

    /// `user_id` approves request `id`: a code for the client, the
    /// approval remembered, audited as `oauth.authorized`.
    pub(crate) async fn approve(
        &self,
        uow: &Uow,
        user_id: Uuid,
        id: Uuid,
    ) -> Result<Decision, OAuthError> {
        let tx = uow.tx().await?;
        let (request, client) = self.pending(&*tx, id).await?;
        drop(tx);
        self.check_workspace_member(&client, user_id).await?;
        let tx = uow.tx().await?;
        let now = self.core.now();
        let claimed = authorization_request::Entity::update_many()
            .col_expr(
                authorization_request::Column::Status,
                Expr::value("approved"),
            )
            .col_expr(authorization_request::Column::UserId, Expr::value(user_id))
            .filter(authorization_request::Column::Id.eq(id))
            .filter(authorization_request::Column::Status.eq("pending"))
            .exec(&*tx)
            .await?;
        if claimed.rows_affected != 1 {
            return Err(OAuthError::NotFound("Authorization request not found."));
        }
        let scopes = parse_scope(&request.scopes);
        let code = new_token(CODE_PREFIX);
        code::ActiveModel {
            id: Set(Uuid::now_v7()),
            code_hash: Set(digest(&code)),
            client_id: Set(client.id),
            user_id: Set(user_id),
            redirect_uri: Set(request.redirect_uri.clone()),
            scopes: Set(request.scopes.clone()),
            code_challenge: Set(request.code_challenge.clone()),
            nonce: Set(request.nonce.clone()),
            expires_at: Set(self.core.after(self.core.options.code_ttl_secs)),
            used_at: Set(None),
            created_at: Set(now),
        }
        .insert(&*tx)
        .await?;
        drop(tx);
        self.remember(uow, user_id, &client, &scopes).await?;
        self.audit_decision(
            uow,
            "oauth.authorized",
            user_id,
            &client,
            &scopes,
            "authorization_code",
        )
        .await;
        let mut query = vec![("code", code.as_str())];
        if let Some(state) = request.state.as_deref() {
            query.push(("state", state));
        }
        query.push(("iss", self.core.issuer.as_str()));
        Ok(Decision {
            redirect_to: with_query(&request.redirect_uri, &query),
        })
    }

    /// `user_id` denies request `id`, audited as `oauth.denied`.
    pub(crate) async fn deny(
        &self,
        uow: &Uow,
        user_id: Uuid,
        id: Uuid,
    ) -> Result<Decision, OAuthError> {
        let tx = uow.tx().await?;
        let (request, client) = self.pending(&*tx, id).await?;
        let claimed = authorization_request::Entity::update_many()
            .col_expr(authorization_request::Column::Status, Expr::value("denied"))
            .col_expr(authorization_request::Column::UserId, Expr::value(user_id))
            .filter(authorization_request::Column::Id.eq(id))
            .filter(authorization_request::Column::Status.eq("pending"))
            .exec(&*tx)
            .await?;
        drop(tx);
        if claimed.rows_affected != 1 {
            return Err(OAuthError::NotFound("Authorization request not found."));
        }
        let scopes = parse_scope(&request.scopes);
        self.audit_decision(
            uow,
            "oauth.denied",
            user_id,
            &client,
            &scopes,
            "authorization_code",
        )
        .await;
        let mut query = vec![
            ("error", "access_denied"),
            ("error_description", "The user denied the request"),
        ];
        if let Some(state) = request.state.as_deref() {
            query.push(("state", state));
        }
        query.push(("iss", self.core.issuer.as_str()));
        Ok(Decision {
            redirect_to: with_query(&request.redirect_uri, &query),
        })
    }

    /// A workspace-limited client's tokens only work in its workspace, so
    /// only its members may approve it.
    async fn check_workspace_member(
        &self,
        client: &Client,
        user_id: Uuid,
    ) -> Result<(), OAuthError> {
        let Some(workspace) = client.workspace_id else {
            return Ok(());
        };
        let members = match self.core.app() {
            Some(app) => app.try_get::<Members>().await,
            None => None,
        };
        let Some(members) = members else {
            return Ok(());
        };
        let member = members
            .find(workspace, user_id)
            .await
            .map_err(|error| OAuthError::Other(error.into()))?;
        if member.is_none() {
            return Err(OAuthError::Forbidden(
                "This application is limited to a workspace you are not a member of.".into(),
            ));
        }
        Ok(())
    }

    /// Remembers that `user_id` approved `scopes` for `client` (adding to
    /// what they approved before).
    async fn remember(
        &self,
        uow: &Uow,
        user_id: Uuid,
        client: &Client,
        scopes: &[String],
    ) -> Result<(), OAuthError> {
        let tx = uow.tx().await?;
        let now = self.core.now();
        let existing = authorization::Entity::find()
            .filter(authorization::Column::UserId.eq(user_id))
            .filter(authorization::Column::ClientId.eq(client.id))
            .one(&*tx)
            .await?;
        match existing {
            Some(row) => {
                let mut merged = row.scope_list();
                for scope in scopes {
                    if !merged.contains(scope) {
                        merged.push(scope.clone());
                    }
                }
                let mut active: authorization::ActiveModel = row.into();
                active.scopes = Set(join_scopes(&merged));
                active.updated_at = Set(now);
                active.update(&*tx).await?;
            }
            None => {
                authorization::ActiveModel {
                    id: Set(Uuid::now_v7()),
                    user_id: Set(user_id),
                    client_id: Set(client.id),
                    scopes: Set(join_scopes(scopes)),
                    created_at: Set(now),
                    updated_at: Set(now),
                }
                .insert(&*tx)
                .await?;
            }
        }
        Ok(())
    }

    async fn audit_decision(
        &self,
        uow: &Uow,
        action: &str,
        user_id: Uuid,
        client: &Client,
        scopes: &[String],
        grant: &str,
    ) {
        let entry = AuditEntry::new(AuditKind::Auth, action)
            .actor(user_id)
            .subject("OAuthClient", client.id.to_string())
            .metadata(json!({
                "client_id": client.id,
                "client": client.name,
                "scopes": scopes,
                "grant": grant,
            }));
        let entry = match uow.request_id() {
            Some(request_id) => entry.request_id(request_id),
            None => entry,
        };
        self.core.audit_after_commit(uow, entry).await;
    }

    /// `POST /oauth/device_authorization` for `client`.
    pub(crate) async fn device_authorization(
        &self,
        uow: &Uow,
        client: &Client,
        scope: Option<&str>,
    ) -> Result<DeviceAuthorization, OAuthError> {
        if !client.allows(GRANT_DEVICE_CODE) {
            return Err(OAuthError::UnauthorizedClient(
                "This client may not use the device authorization grant".into(),
            ));
        }
        let core = &self.core;
        let verification_uri = core.options.device_url.clone().ok_or_else(|| {
            tracing::error!(target: "navree::oauth", "oauth.device_url is not set: the device grant has no page to send users to");
            OAuthError::Unavailable("The server has no device page configured".into())
        })?;
        let allowed = client.scope_list();
        let scopes = match scope {
            Some(scope) => parse_scope(scope),
            None => allowed.clone(),
        };
        if scopes.is_empty() || !is_subset(&scopes, &allowed) {
            return Err(OAuthError::InvalidScope(
                "The client may not ask for one of these scopes".into(),
            ));
        }
        let device_code = new_token(DEVICE_CODE_PREFIX);
        let tx = uow.tx().await?;
        // A clash with a pending code is astronomically rare; retry anyway.
        let mut user_code = new_user_code();
        for _ in 0..5 {
            let taken = device_code::Entity::find()
                .filter(device_code::Column::UserCode.eq(&user_code))
                .one(&*tx)
                .await?
                .is_some();
            if !taken {
                break;
            }
            user_code = new_user_code();
        }
        let ttl = core.options.request_ttl_secs;
        device_code::ActiveModel {
            id: Set(Uuid::now_v7()),
            device_code_hash: Set(digest(&device_code)),
            user_code: Set(user_code.clone()),
            client_id: Set(client.id),
            scopes: Set(join_scopes(&scopes)),
            user_id: Set(None),
            status: Set("pending".to_owned()),
            interval_secs: Set(DEVICE_INTERVAL_SECS as i64),
            last_polled_at: Set(None),
            expires_at: Set(core.after(ttl)),
            used_at: Set(None),
            created_at: Set(core.now()),
        }
        .insert(&*tx)
        .await?;
        let shown = display_user_code(&user_code);
        Ok(DeviceAuthorization {
            device_code,
            verification_uri_complete: with_query(&verification_uri, &[("user_code", &shown)]),
            verification_uri,
            user_code: shown,
            expires_in: ttl,
            interval: DEVICE_INTERVAL_SECS,
        })
    }

    /// The pending device authorization with `user_code`: 404 when
    /// unknown, decided or expired.
    async fn pending_device<C: ConnectionTrait>(
        &self,
        conn: &C,
        user_code: &str,
    ) -> Result<(device_code::Model, Client), OAuthError> {
        let not_found = || OAuthError::NotFound("Device code not found or expired.");
        let row = device_code::Entity::find()
            .filter(device_code::Column::UserCode.eq(normalize_user_code(user_code)))
            .one(conn)
            .await?
            .filter(|row| row.status == "pending" && row.expires_at > self.core.now())
            .ok_or_else(not_found)?;
        let client = client::Entity::find_by_id(row.client_id)
            .one(conn)
            .await?
            .filter(|client| !client.is_revoked())
            .ok_or_else(not_found)?;
        Ok((row, client))
    }

    /// The device authorization `user_code` for `user_id`'s device page.
    pub(crate) async fn show_device(
        &self,
        user_id: Uuid,
        user_code: &str,
    ) -> Result<DeviceRequestView, OAuthError> {
        let conn = self.core.db.reader().await;
        let (row, client) = self.pending_device(&conn, user_code).await?;
        let scopes = parse_scope(&row.scopes);
        let already_approved =
            client.trusted || self.approved(&conn, user_id, &client, &scopes).await?;
        Ok(DeviceRequestView {
            user_code: display_user_code(&row.user_code),
            client: ClientSummary::from(&client),
            scopes: self.core.scopes.describe(&scopes),
            already_approved,
            expires_at: row.expires_at,
        })
    }

    /// `user_id` approves (or denies) the device authorization
    /// `user_code`; the device's next poll gets the tokens (or
    /// `access_denied`).
    pub(crate) async fn decide_device(
        &self,
        uow: &Uow,
        user_id: Uuid,
        user_code: &str,
        approve: bool,
    ) -> Result<(), OAuthError> {
        let tx = uow.tx().await?;
        let (row, client) = self.pending_device(&*tx, user_code).await?;
        drop(tx);
        if approve {
            self.check_workspace_member(&client, user_id).await?;
        }
        let tx = uow.tx().await?;
        let status = if approve { "approved" } else { "denied" };
        let claimed = device_code::Entity::update_many()
            .col_expr(device_code::Column::Status, Expr::value(status))
            .col_expr(device_code::Column::UserId, Expr::value(user_id))
            .filter(device_code::Column::Id.eq(row.id))
            .filter(device_code::Column::Status.eq("pending"))
            .exec(&*tx)
            .await?;
        drop(tx);
        if claimed.rows_affected != 1 {
            return Err(OAuthError::NotFound("Device code not found or expired."));
        }
        let scopes = parse_scope(&row.scopes);
        if approve {
            self.remember(uow, user_id, &client, &scopes).await?;
        }
        let action = if approve {
            "oauth.authorized"
        } else {
            "oauth.denied"
        };
        self.audit_decision(uow, action, user_id, &client, &scopes, "device_code")
            .await;
        Ok(())
    }

    /// The clients `user_id` approved, newest first.
    pub(crate) async fn authorizations(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<AuthorizationView>, OAuthError> {
        let conn = self.core.db.reader().await;
        let rows: Vec<Authorization> = authorization::Entity::find()
            .filter(authorization::Column::UserId.eq(user_id))
            .order_by_desc(authorization::Column::UpdatedAt)
            .all(&conn)
            .await?;
        let mut views = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(client) = client::Entity::find_by_id(row.client_id)
                .one(&conn)
                .await?
                .filter(|client| !client.is_revoked())
            else {
                continue;
            };
            views.push(AuthorizationView {
                client: ClientSummary::from(&client),
                scopes: self.core.scopes.describe(&row.scope_list()),
                created_at: row.created_at,
                updated_at: row.updated_at,
            });
        }
        Ok(views)
    }

    /// `user_id` revokes `client_id`'s access: the approval is forgotten
    /// and every token the client holds for them is revoked. Audited as
    /// `oauth.revoked`; 404 when they never approved it.
    pub(crate) async fn revoke_authorization(
        &self,
        uow: &Uow,
        user_id: Uuid,
        client_id: Uuid,
    ) -> Result<(), OAuthError> {
        let tx = uow.tx().await?;
        let row = authorization::Entity::find()
            .filter(authorization::Column::UserId.eq(user_id))
            .filter(authorization::Column::ClientId.eq(client_id))
            .one(&*tx)
            .await?
            .ok_or(OAuthError::NotFound("Authorization not found."))?;
        let scopes = row.scope_list();
        authorization::Entity::delete_by_id(row.id)
            .exec(&*tx)
            .await?;
        let now = self.core.now();
        access_token::Entity::update_many()
            .col_expr(access_token::Column::RevokedAt, Expr::value(now))
            .filter(access_token::Column::UserId.eq(user_id))
            .filter(access_token::Column::ClientId.eq(client_id))
            .filter(access_token::Column::RevokedAt.is_null())
            .exec(&*tx)
            .await?;
        refresh_token::Entity::update_many()
            .col_expr(refresh_token::Column::RevokedAt, Expr::value(now))
            .filter(refresh_token::Column::UserId.eq(user_id))
            .filter(refresh_token::Column::ClientId.eq(client_id))
            .filter(refresh_token::Column::RevokedAt.is_null())
            .exec(&*tx)
            .await?;
        code::Entity::update_many()
            .col_expr(code::Column::UsedAt, Expr::value(now))
            .filter(code::Column::UserId.eq(user_id))
            .filter(code::Column::ClientId.eq(client_id))
            .filter(code::Column::UsedAt.is_null())
            .exec(&*tx)
            .await?;
        let client = client::Entity::find_by_id(client_id).one(&*tx).await?;
        drop(tx);
        let core = self.core.clone();
        uow.after_commit(move || {
            Box::pin(async move {
                core.forget_all();
            })
        });
        if let Some(client) = client {
            self.audit_decision(
                uow,
                "oauth.revoked",
                user_id,
                &client,
                &scopes,
                "authorization_code",
            )
            .await;
        }
        Ok(())
    }
}

impl fmt::Debug for Requests {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Requests").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorize_parameters_parse_from_the_query() {
        let params = AuthorizeParams::from_query(
            "response_type=code&client_id=abc&scope=deals%3Aread+openid&state=xyz&state=second&nonce=",
        );
        assert_eq!(params.response_type.as_deref(), Some("code"));
        assert_eq!(params.client_id.as_deref(), Some("abc"));
        assert_eq!(params.scope.as_deref(), Some("deals:read openid"));
        assert_eq!(params.state.as_deref(), Some("xyz"));
        assert_eq!(params.nonce, None);
        assert_eq!(params.redirect_uri, None);
    }
}
