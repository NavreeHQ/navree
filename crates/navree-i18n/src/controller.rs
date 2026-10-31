//! The routes: `GET {api_prefix}/i18n/locales` (a language picker's list)
//! and `PUT {api_prefix}/me/locale` (the signed-in user's saved locale).

use std::collections::BTreeMap;
use std::sync::Arc;

use http::Method;
use navree_db::{Db, Uow};
use navree_http::response::{IntoResponse, Response};
use navree_http::{
    ApiMeta, AppError, Authenticator, Controller, CurrentPrincipal, Json, NoContent, RouteDef,
    schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_validation::{MessageTranslator, Messages};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{I18n, I18nError};

/// The OpenAPI tag of the routes.
const TAG: &str = "Localization";

/// One supported locale (`GET /api/i18n/locales`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LocaleResource {
    /// The tag (`fr`, `pt-BR`).
    pub tag: String,
    /// Its name in its own language (`Français`, `Português (Brasil)`).
    pub name: String,
    /// `ltr` or `rtl`.
    pub direction: String,
    /// Whether it is `i18n.default_locale`.
    pub default: bool,
}

/// `{"data": [...]}`: the supported locales, in configuration order.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct LocaleListResource {
    /// The supported locales.
    pub data: Vec<LocaleResource>,
}

/// The body of `PUT /api/me/locale`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LocaleInput {
    /// A supported locale (`fr`; `fr-CA` is saved as `fr` when only `fr`
    /// is supported).
    pub locale: String,
}

/// `GET {api_prefix}/i18n/locales` (public) and `PUT
/// {api_prefix}/me/locale` (authenticated; mounted when the app has
/// `DatabaseModule` and an authenticator). Neither is mounted when
/// `i18n.routes` is false.
pub struct I18nController {
    i18n: Arc<I18n>,
    /// Whether `PUT /me/locale` can work in this app.
    save_route: bool,
}

impl Injectable for I18nController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<I18n>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let app = r.app();
            let save_route = app.try_get::<Db>().await.is_some()
                && app.try_get::<dyn Authenticator>().await.is_some();
            Ok(I18nController {
                i18n: r.get::<I18n>().await?,
                save_route,
            })
        })
    }
}

impl Controller for I18nController {
    fn prefix(&self) -> &'static str {
        "/"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.i18n.options().routes {
            return Vec::new();
        }
        let index = self.clone();
        let mut routes = vec![
            RouteDef::on(Method::GET, "/i18n/locales", move || {
                let this = index.clone();
                async move { this.index() }
            })
            .public()
            .name("i18n.locales")
            .api(ApiMeta {
                summary: Some("The supported locales, with their native names".to_owned()),
                tags: vec![TAG.to_owned()],
                response: Some(schema_fn::<LocaleListResource>),
                ..ApiMeta::default()
            }),
        ];
        if self.save_route {
            let save = self.clone();
            routes.push(
                RouteDef::on(
                    Method::PUT,
                    "/me/locale",
                    move |CurrentPrincipal(principal): CurrentPrincipal,
                          uow: Uow,
                          Json(input): Json<LocaleInput>| {
                        let this = save.clone();
                        async move { this.save(principal.user_id, uow, input).await }
                    },
                )
                .authenticated()
                .name("i18n.me.locale")
                .api(ApiMeta {
                    summary: Some("Save the signed-in user's locale".to_owned()),
                    tags: vec![TAG.to_owned()],
                    request: Some(schema_fn::<LocaleInput>),
                    success_status: 204,
                    responses: vec![400],
                    ..ApiMeta::default()
                }),
            );
        }
        routes
    }
}

impl I18nController {
    fn index(&self) -> Json<LocaleListResource> {
        let default = self.i18n.default_locale();
        Json(LocaleListResource {
            data: self
                .i18n
                .locales()
                .into_iter()
                .map(|tag| {
                    let locale = self.i18n.locale(&tag);
                    LocaleResource {
                        name: locale.native_name(),
                        direction: locale.direction().to_owned(),
                        default: tag == default,
                        tag,
                    }
                })
                .collect(),
        })
    }

    async fn save(&self, user: Uuid, uow: Uow, input: LocaleInput) -> Result<Response, AppError> {
        match self.i18n.set_user_locale(&uow, user, &input.locale).await {
            Ok(_) => Ok(NoContent.into_response()),
            Err(I18nError::InvalidLocale(_) | I18nError::UnsupportedLocale { .. }) => {
                Err(self.unsupported())
            }
            Err(error) => Err(AppError::internal(error)),
        }
    }

    /// The 400 for a locale that is not supported, in the request's
    /// language: `locale must be one of the following values: en, fr`.
    fn unsupported(&self) -> AppError {
        let values = [("values", self.i18n.locales().join(", "))];
        let messages = Messages::default();
        let translator = crate::validation::Translator {
            i18n: self.i18n.clone(),
        };
        let message = translator.message(&messages, "locale", "in", &values);
        AppError::validation(BTreeMap::from([("locale".to_owned(), vec![message])]))
    }
}
