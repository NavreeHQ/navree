//! The routes of imports: `/api/imports...`.

use std::sync::Arc;

use axum::extract::Multipart;
use bytes::BytesMut;
use http::{Method, StatusCode};
use navree_contracts::Principal;
use navree_db::Uow;
use navree_http::response::IntoResponse;
use navree_http::{
    AppError, Authenticator, Controller, CurrentPrincipal, Json, NoContent, Path, Query, RouteDef,
    schema_fn,
};
use navree_kernel::{
    AppContext, BoxFuture, Injectable, KernelError, ProviderKey, Resolver, WeakAppContext,
};
use navree_workspace::WorkspaceConfig;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::service::{Imports, Upload, UploadOptions};
use super::{DuplicateMode, ImportColumn, ImportRecord, Mapping, RowQuery, RowsPage};
use crate::http::{DataList, Scope, allowed, api, require, scoped, upgrade};
use crate::{SheetFormat, SheetsError, SheetsOptions};

/// The body of `PUT /api/imports/{id}/mapping`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct MappingInput {
    /// Which header each field reads.
    pub mapping: Mapping,
    /// Keep it for later files with the same headers.
    #[serde(default)]
    pub save: bool,
}

/// The body of `POST /api/imports/{id}/apply`.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct ApplyInput {
    /// What happens to duplicates: `skip` (the default), `update` or `fail`.
    #[serde(default)]
    pub duplicates: DuplicateMode,
}

/// An importer the user may use: what `GET /api/imports/importers` lists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ImporterInfo {
    /// The importer's name.
    pub name: String,
    /// Its columns.
    pub columns: Vec<ImportColumn>,
}

/// `?limit=20` on `GET /api/imports`.
#[derive(Clone, Debug, Default, Deserialize)]
struct ListQuery {
    limit: Option<u64>,
}

/// `?format=xlsx` on the template route.
#[derive(Clone, Debug, Default, Deserialize)]
struct FormatQuery {
    format: Option<String>,
}

/// The routes of imports, under `{http.api_prefix}/imports` (default
/// `/api/imports`). They mount when `sheets.imports.routes` is true and
/// the app has an authenticator. Every route needs a signed-in user; an
/// importer's routes need its permission (default `{importer}.import`,
/// app-wide or in the import's workspace), and an import is a 404 to
/// users without it (an importer without a permission: to everyone but
/// the uploader). In an app with `WorkspaceModule` they act in the
/// request's workspace.
///
/// | Route | Answer |
/// |---|---|
/// | `GET /imports?limit=20` | `{data: [Import]}`: the user's own imports, newest first |
/// | `POST /imports` (multipart: `importer`, `file`, `encoding?`, `sheet?`, `mapping?` JSON, `save_mapping?`) | 202, the import (`uploaded`), staging queued |
/// | `GET /imports/importers` | `{data: [{name, columns}]}`: the importers the user may use |
/// | `GET /imports/templates/{importer}?format=csv` | an empty file with the importer's headers |
/// | `GET /imports/{id}` | the import |
/// | `GET /imports/{id}/rows?status=invalid&page=1&per_page=50` | a page of staged rows |
/// | `PUT /imports/{id}/mapping` `{mapping, save?}` | 202, the import, staged again |
/// | `POST /imports/{id}/apply` `{duplicates?}` | 202, the import (`applying`) |
/// | `GET /imports/{id}/errors` | the error file (CSV), 404 when there is none |
/// | `DELETE /imports/{id}` | 204 (409 while it is being applied) |
pub struct ImportsController {
    imports: Arc<Imports>,
    options: Arc<SheetsOptions>,
    enabled: bool,
    workspaces: bool,
    app: WeakAppContext,
}

impl Injectable for ImportsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Imports>(),
            ProviderKey::of::<SheetsOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let app = r.app();
            let options = r.get::<SheetsOptions>().await?;
            let enabled =
                options.imports.routes && app.try_get::<dyn Authenticator>().await.is_some();
            Ok(ImportsController {
                imports: r.get::<Imports>().await?,
                enabled,
                workspaces: app.try_get::<WorkspaceConfig>().await.is_some(),
                options,
                app: app.downgrade(),
            })
        })
    }
}

/// The import `id` when the principal may see it, else a 404.
async fn visible(
    imports: &Imports,
    app: &AppContext,
    principal: &Principal,
    scope: Option<Uuid>,
    workspaces: bool,
    id: &str,
) -> Result<ImportRecord, AppError> {
    let id = Uuid::parse_str(id).map_err(|_| AppError::not_found())?;
    let record = imports.find(id).await?;
    if workspaces && record.workspace_id != scope {
        return Err(AppError::not_found());
    }
    let permission = imports
        .registration(&record.importer)
        .ok()
        .and_then(|registration| registration.permission.clone());
    let may = match permission {
        Some(permission) => allowed(app, principal, Some(&permission), record.workspace_id).await?,
        None => record.owner_id == Some(principal.user_id),
    };
    if may {
        Ok(record)
    } else {
        Err(AppError::not_found())
    }
}

/// The upload form, read field by field (the file at most `limit` bytes).
struct UploadForm {
    importer: Option<String>,
    file: Option<Upload>,
    options: UploadOptions,
}

async fn read_form(mut form: Multipart, limit: u64) -> Result<UploadForm, AppError> {
    let bad = |error: axum::extract::multipart::MultipartError| {
        AppError::bad_request(format!("the upload cannot be read: {}", error.body_text()))
    };
    let mut out = UploadForm {
        importer: None,
        file: None,
        options: UploadOptions::default(),
    };
    while let Some(mut field) = form.next_field().await.map_err(bad)? {
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "file" => {
                let filename = field.file_name().unwrap_or("upload").to_owned();
                let mut bytes = BytesMut::new();
                while let Some(chunk) = field.chunk().await.map_err(bad)? {
                    if (bytes.len() + chunk.len()) as u64 > limit {
                        return Err(SheetsError::FileTooLarge { limit }.to_app_error());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                out.file = Some(Upload::new(filename, bytes.freeze()));
            }
            "importer" | "encoding" | "sheet" | "mapping" | "save_mapping" => {
                let text = field.text().await.map_err(bad)?;
                let text = text.trim().to_owned();
                match name.as_str() {
                    "importer" => out.importer = Some(text),
                    "encoding" if !text.is_empty() => out.options.encoding = Some(text),
                    "sheet" if !text.is_empty() => out.options.sheet = Some(text),
                    "mapping" if !text.is_empty() => {
                        let mapping: Mapping = serde_json::from_str(&text).map_err(|error| {
                            AppError::validation(
                                [(
                                    "mapping".to_owned(),
                                    vec![format!(
                                        "mapping must be a JSON object of field to header: {error}"
                                    )],
                                )]
                                .into(),
                            )
                        })?;
                        out.options.mapping = Some(mapping);
                    }
                    "save_mapping" => {
                        out.options.save_mapping =
                            matches!(text.as_str(), "true" | "1" | "on" | "yes");
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

impl Controller for ImportsController {
    fn prefix(&self) -> &'static str {
        "/imports"
    }

    #[allow(clippy::too_many_lines)]
    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.enabled {
            return Vec::new();
        }
        let route = scoped(self.workspaces);
        let workspaces = self.workspaces;

        let imports = self.imports.clone();
        let index = move |CurrentPrincipal(principal): CurrentPrincipal,
                          Scope(scope): Scope,
                          Query(query): Query<ListQuery>| {
            let imports = imports.clone();
            async move {
                let limit = query.limit.unwrap_or(20).clamp(1, 100);
                let data = imports.list(principal.user_id, scope, limit).await?;
                Ok::<_, AppError>(Json(DataList { data }))
            }
        };

        let (imports, app) = (self.imports.clone(), self.app.clone());
        let limit = self.options.imports.max_upload_bytes;
        let store = move |CurrentPrincipal(principal): CurrentPrincipal,
                          Scope(scope): Scope,
                          uow: Uow,
                          form: Multipart| {
            let (imports, app) = (imports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let form = read_form(form, limit).await?;
                let importer = form.importer.filter(|i| !i.is_empty()).ok_or_else(|| {
                    AppError::validation(
                        [(
                            "importer".to_owned(),
                            vec!["importer should not be empty".to_owned()],
                        )]
                        .into(),
                    )
                })?;
                let registration = imports.registration(&importer)?;
                require(&app, &principal, registration.permission.as_deref(), scope).await?;
                let file = form.file.ok_or_else(|| {
                    AppError::validation(
                        [(
                            "file".to_owned(),
                            vec!["file should not be empty".to_owned()],
                        )]
                        .into(),
                    )
                })?;
                let mut options = form.options.owner(principal.user_id);
                options.workspace_id = scope;
                let record = imports.upload(&uow, &importer, file, options).await?;
                Ok::<_, AppError>((StatusCode::ACCEPTED, Json(record)))
            }
        };

        let (imports, app) = (self.imports.clone(), self.app.clone());
        let importers = move |CurrentPrincipal(principal): CurrentPrincipal,
                              Scope(scope): Scope| {
            let (imports, app) = (imports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let mut data = Vec::new();
                for registration in imports.registrations() {
                    if allowed(&app, &principal, registration.permission.as_deref(), scope).await? {
                        data.push(ImporterInfo {
                            name: registration.name.to_owned(),
                            columns: (registration.columns)(),
                        });
                    }
                }
                Ok::<_, AppError>(Json(DataList { data }))
            }
        };

        let (imports, app) = (self.imports.clone(), self.app.clone());
        let template = move |CurrentPrincipal(principal): CurrentPrincipal,
                             Scope(scope): Scope,
                             Path(importer): Path<String>,
                             Query(query): Query<FormatQuery>| {
            let (imports, app) = (imports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let registration = imports.registration(&importer)?;
                require(&app, &principal, registration.permission.as_deref(), scope).await?;
                let format = match query.format.as_deref() {
                    None | Some("") => SheetFormat::Csv,
                    Some(name) => SheetFormat::parse(name).ok_or_else(|| {
                        AppError::bad_request(format!("unknown format `{name}`: use csv or xlsx"))
                    })?,
                };
                Ok::<_, AppError>(imports.template(&importer, format)?.into_response())
            }
        };

        let (imports, app) = (self.imports.clone(), self.app.clone());
        let show = move |CurrentPrincipal(principal): CurrentPrincipal,
                         Scope(scope): Scope,
                         Path(id): Path<String>| {
            let (imports, app) = (imports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let record = visible(&imports, &app, &principal, scope, workspaces, &id).await?;
                Ok::<_, AppError>(Json(record))
            }
        };

        let (imports, app) = (self.imports.clone(), self.app.clone());
        let rows = move |CurrentPrincipal(principal): CurrentPrincipal,
                         Scope(scope): Scope,
                         Path(id): Path<String>,
                         Query(query): Query<RowQuery>| {
            let (imports, app) = (imports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let record = visible(&imports, &app, &principal, scope, workspaces, &id).await?;
                Ok::<_, AppError>(Json(imports.rows(record.id, &query).await?))
            }
        };

        let (imports, app) = (self.imports.clone(), self.app.clone());
        let map = move |CurrentPrincipal(principal): CurrentPrincipal,
                        Scope(scope): Scope,
                        uow: Uow,
                        Path(id): Path<String>,
                        Json(input): Json<MappingInput>| {
            let (imports, app) = (imports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let record = visible(&imports, &app, &principal, scope, workspaces, &id).await?;
                let record = imports
                    .map(&uow, record.id, input.mapping, input.save)
                    .await?;
                Ok::<_, AppError>((StatusCode::ACCEPTED, Json(record)))
            }
        };

        let (imports, app) = (self.imports.clone(), self.app.clone());
        let apply = move |CurrentPrincipal(principal): CurrentPrincipal,
                          Scope(scope): Scope,
                          uow: Uow,
                          Path(id): Path<String>,
                          body: Option<Json<ApplyInput>>| {
            let (imports, app) = (imports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let record = visible(&imports, &app, &principal, scope, workspaces, &id).await?;
                let input = body.map(|Json(input)| input).unwrap_or_default();
                let record = imports.apply(&uow, record.id, input.duplicates).await?;
                Ok::<_, AppError>((StatusCode::ACCEPTED, Json(record)))
            }
        };

        let (imports, app) = (self.imports.clone(), self.app.clone());
        let errors = move |CurrentPrincipal(principal): CurrentPrincipal,
                           Scope(scope): Scope,
                           Path(id): Path<String>| {
            let (imports, app) = (imports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let record = visible(&imports, &app, &principal, scope, workspaces, &id).await?;
                Ok::<_, AppError>(imports.error_file(record.id).await?.into_response())
            }
        };

        let (imports, app) = (self.imports.clone(), self.app.clone());
        let destroy = move |CurrentPrincipal(principal): CurrentPrincipal,
                            Scope(scope): Scope,
                            uow: Uow,
                            Path(id): Path<String>| {
            let (imports, app) = (imports.clone(), app.clone());
            async move {
                let app = upgrade(&app)?;
                let record = visible(&imports, &app, &principal, scope, workspaces, &id).await?;
                imports.discard(&uow, record.id).await?;
                Ok::<_, AppError>(NoContent)
            }
        };

        let body_limit = usize::try_from(self.options.imports.max_upload_bytes)
            .unwrap_or(usize::MAX)
            .saturating_add(64 * 1024);
        vec![
            route(RouteDef::on(Method::GET, "/", index))
                .name("sheets.imports.index")
                .api(api(
                    "Imports",
                    "List your imports",
                    &[401],
                    None,
                    Some(schema_fn::<ImportRecord>),
                )),
            route(RouteDef::on(Method::POST, "/", store))
                .name("sheets.imports.store")
                .body_limit(body_limit)
                .api(api(
                    "Imports",
                    "Upload a CSV or Excel file to import",
                    &[202, 400, 401, 403, 404, 413],
                    None,
                    Some(schema_fn::<ImportRecord>),
                )),
            route(RouteDef::on(Method::GET, "/importers", importers))
                .name("sheets.imports.importers")
                .api(api(
                    "Imports",
                    "List the importers you may use",
                    &[401],
                    None,
                    Some(schema_fn::<ImporterInfo>),
                )),
            route(RouteDef::on(Method::GET, "/templates/{importer}", template))
                .name("sheets.imports.template")
                .api(api(
                    "Imports",
                    "Download an importer's template",
                    &[400, 401, 403, 404],
                    None,
                    None,
                )),
            route(RouteDef::on(Method::GET, "/{id}", show))
                .name("sheets.imports.show")
                .api(api(
                    "Imports",
                    "Show an import",
                    &[401, 404],
                    None,
                    Some(schema_fn::<ImportRecord>),
                )),
            route(RouteDef::on(Method::GET, "/{id}/rows", rows))
                .name("sheets.imports.rows")
                .api(api(
                    "Imports",
                    "Preview an import's rows",
                    &[401, 404],
                    None,
                    Some(schema_fn::<RowsPage>),
                )),
            route(RouteDef::on(Method::PUT, "/{id}/mapping", map))
                .name("sheets.imports.mapping")
                .api(api(
                    "Imports",
                    "Choose the mapping and stage again",
                    &[202, 400, 401, 404, 409],
                    Some(schema_fn::<MappingInput>),
                    Some(schema_fn::<ImportRecord>),
                )),
            route(RouteDef::on(Method::POST, "/{id}/apply", apply))
                .name("sheets.imports.apply")
                .api(api(
                    "Imports",
                    "Apply a staged import",
                    &[202, 401, 404, 409],
                    Some(schema_fn::<ApplyInput>),
                    Some(schema_fn::<ImportRecord>),
                )),
            route(RouteDef::on(Method::GET, "/{id}/errors", errors))
                .name("sheets.imports.errors")
                .api(api(
                    "Imports",
                    "Download the error file",
                    &[401, 404],
                    None,
                    None,
                )),
            route(RouteDef::on(Method::DELETE, "/{id}", destroy))
                .name("sheets.imports.destroy")
                .api(api(
                    "Imports",
                    "Delete an import",
                    &[204, 401, 404, 409],
                    None,
                    None,
                )),
        ]
    }
}
