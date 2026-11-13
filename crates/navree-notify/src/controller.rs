//! `{api_prefix}/notifications`: the signed-in user's notifications, push
//! subscriptions and push devices.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use http::{HeaderMap, Method, Uri, header};
use navree_contracts::Principal;
use navree_db::{Page, Uow};
use navree_http::list::List;
use navree_http::{
    ApiMeta, AppError, Controller, Created, CurrentPrincipal, Json, NoContent, Path, RouteDef,
    list_spec,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_time::Zone;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::deferred::recipient_zone;
use crate::devices::not_an_app;
use crate::schema::{
    new_push_device_schema, new_push_subscription_schema, notification_page_schema,
    preferences_patch_schema, preferences_schema, push_device_schema, push_key_schema,
    push_subscription_schema, remove_device_schema, unread_count_schema, unsubscribe_schema,
};
use crate::{
    DatabaseNotification, Digest, NewPushDevice, NewPushSubscription, Notifiable,
    NotificationCatalog, NotificationCategory, NotificationList, NotificationPreferences, Notifier,
    NotifyError, PreferenceOwner, PushDevices, PushSubscriptions, ResolvedPreferences,
    USER_NOTIFIABLE_TYPE,
};

/// How often one user may register or remove push devices, per minute.
const DEVICE_CHANGES_PER_MINUTE: u32 = 30;

/// The largest body the device routes read (a Firebase token is at most
/// 4096 bytes).
const DEVICE_BODY_LIMIT: usize = 8 * 1024;

/// Serves `GET /notifications`, `GET /notifications/unread-count`,
/// `POST /notifications/{id}/read` and `POST /notifications/read-all` for
/// the signed-in user (notifiable type `user`), when
/// [`NotifyOptions::routes`](crate::NotifyOptions::routes) is on; and
/// `GET /notifications/push-key` (with a VAPID key pair) and `POST` /
/// `DELETE /notifications/push-subscriptions`; `POST` / `DELETE
/// /notifications/push-devices` (while the app can push to native apps); and `GET` / `PATCH
/// /notifications/preferences` and `/notifications/preferences/workspace`.
pub(crate) struct NotificationsController {
    notifier: Arc<Notifier>,
    push: Arc<PushSubscriptions>,
    devices: Arc<PushDevices>,
    preferences: Arc<NotificationPreferences>,
    catalog: Arc<NotificationCatalog>,
}

impl Injectable for NotificationsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Notifier>(),
            ProviderKey::of::<PushSubscriptions>(),
            ProviderKey::of::<PushDevices>(),
            ProviderKey::of::<NotificationPreferences>(),
            ProviderKey::of::<NotificationCatalog>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(NotificationsController {
                notifier: r.get::<Notifier>().await?,
                push: r.get::<PushSubscriptions>().await?,
                devices: r.get::<PushDevices>().await?,
                preferences: r.get::<NotificationPreferences>().await?,
                catalog: r.get::<NotificationCatalog>().await?,
            })
        })
    }
}

/// The body of `DELETE /notifications/push-subscriptions`.
#[derive(Deserialize)]
struct Unsubscribe {
    endpoint: String,
}

/// The body of `DELETE /notifications/push-devices`.
#[derive(Deserialize)]
struct RemoveDevice {
    token: String,
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
        // Devices register only where something can be pushed to them.
        if self.devices.served() {
            routes.extend(self.clone().device_routes());
        }
        routes.extend(self.clone().preference_routes());
        routes.extend(self.read_routes());
        routes
    }
}

impl NotificationsController {
    /// `GET`/`PATCH /notifications/preferences` (the signed-in user's) and
    /// `GET`/`PATCH /notifications/preferences/workspace` (the request's
    /// workspace's defaults).
    fn preference_routes(self: Arc<Self>) -> Vec<RouteDef> {
        let (show, update, show_workspace, update_workspace) =
            (self.clone(), self.clone(), self.clone(), self);
        vec![
            RouteDef::on(
                Method::GET,
                "/preferences",
                move |CurrentPrincipal(me): CurrentPrincipal, uow: Uow| async move {
                    show.own_preferences(me.user_id, uow.workspace(), None)
                        .await
                },
            )
            .authenticated()
            .name("notifications.preferences.show")
            .api(api(
                "The signed-in user's notification preferences, with their workspace's defaults",
                &[401],
                Some(preferences_schema),
            )),
            RouteDef::on(
                Method::PATCH,
                "/preferences",
                move |CurrentPrincipal(me): CurrentPrincipal,
                      uow: Uow,
                      Json(patch): Json<Value>| async move {
                    update
                        .own_preferences(me.user_id, uow.workspace(), Some(patch))
                        .await
                },
            )
            .authenticated()
            .name("notifications.preferences.update")
            .api(ApiMeta {
                request: Some(preferences_patch_schema),
                ..api(
                    "Change the signed-in user's notification preferences (a JSON merge patch)",
                    &[400, 401],
                    Some(preferences_schema),
                )
            }),
            RouteDef::on(
                Method::GET,
                "/preferences/workspace",
                move |CurrentPrincipal(_me): CurrentPrincipal, uow: Uow| async move {
                    let workspace = uow.workspace().ok_or_else(no_workspace)?;
                    show_workspace.workspace_preferences(workspace, None).await
                },
            )
            .authenticated()
            .name("notifications.preferences.workspace.show")
            .api(api(
                "The notification preferences the request's workspace gives its members",
                &[401, 404],
                Some(preferences_schema),
            )),
            RouteDef::on(
                Method::PATCH,
                "/preferences/workspace",
                move |CurrentPrincipal(me): CurrentPrincipal,
                      uow: Uow,
                      Json(patch): Json<Value>| async move {
                    let workspace = uow.workspace().ok_or_else(no_workspace)?;
                    update_workspace.authorize_workspace(&me, workspace).await?;
                    update_workspace
                        .workspace_preferences(workspace, Some(patch))
                        .await
                },
            )
            .authenticated()
            .name("notifications.preferences.workspace.update")
            .api(ApiMeta {
                request: Some(preferences_patch_schema),
                ..api(
                    "Change the notification preferences the request's workspace gives its members",
                    &[400, 401, 403, 404],
                    Some(preferences_schema),
                )
            }),
        ]
    }

    /// The signed-in user's preferences, changed by `patch` first.
    async fn own_preferences(
        &self,
        user: Uuid,
        workspace: Option<Uuid>,
        patch: Option<Value>,
    ) -> Result<Json<Value>, AppError> {
        let owner = PreferenceOwner::user(user);
        let own = match patch {
            Some(patch) => self.preferences.update(&owner, &patch).await,
            None => self.preferences.of(&owner).await,
        }
        .map_err(to_app_error)?;
        let resolved = self
            .preferences
            .resolve_owner(&owner, workspace)
            .await
            .map_err(to_app_error)?;
        let resolved = ResolvedPreferences { own, ..resolved };
        let ctx = self.notifier.app().map_err(to_app_error)?;
        let zone = recipient_zone(&ctx, &SignedIn(user), workspace).await;
        Ok(Json(self.view(&resolved, zone)))
    }

    /// The workspace's defaults, changed by `patch` first.
    async fn workspace_preferences(
        &self,
        workspace: Uuid,
        patch: Option<Value>,
    ) -> Result<Json<Value>, AppError> {
        let owner = PreferenceOwner::workspace(workspace);
        let own = match patch {
            Some(patch) => self.preferences.update(&owner, &patch).await,
            None => self.preferences.of(&owner).await,
        }
        .map_err(to_app_error)?;
        let resolved = ResolvedPreferences {
            own,
            workspace: None,
            required: self.preferences.options().required.clone(),
        };
        let ctx = self.notifier.app().map_err(to_app_error)?;
        let zone = recipient_zone(&ctx, &WorkspaceOwner(workspace), Some(workspace)).await;
        Ok(Json(self.view(&resolved, zone)))
    }

    /// 403 unless the user may change `workspace`'s defaults
    /// (`notify.preferences.workspace_permission`).
    async fn authorize_workspace(
        &self,
        principal: &Principal,
        workspace: Uuid,
    ) -> Result<(), AppError> {
        let ctx = self.notifier.app().map_err(to_app_error)?;
        let permission = &self.preferences.options().workspace_permission;
        let allowed = navree_contracts::permitted(&ctx, principal, permission, Some(workspace))
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        if allowed {
            Ok(())
        } else {
            Err(AppError::forbidden().with_detail(format!(
                "Changing the workspace's notification preferences needs `{permission}`."
            )))
        }
    }

    /// `{data, workspace, categories, channels, time_zone}`: the owner's own
    /// choices, the workspace's, and what each category gets.
    fn view(&self, resolved: &ResolvedPreferences, zone: Zone) -> Value {
        let options = self.preferences.options();
        let mut categories = self.catalog.categories(&options.required);
        let listed: Vec<String> = categories.iter().map(|c| c.category.clone()).collect();
        let chosen = resolved
            .own
            .categories
            .keys()
            .chain(resolved.workspace.iter().flat_map(|w| w.categories.keys()));
        let mut extra: Vec<&String> = chosen.filter(|c| !listed.contains(c)).collect();
        extra.sort();
        extra.dedup();
        for category in extra {
            categories.push(NotificationCategory {
                category: category.clone(),
                types: Vec::new(),
                digest: Digest::Off,
                required: options.is_required(category),
            });
        }
        let categories: Vec<Value> = categories
            .into_iter()
            .map(|category| {
                let channels: serde_json::Map<String, Value> = options
                    .channels
                    .iter()
                    .map(|channel| {
                        (
                            channel.as_str().to_owned(),
                            Value::Bool(resolved.allows(&category.category, *channel)),
                        )
                    })
                    .collect();
                json!({
                    "category": category.category,
                    "types": category.types,
                    "required": category.required,
                    "digest": resolved.digest(&category.category, category.digest),
                    "channels": channels,
                })
            })
            .collect();
        json!({
            "data": resolved.own,
            "workspace": resolved.workspace,
            "categories": categories,
            "channels": options.channels,
            "time_zone": zone.name(),
        })
    }
}

/// 404 for the workspace routes outside a workspace.
fn no_workspace() -> AppError {
    AppError::not_found().with_detail("This request is not in a workspace.")
}

/// A workspace as the owner of preferences (its zone is the workspace's).
struct WorkspaceOwner(Uuid);

impl Notifiable for WorkspaceOwner {
    fn notifiable_type(&self) -> &'static str {
        crate::WORKSPACE_OWNER_TYPE
    }

    fn notifiable_id(&self) -> String {
        self.0.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        None
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

    /// `POST` and `DELETE /push-devices`: a native app registers the device
    /// token of the signed-in session, and removes it (signing out). Each
    /// is throttled per user.
    fn device_routes(self: Arc<Self>) -> Vec<RouteDef> {
        let devices = self.devices.clone();
        let register = move |CurrentPrincipal(me): CurrentPrincipal,
                             _uow: Uow,
                             uri: Uri,
                             Json(input): Json<NewPushDevice>| {
            let devices = devices.clone();
            async move {
                let Some(session) = me.session_id.as_deref().filter(|s| !s.is_empty()) else {
                    return Err(AppError::forbidden()
                        .with_detail("Registering a push device needs a signed-in session."));
                };
                if let Err((field, message)) = input.check() {
                    return Err(field_error(field, message));
                }
                if !devices.allows(input.platform, input.app_id.trim()) {
                    return Err(field_error("app_id", not_an_app()));
                }
                let device = devices
                    .register(&SignedIn(me.user_id), session, input)
                    .await
                    .map_err(|error| match error {
                        NotifyError::InvalidRecipient(message) => field_error("token", message),
                        other => to_app_error(other),
                    })?;
                let location = format!("{}/{}", uri.path().trim_end_matches('/'), device.id);
                Ok::<_, AppError>(Created(location, device))
            }
        };
        let devices = self.devices.clone();
        let remove = move |CurrentPrincipal(me): CurrentPrincipal,
                           _uow: Uow,
                           Json(input): Json<RemoveDevice>| {
            let devices = devices.clone();
            async move {
                if devices
                    .unregister(&SignedIn(me.user_id), &input.token)
                    .await
                    .map_err(to_app_error)?
                {
                    Ok(NoContent)
                } else {
                    Err(AppError::not_found())
                }
            }
        };
        let per_minute = Duration::from_secs(60);
        vec![
            RouteDef::on(Method::POST, "/push-devices", register)
                .authenticated()
                .throttle(DEVICE_CHANGES_PER_MINUTE, per_minute)
                .body_limit(DEVICE_BODY_LIMIT)
                .name("notifications.push_devices.store")
                .api(ApiMeta {
                    success_status: 201,
                    request: Some(new_push_device_schema),
                    ..api(
                        "Register the native app device of the signed-in session for push notifications",
                        &[400, 401, 403, 422, 429],
                        Some(push_device_schema),
                    )
                }),
            RouteDef::on(Method::DELETE, "/push-devices", remove)
                .authenticated()
                .throttle(DEVICE_CHANGES_PER_MINUTE, per_minute)
                .body_limit(DEVICE_BODY_LIMIT)
                .name("notifications.push_devices.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    request: Some(remove_device_schema),
                    ..api(
                        "Remove one of the signed-in user's push devices",
                        &[400, 401, 404, 429],
                        None,
                    )
                }),
        ]
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
                      list: List<NotificationList>| async move {
                    index.index(SignedIn(me.user_id), uow, list).await
                },
            )
            .authenticated()
            .name("notifications.index")
            .api(ApiMeta {
                query: Some(list_spec::<NotificationList>),
                ..api(
                    "List the signed-in user's notifications, newest first",
                    &[400, 401],
                    Some(notification_page_schema),
                )
            }),
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
        list: List<NotificationList>,
    ) -> Result<Page<DatabaseNotification>, AppError> {
        Ok(self
            .notifier
            .notification_list(uow.db(), &me, &list)
            .await?)
    }

    async fn unread_count(&self, me: SignedIn, uow: Uow) -> Result<Json<Value>, AppError> {
        let count = self.notifier.unread_count(uow.db(), &me).await?;
        Ok(Json(json!({ "count": count })))
    }

    async fn mark_read(&self, me: SignedIn, uow: Uow, id: &str) -> Result<NoContent, AppError> {
        let id = Uuid::parse_str(id).map_err(|_| AppError::not_found())?;
        if self.notifier.mark_as_read(&uow, &me, id).await? {
            Ok(NoContent)
        } else {
            Err(AppError::not_found())
        }
    }

    async fn mark_all_read(&self, me: SignedIn, uow: Uow) -> Result<NoContent, AppError> {
        self.notifier.mark_all_as_read(&uow, &me).await?;
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

/// A 422 naming `field`.
fn field_error(field: &str, message: String) -> AppError {
    AppError::validation(BTreeMap::from([(field.to_owned(), vec![message])]))
}

/// The HTTP error of a [`NotifyError`] from the push subscription and
/// preference routes.
fn to_app_error(error: NotifyError) -> AppError {
    match error {
        NotifyError::Database(error) => AppError::from(error),
        NotifyError::InvalidRecipient(message) => {
            AppError::validation(BTreeMap::from([("endpoint".to_owned(), vec![message])]))
        }
        NotifyError::Unavailable { reason, .. } => AppError::service_unavailable(reason),
        NotifyError::InvalidPreferences(errors) => AppError::validation(errors),
        other => AppError::internal(other.to_string()),
    }
}

/// The OpenAPI metadata shared by the routes (the bell's too).
pub(crate) fn api(
    summary: &str,
    responses: &[u16],
    response: Option<navree_http::SchemaFn>,
) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Notifications".to_owned()],
        responses: responses.to_vec(),
        response,
        ..ApiMeta::default()
    }
}

#[cfg(test)]
mod tests {
    use navree_http::handler::Endpoint;

    use super::*;

    #[test]
    fn handlers_on_the_list_are_native() {
        async fn handler(_: List<NotificationList>) {}
        let _ = Endpoint::new(handler);
    }
}
