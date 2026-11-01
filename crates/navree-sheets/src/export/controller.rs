//! The routes of registered exports: `/api/exports...`.

use std::sync::Arc;

use http::{Method, StatusCode, Uri};
use navree_db::Uow;
use navree_http::response::IntoResponse;
use navree_http::{
    Authenticator, Controller, CurrentPrincipal, Json, Path, Query, RouteDef, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver, WeakAppContext};
use navree_openapi::ToSchema;
use navree_queue::OperationRecord;
use navree_workspace::WorkspaceConfig;
use serde::Deserialize;
use serde_json::Value;

use super::service::Exports;
use super::{ExportInfo, ExportRequest};
use crate::http::{DataList, Scope, allowed, api, require, scoped, upgrade};
use crate::{SheetFormat, SheetsError, SheetsOptions};

/// `?format=csv` on `GET /api/exports/{name}`.
#[derive(Clone, Debug, Default, Deserialize)]
struct FormatQuery {
    format: Option<String>,
}

/// The body of `POST /api/exports/{name}`.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct QueueExportInput {
    /// `csv` (the default) or `xlsx`.
    #[serde(default)]
    pub format: SheetFormat,
    /// The export's parameters.
    #[schema(value_type = Object)]
    pub params: Option<Value>,
}

/// The routes of registered exports, under `{http.api_prefix}/exports`
/// (default `/api/exports`). They mount when `sheets.exports.routes` is
/// true and the app has an authenticator. Every route needs a signed-in
/// user holding the export's permission (default `{name}.export`,
/// app-wide or in the request's workspace); in an app with
/// `WorkspaceModule` they run in the request's workspace.
///
/// | Route | Answer |
/// |---|---|
/// | `GET /exports` | `{data: [{name, columns: [{key, header}]}]}`: the exports the user may run |
/// | `GET /exports/{name}?format=csv&...` | the file (CSV streamed; the query string fills the export's parameters) |
/// | `POST /exports/{name}` `{format?, params?}` | 202, the operation; its result is an `ExportResult` with a signed link |
pub struct ExportsController {
    exports: Arc<Exports>,
    enabled: bool,
    workspaces: bool,
    app: WeakAppContext,
}

impl Injectable for ExportsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Exports>(),
            ProviderKey::of::<SheetsOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let app = r.app();
            let options = r.get::<SheetsOptions>().await?;
            let enabled =
                options.exports.routes && app.try_get::<dyn Authenticator>().await.is_some();
            Ok(ExportsController {
                exports: r.get::<Exports>().await?,
                enabled,
                workspaces: app.try_get::<WorkspaceConfig>().await.is_some(),
                app: app.downgrade(),
            })
        })
    }
}

fn format_of(query: &FormatQuery) -> Result<SheetFormat, SheetsError> {
    match query.format.as_deref() {
        None | Some("") => Ok(SheetFormat::Csv),
        Some(name) => SheetFormat::parse(name).ok_or_else(|| {
            SheetsError::Invalid(format!("unknown format `{name}`: use csv or xlsx"))
        }),
    }
}

impl Controller for ExportsController {
    fn prefix(&self) -> &'static str {
        "/exports"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.enabled {
            return Vec::new();
        }
        let route = scoped(self.workspaces);

        let (exports, app) = (self.exports.clone(), self.app.clone());
        let index = move |CurrentPrincipal(principal): CurrentPrincipal, Scope(scope): Scope| {
            let (exports, app) = (exports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let mut data: Vec<ExportInfo> = Vec::new();
                for registration in exports.registrations() {
                    if allowed(&app, &principal, registration.permission.as_deref(), scope).await? {
                        data.push(exports.info(registration));
                    }
                }
                Ok::<_, navree_http::AppError>(Json(DataList { data }))
            }
        };

        let (exports, app) = (self.exports.clone(), self.app.clone());
        let download = move |CurrentPrincipal(principal): CurrentPrincipal,
                             Scope(scope): Scope,
                             Path(name): Path<String>,
                             Query(query): Query<FormatQuery>,
                             uri: Uri| {
            let (exports, app) = (exports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let registration = exports.registration(&name)?;
                require(&app, &principal, registration.permission.as_deref(), scope).await?;
                let format = format_of(&query)?;
                let params = (registration.query_params)(&uri)?;
                let mut request = ExportRequest::new(format).user(principal.user_id);
                request.workspace_id = scope;
                let download = exports.download_named(&name, params, &request).await?;
                Ok::<_, navree_http::AppError>(download.into_response())
            }
        };

        let (exports, app) = (self.exports.clone(), self.app.clone());
        let queue = move |CurrentPrincipal(principal): CurrentPrincipal,
                          Scope(scope): Scope,
                          uow: Uow,
                          Path(name): Path<String>,
                          Json(input): Json<QueueExportInput>| {
            let (exports, app) = (exports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let registration = exports.registration(&name)?;
                require(&app, &principal, registration.permission.as_deref(), scope).await?;
                let params = (registration.json_params)(input.params)?;
                let mut request = ExportRequest::new(input.format).user(principal.user_id);
                request.workspace_id = scope;
                let operation = exports.queue_named(&uow, &name, params, &request).await?;
                Ok::<_, navree_http::AppError>((StatusCode::ACCEPTED, Json(operation)))
            }
        };

        vec![
            route(RouteDef::on(Method::GET, "/", index))
                .name("sheets.exports.index")
                .api(api(
                    "Exports",
                    "List the exports you may run",
                    &[401],
                    None,
                    Some(schema_fn::<ExportInfo>),
                )),
            route(RouteDef::on(Method::GET, "/{name}", download))
                .name("sheets.exports.download")
                .api(api(
                    "Exports",
                    "Download an export (CSV or XLSX)",
                    &[400, 401, 403, 404],
                    None,
                    None,
                )),
            route(RouteDef::on(Method::POST, "/{name}", queue))
                .name("sheets.exports.queue")
                .api(api(
                    "Exports",
                    "Queue an export; follow its operation",
                    &[202, 400, 401, 403, 404],
                    Some(schema_fn::<QueueExportInput>),
                    Some(schema_fn::<OperationRecord>),
                )),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_defaults_to_csv() {
        assert_eq!(
            format_of(&FormatQuery::default()).unwrap(),
            SheetFormat::Csv
        );
        let xlsx = FormatQuery {
            format: Some("XLSX".into()),
        };
        assert_eq!(format_of(&xlsx).unwrap(), SheetFormat::Xlsx);
        let bad = FormatQuery {
            format: Some("pdf".into()),
        };
        assert!(
            format_of(&bad)
                .unwrap_err()
                .to_string()
                .contains("unknown format `pdf`")
        );
    }
}
