//! Route documentation: which routes have no `#[api]` metadata, the
//! `api.require_docs` boot check and the `api.docs` conformity check.

use navree_contracts::{CheckOutcome, ConformityCheck, Finding};
use navree_http::{RouteMeta, route_table};
use navree_kernel::{AppContext, BoxError, ModuleId};

use crate::{ApiModule, ApiOptions};

/// The hint of an undocumented-route finding.
const HINT: &str = "add #[api(summary = \"...\")] to the handler (or `.api(ApiMeta { .. })` to the RouteDef), or list the route under api.undocumented";

/// The routes the document describes: every route except the API module's
/// own (`openapi.json`, `docs`).
pub(crate) async fn documented_routes(ctx: &AppContext) -> Result<Vec<RouteMeta>, BoxError> {
    let own_module = ModuleId::of::<ApiModule>().name();
    Ok(route_table(ctx)
        .await?
        .into_iter()
        .filter(|route| route.module != Some(own_module))
        .collect())
}

/// The routes of `routes` without `#[api]` documentation that
/// `api.undocumented` does not allow.
pub(crate) fn undocumented<'a>(
    routes: &'a [RouteMeta],
    options: &ApiOptions,
) -> Vec<&'a RouteMeta> {
    routes
        .iter()
        .filter(|route| route.api.is_none())
        .filter(|route| {
            !options
                .undocumented
                .iter()
                .any(|pattern| allows(pattern, &route.method, &route.full_path))
        })
        .collect()
}

/// `GET /api/v1/deals`.
pub(crate) fn route_label(route: &RouteMeta) -> String {
    format!("{} {}", route.method, route.full_path)
}

fn module_label(route: &RouteMeta) -> String {
    format!("module {}", route.module.unwrap_or("unknown"))
}

/// The boot error of `api.require_docs`, when a route is undocumented.
pub(crate) fn require_docs_error(routes: &[&RouteMeta]) -> String {
    let listed: Vec<String> = routes
        .iter()
        .map(|route| format!("{} ({})", route_label(route), module_label(route)))
        .collect();
    let count = if routes.len() == 1 {
        "1 route has".to_owned()
    } else {
        format!("{} routes have", routes.len())
    };
    format!(
        "api.require_docs: {count} no #[api] documentation: {}",
        listed.join(", ")
    )
}

/// Whether `api.undocumented` entry `pattern` allows `method path`.
fn allows(pattern: &str, method: &str, path: &str) -> bool {
    let pattern = pattern.trim();
    let (pattern_method, pattern_path) = match pattern.split_once(char::is_whitespace) {
        Some((m, p)) => (Some(m), p.trim()),
        None => (None, pattern),
    };
    if pattern_method.is_some_and(|m| !m.eq_ignore_ascii_case(method)) {
        return false;
    }
    match pattern_path.strip_suffix('*') {
        Some(prefix) => path.starts_with(prefix),
        None => path == pattern_path,
    }
}

/// `api.docs`: every route is documented with `#[api]`.
pub(crate) fn docs_check() -> ConformityCheck {
    ConformityCheck::new(
        "api.docs",
        "Every route is documented with #[api]",
        |cx| async move {
            let options = cx.app().get::<ApiOptions>().await?;
            let routes = documented_routes(cx.app()).await?;
            let findings = undocumented(&routes, &options)
                .into_iter()
                .map(|route| {
                    Finding::error(
                        route_label(route),
                        format!("has no #[api] documentation ({})", module_label(route)),
                    )
                    .hint(HINT)
                })
                .collect::<Vec<_>>();
            Ok(CheckOutcome::checked(routes.len()).findings(findings))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::allows;

    #[test]
    fn patterns_match_method_and_path() {
        assert!(allows("GET /files/*", "GET", "/files/a/b"));
        assert!(allows("get /files/*", "GET", "/files/"));
        assert!(!allows("GET /files/*", "POST", "/files/a"));
        assert!(allows("/files/*", "DELETE", "/files/a"));
        assert!(allows("/health", "GET", "/health"));
        assert!(!allows("/health", "GET", "/health/live"));
        assert!(allows("  POST   /api/hooks  ", "POST", "/api/hooks"));
        assert!(allows("*", "GET", "/anything"));
    }
}
