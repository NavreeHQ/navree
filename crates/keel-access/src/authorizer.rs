//! [`AccessAuthorizer`]: keel-http's `RouteAuthorizer` for roles,
//! permissions and `can` declarations.

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use axum::extract::rejection::PathRejection;
use axum::extract::{FromRequestParts, Path};
use http::header::USER_AGENT;
use http::request::Parts;
use keel_contracts::{AuditEntry, AuditKind, AuditSink, RequestId};
use keel_db::Uow;
use keel_http::{AppError, ClientIp, RouteAuthorizer, RouteMeta};
use keel_kernel::{AppContext, BoxFuture};
use serde_json::{Value, json};

use crate::engine::Ability;
use crate::extract::request_access;
use crate::factory::{AbilityFactory, PrincipalAccess};

/// The record a `can(.., from = "id")` route loaded and authorized, kept in
/// the request extensions for [`Authorized`](crate::Authorized).
#[derive(Clone)]
pub(crate) struct AuthorizedRecord {
    pub(crate) subject: String,
    pub(crate) action: String,
    pub(crate) json: Arc<Value>,
    pub(crate) model: Arc<dyn Any + Send + Sync>,
    pub(crate) ability: Arc<Ability>,
}

/// Checks a route's access declaration, in order:
///
/// 1. **roles:** the user has any of them (the super-admin role passes);
/// 2. **permissions:** for each, an unconditional rule grants it
///    ([`Ability::has_unconditional`]);
/// 3. **can:** the type-level check passes; with `from`, the record whose id
///    is that path parameter is loaded in the request's unit of work (`FOR
///    UPDATE` where supported) and checked with its conditions. A missing
///    record is a 404; the record is kept for `Authorized<E, A>`.
///
/// Any failure is a 403. Registered as the app's `dyn RouteAuthorizer` by
/// the access module.
pub struct AccessAuthorizer {
    factory: Arc<AbilityFactory>,
}

impl AccessAuthorizer {
    pub(crate) fn new(factory: Arc<AbilityFactory>) -> Self {
        Self { factory }
    }

    async fn check(&self, parts: &mut Parts, meta: &RouteMeta) -> Result<(), AppError> {
        let access = request_access(parts, Some(&self.factory)).await?;
        let spec = &meta.access;
        if !spec.roles.is_empty()
            && !access.super_admin
            && !spec.roles.iter().any(|role| access.has_role(role))
        {
            let denial = Denial::new(json!({ "check": "roles", "roles": spec.roles }));
            return Err(self.deny(parts, meta, &access, denial).await);
        }
        for permission in &spec.permissions {
            let (action, subject) = self.factory.subjects().permission_target(permission);
            if !access.ability.has_unconditional(action, subject) {
                let denial =
                    Denial::new(json!({ "check": "permissions", "permission": permission }));
                return Err(self.deny(parts, meta, &access, denial).await);
            }
        }
        if let Some(can) = &spec.can {
            if !access.ability.can(&can.action, &can.subject, None) {
                let denial = Denial::can(&can.action, &can.subject, None);
                return Err(self.deny(parts, meta, &access, denial).await);
            }
            if let Some(param) = &can.from {
                self.check_record(parts, meta, &access, &can.action, &can.subject, param)
                    .await?;
            }
        }
        Ok(())
    }

    async fn check_record(
        &self,
        parts: &mut Parts,
        meta: &RouteMeta,
        access: &PrincipalAccess,
        action: &str,
        subject: &str,
        param: &str,
    ) -> Result<(), AppError> {
        let misdeclared = || {
            AppError::internal(format!(
                "route `{} {}` checks `{action} {subject}` from the path parameter `{param}`, which it does not have",
                meta.method, meta.full_path
            ))
        };
        let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, &())
            .await
            .map_err(|rejection| match rejection {
                PathRejection::MissingPathParams(_) => misdeclared(),
                // A parameter that is not valid percent-encoded UTF-8 names
                // no record.
                _ => AppError::not_found(),
            })?;
        let id = params.get(param).ok_or_else(misdeclared)?;
        let loader = self.factory.subjects().loader(subject).ok_or_else(|| {
            AppError::internal(format!(
                "no record loader for subject `{subject}`: import `AccessModule::for_feature::<E>()` for its entity"
            ))
        })?;
        let uow = parts.extensions.get::<Uow>().cloned().ok_or_else(|| {
            AppError::internal(
                "no unit of work in the request: add DatabaseModule to the app to check records",
            )
        })?;
        let record = loader
            .load(&uow, id)
            .await
            .map_err(AppError::internal)?
            .ok_or_else(AppError::not_found)?;
        if !access.ability.can(action, subject, Some(&record.json)) {
            let denial = Denial::can(action, subject, Some(id));
            return Err(self.deny(parts, meta, access, denial).await);
        }
        parts.extensions.insert(AuthorizedRecord {
            subject: subject.to_owned(),
            action: action.to_owned(),
            json: Arc::new(record.json),
            model: record.model,
            ability: access.ability.clone(),
        });
        Ok(())
    }

    /// Logs the denial, writes an `access.denied` security audit entry
    /// (when an `AuditSink` is registered; it survives the request's
    /// rollback) and returns the 403.
    async fn deny(
        &self,
        parts: &Parts,
        meta: &RouteMeta,
        access: &PrincipalAccess,
        denial: Denial,
    ) -> AppError {
        tracing::warn!(
            target: "keel::security",
            method = %meta.method,
            route = %meta.full_path,
            user_id = access.user.map(tracing::field::display),
            check = %denial.metadata["check"],
            "access denied"
        );
        let sink = match parts.extensions.get::<AppContext>() {
            Some(ctx) => ctx.try_get::<dyn AuditSink>().await,
            None => None,
        };
        if let Some(sink) = sink {
            // A 403 rolls the request's unit of work back anyway; doing it
            // now releases the locks a record check took (on SQLite, the
            // database write lock), which an audit sink writing on its own
            // connection would otherwise wait for.
            if let Some(uow) = parts.extensions.get::<Uow>()
                && let Err(error) = uow.rollback().await
            {
                tracing::warn!(target: "keel::audit", %error, "rolling back before the denial audit entry failed");
            }
            let entry = denial.entry(parts, meta, access);
            if let Err(error) = sink.record(entry).await {
                tracing::warn!(target: "keel::audit", %error, "could not write the access denial audit entry");
            }
        }
        AppError::forbidden()
    }
}

/// Why a request was denied.
struct Denial {
    metadata: Value,
    subject: Option<(String, String)>,
}

impl Denial {
    fn new(metadata: Value) -> Self {
        Self {
            metadata,
            subject: None,
        }
    }

    fn can(action: &str, subject: &str, id: Option<&str>) -> Self {
        Self {
            metadata: json!({ "check": "can", "action": action, "subject": subject }),
            subject: id.map(|id| (subject.to_owned(), id.to_owned())),
        }
    }

    fn entry(self, parts: &Parts, meta: &RouteMeta, access: &PrincipalAccess) -> AuditEntry {
        let mut metadata = self.metadata;
        if let Some(object) = metadata.as_object_mut() {
            object.insert("method".to_owned(), json!(meta.method));
            object.insert("route".to_owned(), json!(meta.full_path));
        }
        let mut entry = AuditEntry::new(AuditKind::Security, "access.denied").metadata(metadata);
        if let Some(user) = access.user {
            entry = entry.actor(user);
        }
        if let Some((subject, id)) = self.subject {
            entry = entry.subject(subject, id);
        }
        if let Some(request_id) = parts.extensions.get::<RequestId>() {
            entry = entry.request_id(request_id.as_str());
        }
        if let Some(ClientIp(ip)) = parts.extensions.get::<ClientIp>() {
            entry = entry.ip(ip.to_string());
        }
        if let Some(agent) = parts.headers.get(USER_AGENT).and_then(|v| v.to_str().ok()) {
            entry = entry.user_agent(agent);
        }
        entry
    }
}

impl RouteAuthorizer for AccessAuthorizer {
    fn authorize<'a>(
        &'a self,
        parts: &'a mut Parts,
        meta: &'a RouteMeta,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(self.check(parts, meta))
    }
}

impl fmt::Debug for AccessAuthorizer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessAuthorizer").finish_non_exhaustive()
    }
}
