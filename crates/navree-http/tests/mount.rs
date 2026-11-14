//! Mount: any tower service under a path prefix, checked at boot with the
//! app's routes and other mounts.

#[path = "http/support.rs"]
mod support;

use std::future::Future;
use std::sync::Arc;

use http::Method;
use navree_http::body::Body;
use navree_http::extract::Request;
use navree_http::{AppRouter, ModuleBuilderMountExt, Mount, RouteDef};

use support::{Counter, Setup};

/// Harpreet Sandhu, signed in.
const HARPREET: &str = "0192f0c1-0000-7000-8000-0000000000a1";
/// Meera Sharma, signed in.
const MEERA: &str = "0192f0c1-0000-7000-8000-0000000000a2";

/// A request from `user` (the test authenticator reads `x-user-id`).
fn as_user(method: &str, path: &str, user: &str) -> http::Request<Body> {
    http::Request::builder()
        .method(method)
        .uri(path)
        .header("x-user-id", user)
        .body(Body::empty())
        .unwrap()
}

/// A service that counts its calls and answers `{method} {path and query}`.
fn counted(calls: &Arc<Counter>) -> AppRouter {
    let calls = calls.clone();
    AppRouter::from_handler(move |request: Request| {
        calls.hit();
        async move {
            let target = request
                .uri()
                .path_and_query()
                .map_or("", |target| target.as_str());
            format!("{} {target}", request.method())
        }
    })
}

/// The boot error of an app with a signed-in user's authenticator, `routes`
/// and one mount at `prefix` that `mount` declares.
fn mount_error(
    prefix: &'static str,
    mount: impl Fn(Mount) -> Mount + Send + Sync + 'static,
    routes: Vec<RouteDef>,
) -> impl Future<Output = String> {
    let calls = Arc::new(Counter::default());
    Setup::new(routes)
        .authenticated()
        .register(move |m| {
            m.mount(mount(Mount::new(prefix, counted(&calls))));
        })
        .boot_error()
}

#[tokio::test]
async fn a_mount_without_access_or_overlapping_a_route_fails_the_boot() {
    let error = mount_error("/khata", |mount| mount, vec![]).await;
    assert!(
        error.contains(
            "mount `/khata` in `TestModule` has no access declaration: add .public(), .authenticated(), .roles(..), .permissions(..) or .can(..) (deny by default)"
        ),
        "{error}"
    );

    let ok = || RouteDef::on(Method::GET, "!/khatas", || async { "other" }).public();
    let error = mount_error(
        "/khata",
        Mount::public,
        vec![
            RouteDef::on(Method::GET, "!/khata/status", || async { "ok" }).public(),
            RouteDef::on(Method::POST, "!/khata", || async { "ok" }).public(),
            ok(),
        ],
    )
    .await;
    assert!(
        error.contains(
            "mount `/khata` in `TestModule` overlaps the route GET /khata/status of `TestModule`; give the mount a prefix of its own"
        ),
        "{error}"
    );
    assert!(
        error.contains(
            "mount `/khata` in `TestModule` overlaps the route POST /khata of `TestModule`; give the mount a prefix of its own"
        ),
        "{error}"
    );
    assert!(
        !error.contains("/khatas"),
        "a longer segment is another path: {error}"
    );

    let error = mount_error("/khata", |mount| mount.public().roles(&["munshi"]), vec![]).await;
    assert!(
        error.contains(
            "mount `/khata` in `TestModule` is public but also declares public roles:munshi; remove one of them"
        ),
        "{error}"
    );
    let error = mount_error("/khata", |mount| mount.authenticated().methods(&[]), vec![]).await;
    assert!(
        error.contains(
            "mount `/khata` in `TestModule` takes no method: leave out .methods(..) for every method, or name at least one"
        ),
        "{error}"
    );

    // The same mount beside a route of another path boots.
    let calls = Arc::new(Counter::default());
    Setup::new(vec![ok()])
        .register(move |m| {
            m.mount(Mount::new("/khata", counted(&calls)).public());
        })
        .boot()
        .await;
}

#[tokio::test]
async fn a_mount_overlapping_a_parameter_or_catch_all_route_fails_the_boot() {
    for (pattern, shown, overlaps) in [
        ("!/{workspace}/dashboard", "/{workspace}/dashboard", true),
        ("!/{workspace}", "/{workspace}", true),
        ("!/{*path}", "/{*path}", true),
        ("!/admin/{*rest}", "/admin/{*rest}", true),
        ("!/files/{*path}", "/files/{*path}", false),
        ("!/adminx", "/adminx", false),
    ] {
        let calls = Arc::new(Counter::default());
        let setup = Setup::new(vec![
            RouteDef::on(Method::GET, pattern, || async { "ok" }).public(),
        ])
        .register(move |m| {
            m.mount(Mount::new("/admin", counted(&calls)).public());
        });
        if overlaps {
            let error = setup.boot_error().await;
            let expected = format!(
                "mount `/admin` in `TestModule` overlaps the route GET {shown} of `TestModule`; give the mount a prefix of its own"
            );
            assert!(error.contains(&expected), "{pattern}: {error}");
        } else {
            setup.boot().await;
        }
    }
}

#[tokio::test]
async fn a_mount_prefix_under_slash_underscore_is_refused() {
    for (prefix, detail) in [
        ("/_navree/khata", None),
        ("/_khata", None),
        ("khata", None),
        ("/khata/", None),
        ("/", None),
        ("/a//b", None),
        ("/a/../b", None),
        ("/a..b", None),
        ("/{ledger}", None),
        ("/khata*", None),
        (
            "/a b",
            Some("segment `a b` holds ` `, which a static segment may not"),
        ),
        (
            "/%6Bhata",
            Some("segment `%6Bhata` holds `%`, which a static segment may not"),
        ),
        ("/a/./b", Some("`/a/./b` has a `.` or `..` segment")),
        (
            "/:id",
            Some("segment `:id` uses old capture syntax; write `{param}` or `{*rest}`"),
        ),
    ] {
        let error = mount_error(prefix, Mount::public, vec![]).await;
        let detail = detail
            .map(|detail| format!(" ({detail})"))
            .unwrap_or_default();
        let expected = format!(
            "mount `{prefix}` in `TestModule`: a prefix starts with `/` but not with `/_`, has no `{{`, `}}`, `*`, `..` or `//`, and does not end with `/`{detail}"
        );
        assert!(error.contains(&expected), "{prefix}: {error}");
    }
}

#[tokio::test]
async fn two_mounts_at_or_under_one_prefix_fail_the_boot() {
    for (second, overlaps) in [
        ("/khata/reports", true),
        ("/khata", true),
        ("/khatas", false),
        ("/reports", false),
    ] {
        let calls = Arc::new(Counter::default());
        let setup = Setup::new(vec![]).register(move |m| {
            m.mount(Mount::new("/khata", counted(&calls)).public())
                .mount(Mount::new(second, counted(&calls)).public());
        });
        if overlaps {
            let error = setup.boot_error().await;
            let expected =
                format!("mounts `/khata` (TestModule) and `{second}` (TestModule) overlap");
            assert!(error.contains(&expected), "{second}: {error}");
        } else {
            setup.boot().await;
        }
    }
}
