//! The GraphiQL page served on `GET <path>` in development.

/// GraphiQL, React and the WebSocket client come from unpkg, pinned.
const GRAPHIQL: &str = "https://unpkg.com/graphiql@3.8.3";
const REACT: &str = "https://unpkg.com/react@18.3.1/umd/react.production.min.js";
const REACT_DOM: &str = "https://unpkg.com/react-dom@18.3.1/umd/react-dom.production.min.js";

/// The page's content security policy: the pinned scripts and styles, the
/// inline bootstrap script, and requests to this origin only.
pub(crate) const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' https://unpkg.com; \
     style-src 'unsafe-inline' https://unpkg.com; img-src data: https:; font-src data: \
     https://unpkg.com; connect-src 'self' ws: wss:; frame-ancestors 'none'; base-uri 'none'";

/// The page for the endpoint `path` (and the subscriptions endpoint, when
/// subscriptions are on). The `Authorization` header from GraphiQL's
/// headers tab goes with every query and, for subscriptions, in the
/// `connection_init` payload.
pub(crate) fn page(path: &str, subscriptions: Option<&str>) -> String {
    let subscriptions = match subscriptions {
        Some(ws) => format!("'{}'", escape_js(ws)),
        None => "null".to_owned(),
    };
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="robots" content="noindex">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>GraphiQL</title>
<style>body {{ margin: 0; height: 100vh; overflow: hidden; }} #graphiql {{ height: 100vh; }}</style>
<link rel="stylesheet" href="{GRAPHIQL}/graphiql.min.css">
<script crossorigin src="{REACT}"></script>
<script crossorigin src="{REACT_DOM}"></script>
<script crossorigin src="{GRAPHIQL}/graphiql.min.js"></script>
</head>
<body>
<div id="graphiql">Loading...</div>
<script>
const endpoint = new URL('{path}', window.location.href).toString();
const ws = {subscriptions};
const wsUrl = ws === null ? undefined : (() => {{
  const url = new URL(ws, window.location.href);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  return url.toString();
}})();
const fetcher = (params, options) => {{
  const headers = (options && options.headers) || {{}};
  const authorization = headers.Authorization || headers.authorization;
  return GraphiQL.createFetcher({{
    url: endpoint,
    subscriptionUrl: wsUrl,
    wsConnectionParams: authorization ? {{ authorization }} : {{}},
  }})(params, options);
}};
ReactDOM.createRoot(document.getElementById('graphiql')).render(
  React.createElement(GraphiQL, {{ fetcher, defaultEditorToolsVisibility: true }})
);
</script>
</body>
</html>
"#,
        path = escape_js(path),
    )
}

/// `value` for a single-quoted JavaScript string inside HTML (paths are
/// validated, so this is belt and braces).
fn escape_js(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_points_at_both_endpoints() {
        let html = page("/graphql", Some("/graphql/ws"));
        assert!(html.contains("new URL('/graphql'"));
        assert!(html.contains("const ws = '/graphql/ws';"));
        assert!(html.contains("graphiql@3.8.3"));
        let html = page("/api/graphql", None);
        assert!(html.contains("const ws = null;"));
    }

    #[test]
    fn paths_cannot_break_out_of_the_script() {
        assert_eq!(escape_js("/x';alert(1)//"), "/xalert1//");
    }
}
