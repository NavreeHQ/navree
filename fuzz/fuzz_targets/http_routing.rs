//! `navree_http::routing`: route patterns, path matching and the decoding
//! of path parameters. The input is route patterns (one per line), a NUL,
//! then request paths (one per line).
//!
//! Invariants:
//! - a pattern that parses inserts unless one of the same shape is there;
//! - a matched path is the matched pattern with its raw parameters put
//!   back, and no route of the tree that matches it has a better
//!   precedence (static before parameter before catch-all, from the left);
//! - a path the request check passes has no dot segment and no backslash;
//! - decoded parameters hold no NUL and no backslash, a `{name}` value no
//!   slash, and none is (or, for a catch-all, has a part that is) a dot
//!   segment;
//! - the router answers every path and method, with a 2xx only for a path
//!   the tree matches, and never panics.
#![no_main]

use http::{Method, Request, StatusCode};
use libfuzzer_sys::fuzz_target;
use navree_fuzz::{block_on, split_nul};
use navree_http::PathParams;
use navree_http::routing::{PathPattern, PathTree, Router, Segment, check_path, decode_param};
use tower::ServiceExt;

/// The precedence of `pattern` for `path` (0 static, 1 parameter, 2
/// catch-all per segment), when it matches.
fn rank(pattern: &PathPattern, path: &str) -> Option<Vec<u8>> {
    let parts: Vec<&str> = if path == "/" {
        Vec::new()
    } else {
        path.strip_prefix('/')?.split('/').collect()
    };
    let mut rank = Vec::new();
    for (index, segment) in pattern.segments().iter().enumerate() {
        match segment {
            Segment::Static(text) => {
                if parts.get(index) != Some(&&**text) {
                    return None;
                }
                rank.push(0);
            }
            Segment::Param => {
                parts.get(index).filter(|part| !part.is_empty())?;
                rank.push(1);
            }
            Segment::CatchAll => {
                if index >= parts.len() || parts[index..].join("/").is_empty() {
                    return None;
                }
                rank.push(2);
                return Some(rank);
            }
        }
    }
    (pattern.segments().len() == parts.len()).then_some(rank)
}

fn rebuild(pattern: &PathPattern, raw: &[&str]) -> String {
    let mut values = raw.iter();
    let mut out = String::new();
    for segment in pattern.segments() {
        out.push('/');
        match segment {
            Segment::Static(text) => out.push_str(text),
            Segment::Param | Segment::CatchAll => out.push_str(values.next().expect("a value")),
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let (patterns, paths) = split_nul(data);
    let patterns = String::from_utf8_lossy(patterns);
    let paths = String::from_utf8_lossy(paths);

    let mut tree = PathTree::new();
    let mut router = Router::new();
    let mut shapes = Vec::new();
    for (index, pattern) in patterns.lines().take(64).enumerate() {
        let Ok(parsed) = PathPattern::parse(pattern) else {
            assert!(tree.insert(pattern, index).is_err());
            continue;
        };
        let shape = parsed.shape();
        let inserted = tree.insert(pattern, index);
        assert_eq!(
            inserted.is_ok(),
            !shapes.contains(&shape),
            "{pattern}: same shape {shape}"
        );
        if inserted.is_ok() {
            shapes.push(shape);
            let catch_all = matches!(parsed.segments().last(), Some(Segment::CatchAll));
            router = router.route(Method::GET, pattern, move |params: PathParams| async move {
                for (_, value) in params.iter() {
                    assert!(!value.contains(['\0', '\\']), "{value:?}");
                    assert!(value != "." && value != "..", "{value:?}");
                    if catch_all {
                        assert!(!value.split('/').any(|part| part == "." || part == ".."));
                    }
                }
                "ok"
            });
        }
    }

    for path in paths.lines().take(64) {
        let checked = check_path(path);
        if checked.is_ok() {
            assert!(!path.contains('\\'));
            assert!(!path.split('/').any(|segment| segment == "." || segment == ".."));
        }
        let found = tree.at(path);
        if let Some(found) = &found {
            assert_eq!(rebuild(found.pattern, found.raw_values()), path, "rebuilt");
            let best = rank(found.pattern, path).expect("the matched pattern matches");
            for (pattern, _) in tree.iter() {
                if let Some(other) = rank(pattern, path) {
                    assert!(best <= other, "{} beats {} for {path}", pattern, found.pattern);
                }
            }
            let last = found.raw_values().len().saturating_sub(1);
            for (index, raw) in found.raw_values().iter().enumerate() {
                let catch_all = found.ends_in_catch_all() && index == last;
                if let Ok(value) = decode_param(raw, catch_all) {
                    assert!(!value.contains(['\0', '\\']));
                    assert!(catch_all || !value.contains('/'));
                }
            }
        }

        let Ok(uri) = path.parse::<http::Uri>() else {
            continue;
        };
        for method in [Method::GET, Method::HEAD, Method::POST, Method::OPTIONS] {
            let request = Request::builder()
                .method(method.clone())
                .uri(uri.clone())
                .body(axum::body::Body::empty())
                .expect("a request");
            let response = block_on(router.clone().oneshot(request)).expect("infallible");
            let status = response.status();
            if status.is_success() && method != Method::OPTIONS {
                assert!(
                    tree.at(uri.path()).is_some() && check_path(uri.path()).is_ok(),
                    "{method} {path} answered {status}"
                );
            }
            if method == Method::POST && tree.at(uri.path()).is_some() && check_path(uri.path()).is_ok() {
                assert!(
                    matches!(status, StatusCode::METHOD_NOT_ALLOWED | StatusCode::BAD_REQUEST),
                    "POST {path}: {status}"
                );
            }
        }
    }
});
