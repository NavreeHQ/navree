//! `{api_prefix}/notifications`: the signed-in user's notifications and
//! push subscriptions.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Path, Query};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, Uri, header};
use navree_db::{Page, PageQuery, Uow};
use navree_http::{
    ApiMeta, AppError, Controller, Created, CurrentPrincipal, Json, NoContent, RouteDef,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::schema::{
    new_push_subscription_schema, notification_page_schema, push_key_schema,
    push_subscription_schema, unread_count_schema, unsubscribe_schema,
};
use crate::{
    DatabaseNotification, NewPushSubscription, Notifiable, Notifier, NotifyError,
    PushSubscriptions, USER_NOTIFIABLE_TYPE,
};

/// Serves `GET /notifications`, `GET /notifications/unread-count`,
/// `POST /notifications/{id}/read` and `POST /notifications/read-all` for
/// the signed-in user (notifiable type `user`), when
/// [`NotifyOptions::routes`](crate::NotifyOptions::routes) is on; and
/// `GET /notifications/push-key` (with a VAPID key pair) and `POST` /
/// `DELETE /notifications/push-subscriptions`.
pub(crate) struct NotificationsController {
    notifier: Arc<Notifier>,
    push: Arc<PushSubscriptions>,
}

impl Injectable for NotificationsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Notifier>(),
            ProviderKey::of::<PushSubscriptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(NotificationsController {
                notifier: r.get::<Notifier>().await?,
                push: r.get::<PushSubscriptions>().await?,
            })
        })
    }
}

/// The body of `DELETE /notifications/push-subscriptions`.
#[derive(Deserialize)]
struct Unsubscribe {
    endpoint: String,
}

impl Controller for NotificationsController {
    fn prefix(&self) -> &'static str {
        "/notifications"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.notifier.options().routes {
            return Vec::new();
        }
        let mut routes = self.clone().push_routes();
        routes.extend(self.read_routes());
        routes
    }
}

impl NotificationsController {
    /// The push key and the push subscription routes.
    fn push_routes(self: Arc<Self>) -> Vec<RouteDef> {
        let mut routes = Vec::new();
        if let Some(key) = self.notifier.application_server_key() {
            routes.push(
                RouteDef::on(Method::GET, "/push-key", move || {
                    let key = key.clone();
                    async move { Json(json!({ "public_key": key })) }
                })
                .authenticated()
                .name("notifications.push_key")
                .api(api(
                    "The application server key browsers subscribe to push with",
                    &[401],
                    Some(push_key_schema),
                )),
            );
        }
        let push = self.push.clone();
        let subscribe = move |CurrentPrincipal(me): CurrentPrincipal,
                              _uow: Uow,
                              uri: Uri,
                              headers: HeaderMap,
                              Json(input): Json<NewPushSubscription>| {
            let push = push.clone();
            async move {
                if let Err((field, message)) = input.check() {
                    return Err(AppError::validation(BTreeMap::from([(
                        field.to_owned(),
                        vec![message],
                    )])));
                }
                let user_agent = headers
                    .get(header::USER_AGENT)
                    .and_then(|value| value.to_str().ok());
                let subscription = push
                    .subscribe(&SignedIn(me.user_id), input, user_agent)
                    .await
                    .map_err(to_app_error)?;
                let location = format!("{}/{}", uri.path().trim_end_matches('/'), subscription.id);
                Ok::<_, AppError>(Created(location, subscription))
            }
        };
        let push = self.push.clone();
        let unsubscribe = move |CurrentPrincipal(me): CurrentPrincipal,
                                _uow: Uow,
                                Json(input): Json<Unsubscribe>| {
            let push = push.clone();
            async move {
                if push
                    .unsubscribe(&SignedIn(me.user_id), &input.endpoint)
                    .await
                    .map_err(to_app_error)?
                {
                    Ok(NoContent)
                } else {
                    Err(AppError::not_found())
                }
            }
        };
        routes.push(
            RouteDef::on(Method::POST, "/push-subscriptions", subscribe)
                .authenticated()
                .name("notifications.push_subscriptions.store")
                .api(ApiMeta {
                    success_status: 201,
                    request: Some(new_push_subscription_schema),
                    ..api(
                        "Save a browser push subscription for the signed-in user",
                        &[400, 401],
                        Some(push_subscription_schema),
                    )
                }),
        );
        routes.push(
            RouteDef::on(Method::DELETE, "/push-subscriptions", unsubscribe)
                .authenticated()
                .name("notifications.push_subscriptions.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    request: Some(unsubscribe_schema),
                    ..api(
                        "Delete one of the signed-in user's push subscriptions",
                        &[400, 401, 404],
                        None,
                    )
                }),
        );
        routes
    }

    /// The notification list and read state routes.
    fn read_routes(self: Arc<Self>) -> Vec<RouteDef> {
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
                &[400, 401],
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
/// value keeps all. Anything else is a 400.
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
                vec!["unread must be a boolean value".to_owned()],
            )]))),
        }
    }
}

/// The HTTP error of a [`NotifyError`] from the push subscription routes.
fn to_app_error(error: NotifyError) -> AppError {
    match error {
        NotifyError::Database(error) => AppError::from(error),
        NotifyError::InvalidRecipient(message) => {
            AppError::validation(BTreeMap::from([("endpoint".to_owned(), vec![message])]))
        }
        NotifyError::Unavailable { reason, .. } => AppError::service_unavailable(reason),
        other => AppError::internal(other.to_string()),
    }
}

/// The OpenAPI metadata shared by the routes.
fn api(summary: &str, responses: &[u16], response: Option<navree_http::SchemaFn>) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Notifications".to_owned()],
        responses: responses.to_vec(),
        response,
        ..ApiMeta::default()
    }
}
