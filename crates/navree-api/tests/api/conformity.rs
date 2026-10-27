//! `api.require_docs`, `api.undocumented` and the `api.docs` conformity
//! check: every route is documented with `#[api]`.

use navree_api::ApiOptions;
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{CONFORMITY_COMMAND, ConformityOptions, ConformityReport, Finding};
use navree_kernel::App;

use crate::support::{app, boot_error};

async fn api_docs(app: &App) -> navree_contracts::CheckReport {
    let report = ConformityReport::run(
        app.ctx(),
        &ConformityOptions::new(".").only(["api.docs".to_owned()]),
    )
    .await;
    assert_eq!(report.checks.len(), 1, "{report}");
    report.checks[0].clone()
}

#[tokio::test]
async fn the_check_reports_every_undocumented_route() {
    let app = app(ApiOptions::default()).boot().await.unwrap();
    let check = api_docs(&app).await;
    // The deals module's five routes; the API module's own are left out.
    assert_eq!(check.outcome.checked, 5);
    let hint = "add #[api(summary = \"...\")] to the handler (or `.api(ApiMeta { .. })` to the RouteDef), or list the route under api.undocumented";
    assert_eq!(
        check.outcome.findings,
        vec![
            Finding::error(
                "DELETE /api/v1/deals/{id}/notes/{note_id}",
                "has no #[api] documentation (module DealsModule)"
            )
            .hint(hint),
            Finding::error(
                "GET /files/{*path}",
                "has no #[api] documentation (module DealsModule)"
            )
            .hint(hint),
        ]
    );
}

#[tokio::test]
async fn undocumented_lists_the_routes_allowed_without_docs() {
    let options = ApiOptions {
        undocumented: vec![
            "GET /files/*".to_owned(),
            "delete /api/v1/deals/{id}/notes/{note_id}".to_owned(),
        ],
        ..ApiOptions::default()
    };
    let booted = app(options).boot().await.unwrap();
    let check = api_docs(&booted).await;
    assert_eq!(check.outcome.checked, 5);
    assert!(check.outcome.findings.is_empty(), "{check:?}");

    // A path without a method allows every method; a pattern without `*`
    // matches the whole path only.
    let options = ApiOptions {
        undocumented: vec![
            "/files/{*path}".to_owned(),
            "DELETE /api/v1/deals".to_owned(),
        ],
        ..ApiOptions::default()
    };
    let app = app(options).boot().await.unwrap();
    let check = api_docs(&app).await;
    let subjects: Vec<_> = check.outcome.findings.iter().map(|f| &f.subject).collect();
    assert_eq!(subjects, ["DELETE /api/v1/deals/{id}/notes/{note_id}"]);
}

#[tokio::test]
async fn require_docs_stops_the_boot_on_an_undocumented_route() {
    let error = boot_error(app(ApiOptions {
        require_docs: true,
        ..ApiOptions::default()
    }))
    .await;
    assert!(
        error.contains(
            "api.require_docs: 2 routes have no #[api] documentation: DELETE /api/v1/deals/{id}/notes/{note_id} (module DealsModule), GET /files/{*path} (module DealsModule)"
        ),
        "{error}"
    );
}

#[tokio::test]
async fn require_docs_boots_when_every_route_is_documented_or_allowed() {
    let app = app(ApiOptions {
        require_docs: true,
        undocumented: vec!["/files/*".to_owned(), "/api/v1/deals/*".to_owned()],
        ..ApiOptions::default()
    })
    .boot()
    .await;
    assert!(app.is_ok());
}

#[tokio::test]
async fn both_keys_come_from_the_environment() {
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_API__REQUIRE_DOCS", "true"),
            (
                "APP_API__UNDOCUMENTED",
                "GET /files/*, DELETE /api/v1/deals/{id}/notes/{note_id}",
            ),
        ],
    );
    let app = navree_kernel::Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(navree_http::HttpModule::for_root(
            crate::support::http_options(),
        ))
        .module(navree_api::ApiModule::from_config())
        .module(crate::support::DealsModule)
        .override_value::<dyn navree_http::Authenticator>(std::sync::Arc::new(
            crate::support::GuestAuthenticator,
        ))
        .override_value::<dyn navree_http::RouteAuthorizer>(std::sync::Arc::new(
            crate::support::AllowAll,
        ))
        .boot()
        .await
        .unwrap();
    let options = app.ctx().get::<ApiOptions>().await.unwrap();
    assert!(options.require_docs);
    assert_eq!(
        options.undocumented,
        ["GET /files/*", "DELETE /api/v1/deals/{id}/notes/{note_id}"]
    );
}

#[tokio::test]
async fn api_module_registers_the_conformity_command() {
    let app = app(ApiOptions::default()).boot().await.unwrap();
    assert!(
        app.ctx()
            .commands()
            .iter()
            .any(|c| c.name == CONFORMITY_COMMAND)
    );
    let error = app
        .run_command(CONFORMITY_COMMAND, vec!["--only".into(), "api".into()])
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "conformity: 2 errors, 0 warnings");
}
