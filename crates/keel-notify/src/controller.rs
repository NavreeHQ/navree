//! `{api_prefix}/notifications`: the signed-in user's notifications.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Path, Query};
use axum::http::Method;
use axum::http::request::Parts;
use keel_db::{Page, PageQuery, Uow};
use keel_http::{ApiMeta, AppError, Controller, CurrentPrincipal, Json, NoContent, RouteDef};
use keel_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::schema::{notification_page_schema, unread_count_schema};
use crate::{DatabaseNotification, Notifiable, Notifier, USER_NOTIFIABLE_TYPE};

/// Serves `GET /notifications`, `GET /notifications/unread-count`,
/// `POST /notifications/{id}/read` and `POST /notifications/read-all` for
/// the signed-in user (notifiable type `user`), when
/// [`NotifyOptions::routes`](crate::NotifyOptions::routes) is on.
pub(crate) struct NotificationsController {
    notifier: Arc<Notifier>,
}

impl Injectable for NotificationsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Notifier>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(NotificationsController {
                notifier: r.get::<Notifier>().await?,
            })
        })
    }
}

impl Controller for NotificationsController {
    fn prefix(&self) -> &'static str {
        "/notifications"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.notifier.options().routes {
            return Vec::new();
        }
        let (index, count, read, read_all) = (self.clone(), self.clone(), self.clone(), self);
        vec![
            RouteDef::on(
                Method::GET,
                "/",
                move |CurrentPrincipal(me): CurrentPrincipal,
                      uow: Uow,
                      UnreadFilter(unread): UnreadFilter,
                      page: PageQuery| async move {
                    index.index(SignedIn(me.user_id), uow, unread, page).await
                },
            )
            .authenticated()
            .name("notifications.index")
            .api(api(
                "List the signed-in user's notifications, newest first",
                &[401, 422],
                Some(notification_page_schema),
            )),
            RouteDef::on(
                Method::GET,
                "/unread-count",
                move |CurrentPrincipal(me): CurrentPrincipal, uow: Uow| async move {
                    count.unread_count(SignedIn(me.user_id), uow).await
                },
            )
            .authenticated()
            .name("notifications.unread_count")
            .api(api(
                "Count the signed-in user's unread notifications",
                &[401],
                Some(unread_count_schema),
            )),
            RouteDef::on(
                Method::POST,
                "/{id}/read",
                move |CurrentPrincipal(me): CurrentPrincipal,
                      uow: Uow,
                      Path(id): Path<String>| async move {
                    read.mark_read(SignedIn(me.user_id), uow, &id).await
                },
            )
            .authenticated()
            .name("notifications.read")
            .api(ApiMeta {
                success_status: 204,
                ..api("Mark a notification read", &[401, 404], None)
            }),
            RouteDef::on(
                Method::POST,
                "/read-all",
                move |CurrentPrincipal(me): CurrentPrincipal, uow: Uow| async move {
                    read_all.mark_all_read(SignedIn(me.user_id), uow).await
                },
            )
            .authenticated()
            .name("notifications.read_all")
            .api(ApiMeta {
                success_status: 204,
                ..api("Mark every notification read", &[401], None)
            }),
        ]
    }
}

impl NotificationsController {
    async fn index(
        &self,
        me: SignedIn,
        uow: Uow,
        unread: bool,
        page: PageQuery,
    ) -> Result<Page<DatabaseNotification>, AppError> {
        Ok(self
            .notifier
            .notifications(uow.db().conn(), &me, unread, page)
            .await?)
    }

    async fn unread_count(&self, me: SignedIn, uow: Uow) -> Result<Json<Value>, AppError> {
        let count = self.notifier.unread_count(uow.db().conn(), &me).await?;
        Ok(Json(json!({ "count": count })))
    }

    async fn mark_read(&self, me: SignedIn, uow: Uow, id: &str) -> Result<NoContent, AppError> {
        let id = Uuid::parse_str(id).map_err(|_| AppError::not_found())?;
        let tx = uow.tx().await?;
        if self.notifier.mark_as_read(&*tx, &me, id).await? {
            Ok(NoContent)
        } else {
            Err(AppError::not_found())
        }
    }

    async fn mark_all_read(&self, me: SignedIn, uow: Uow) -> Result<NoContent, AppError> {
        let tx = uow.tx().await?;
        self.notifier.mark_all_as_read(&*tx, &me).await?;
        Ok(NoContent)
    }
}

/// The signed-in user as a notifiable (type `user`, id the user's UUID).
struct SignedIn(Uuid);

impl Notifiable for SignedIn {
    fn notifiable_type(&self) -> &'static str {
        USER_NOTIFIABLE_TYPE
    }

    fn notifiable_id(&self) -> String {
        self.0.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        None
    }
}

/// `?unread=true|1` keeps unread notifications only; `false`, `0` or no
/// value keeps all. Anything else is a 422.
struct UnreadFilter(bool);

#[derive(Deserialize)]
struct RawUnread {
    unread: Option<String>,
}

impl<S: Send + Sync> FromRequestParts<S> for UnreadFilter {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Query(raw) = Query::<RawUnread>::try_from_uri(&parts.uri)
            .map_err(|rejection| AppError::bad_request(rejection.body_text()))?;
        match raw.unread.as_deref().map(str::trim) {
            None | Some("" | "false" | "0") => Ok(UnreadFilter(false)),
            Some("true" | "1") => Ok(UnreadFilter(true)),
            Some(_) => Err(AppError::validation(BTreeMap::from([(
                "unread".to_owned(),
                vec!["The unread field must be true or false.".to_owned()],
            )]))),
        }
    }
}

/// The OpenAPI metadata shared by the routes.
fn api(summary: &str, responses: &[u16], response: Option<keel_http::SchemaFn>) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Notifications".to_owned()],
        responses: responses.to_vec(),
        response,
        ..ApiMeta::default()
    }
}
