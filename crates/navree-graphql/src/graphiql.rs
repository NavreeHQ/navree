//! The GraphiQL page served on `GET <path>` in development, and the files
//! it loads from `GET <path>/assets/{file}`.
//!
//! GraphiQL, React and React DOM are vendored: embedded in the binary at
//! the versions below (unmodified npm builds, each with its MIT license
//! file) and served by the app itself with a year of caching, since the
//! file names carry the version. The page loads nothing from a third party,
//! so its content security policy allows scripts from `'self'` only (the
//! bootstrap is a file too, not an inline script).

/// One embedded file, served as `<path>/assets/{path}`.
pub(crate) struct Asset {
    /// The file name (versioned).
    pub(crate) path: &'static str,
    /// The `Content-Type`.
    pub(crate) content_type: &'static str,
    /// The bytes.
    pub(crate) bytes: &'static [u8],
}

const JS: &str = "text/javascript; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";
const TXT: &str = "text/plain; charset=utf-8";

const REACT: &str = "react-18.3.1.production.min.js";
const REACT_DOM: &str = "react-dom-18.3.1.production.min.js";
const GRAPHIQL_JS: &str = "graphiql-3.8.3.min.js";
const GRAPHIQL_CSS: &str = "graphiql-3.8.3.min.css";
const BOOTSTRAP: &str = "navree-graphiql-1.js";

/// Every embedded file: `graphiql` 3.8.3, `react` and `react-dom` 18.3.1
/// (the `umd` and `graphiql.min.*` builds from npm), their licenses, and
/// the page's bootstrap script. Updating one means replacing its file and
/// the name here.
pub(crate) static ASSETS: &[Asset] = &[
    Asset {
        path: REACT,
        content_type: JS,
        bytes: include_bytes!("../assets/react-18.3.1.production.min.js"),
    },
    Asset {
        path: REACT_DOM,
        content_type: JS,
        bytes: include_bytes!("../assets/react-dom-18.3.1.production.min.js"),
    },
    Asset {
        path: GRAPHIQL_JS,
        content_type: JS,
        bytes: include_bytes!("../assets/graphiql-3.8.3.min.js"),
    },
    Asset {
        path: GRAPHIQL_CSS,
        content_type: CSS,
        bytes: include_bytes!("../assets/graphiql-3.8.3.min.css"),
    },
    Asset {
        path: BOOTSTRAP,
        content_type: JS,
        bytes: include_bytes!("../assets/navree-graphiql-1.js"),
    },
    Asset {
        path: "react-18.3.1.LICENSE.txt",
        content_type: TXT,
        bytes: include_bytes!("../assets/react-18.3.1.LICENSE.txt"),
    },
    Asset {
        path: "react-dom-18.3.1.LICENSE.txt",
        content_type: TXT,
        bytes: include_bytes!("../assets/react-dom-18.3.1.LICENSE.txt"),
    },
    Asset {
        path: "graphiql-3.8.3.LICENSE.txt",
        content_type: TXT,
        bytes: include_bytes!("../assets/graphiql-3.8.3.LICENSE.txt"),
    },
];

/// The cache policy of the embedded files (their names carry the version).
pub(crate) const ASSET_CACHE: &str = "public, max-age=31536000, immutable";

/// The embedded file named `file`, if any.
pub(crate) fn asset(file: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|asset| asset.path == file)
}

/// The directory the assets are served from, for the endpoint at `path`.
pub(crate) fn assets_path(path: &str) -> String {
    format!("{}/assets", path.trim_end_matches('/'))
}

/// The page's content security policy: scripts from this origin only (no
/// inline script), the inline styles GraphiQL's components set, and
/// requests to this origin (and its WebSocket) only.
pub(crate) const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' \
     'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self' ws: \
     wss:; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// The page for the endpoint `path` (and the subscriptions endpoint, when
/// subscriptions are on). The `Authorization` header from GraphiQL's
/// headers tab goes with every query and, for subscriptions, in the
/// `connection_init` payload.
pub(crate) fn page(path: &str, subscriptions: Option<&str>) -> String {
    let assets = escape_attr(&assets_path(path));
    let subscriptions = subscriptions.map(escape_attr).unwrap_or_default();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="robots" content="noindex">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>GraphiQL</title>
<style>body {{ margin: 0; height: 100vh; overflow: hidden; }} #graphiql {{ height: 100vh; }}</style>
<link rel="stylesheet" href="{assets}/{GRAPHIQL_CSS}">
<script src="{assets}/{REACT}"></script>
<script src="{assets}/{REACT_DOM}"></script>
<script src="{assets}/{GRAPHIQL_JS}"></script>
</head>
<body>
<div id="graphiql" data-endpoint="{endpoint}" data-subscriptions="{subscriptions}">Loading...</div>
<script src="{assets}/{BOOTSTRAP}"></script>
</body>
</html>
"#,
        endpoint = escape_attr(path),
    )
}

/// `value` for a double-quoted HTML attribute (paths are validated, so this
/// is belt and braces: only path characters are kept).
fn escape_attr(value: &str) -> String {
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
        assert!(html.contains(r#"data-endpoint="/graphql""#));
        assert!(html.contains(r#"data-subscriptions="/graphql/ws""#));
        assert!(html.contains("/graphql/assets/graphiql-3.8.3.min.js"));
        let html = page("/api/graphql", None);
        assert!(html.contains(r#"data-subscriptions="""#));
        assert!(html.contains("/api/graphql/assets/react-18.3.1.production.min.js"));
    }

    #[test]
    fn the_page_loads_only_local_files_and_has_no_inline_script() {
        let html = page("/graphql", Some("/graphql/ws"));
        for attribute in ["src=\"", "href=\""] {
            for part in html.split(attribute).skip(1) {
                let reference = part.split('"').next().unwrap();
                assert!(reference.starts_with("/graphql/assets/"), "{reference}");
                assert!(asset(reference.rsplit('/').next().unwrap()).is_some());
            }
        }
        assert!(!html.contains("unpkg") && !html.contains("://"), "{html}");
        for script in html.split("<script").skip(1) {
            assert!(script.starts_with(" src=\""), "inline script: {script}");
        }
        let scripts = CSP
            .split(';')
            .find(|directive| directive.trim().starts_with("script-src"))
            .unwrap();
        assert_eq!(scripts.trim(), "script-src 'self'");
        assert!(!CSP.contains("http"), "{CSP}");
    }

    #[test]
    fn every_embedded_file_is_versioned_and_has_content() {
        for asset in ASSETS {
            assert!(!asset.bytes.is_empty(), "{}", asset.path);
            assert!(
                asset.path.chars().any(|c| c.is_ascii_digit()),
                "{} has no version",
                asset.path
            );
        }
        for license in ASSETS.iter().filter(|a| a.path.ends_with("LICENSE.txt")) {
            let text = std::str::from_utf8(license.bytes).unwrap();
            assert!(text.contains("MIT License"), "{}", license.path);
        }
    }

    #[test]
    fn paths_cannot_break_out_of_the_attribute() {
        assert_eq!(escape_attr("/x\"><script>alert(1)//"), "/xscriptalert1//");
    }
}
