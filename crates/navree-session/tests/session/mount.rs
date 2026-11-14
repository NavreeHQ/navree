//! A request to a mount passes the session and its CSRF check first, as a
//! route's does.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use navree_http::extract::Request;
use navree_http::{AppRouter, ModuleBuilderMountExt, Mount};
use navree_kernel::{Module, ModuleBuilder};
use navree_session::{SessionModule, SessionOptions};

use crate::support;

/// Mounts a ledger service at `/khata` that counts its calls and answers its
/// method and path.
struct LedgerModule(Arc<AtomicUsize>);

impl Module for LedgerModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let calls = self.0.clone();
        m.mount(
            Mount::new(
                "/khata",
                AppRouter::from_handler(move |request: Request| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    async move { format!("{} {}", request.method(), request.uri().path()) }
                }),
            )
            .public(),
        );
    }
}

#[tokio::test]
async fn a_mount_takes_the_session_and_its_csrf_check_like_a_route() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = support::base()
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(LedgerModule(calls.clone()))
        .build()
        .await;
    let client = app.client();
    client.csrf_cookie().await;
    client
        .post("/khata/entries")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    assert_eq!(calls.load(Ordering::SeqCst), 0, "the service never saw the post");
    let posted = client
        .post("/khata/entries")
        .xsrf()
        .send()
        .await
        .assert_status(200);
    assert_eq!(posted.text(), "POST /entries");
    let read = client
        .get("/khata/entries")
        .send()
        .await
        .assert_status(200);
    assert_eq!(read.text(), "GET /entries");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
