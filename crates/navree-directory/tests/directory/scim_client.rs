//! The SCIM server driven by a SCIM client Navree did not write: the
//! `scim2` command of scim2-cli (Python, `pip install scim2-cli`), whose `test`
//! command reads the server's discovery documents and then runs a compliance
//! check as a provisioning client does: creates, reads, searches, replaces,
//! patches and deletes users and groups with values it generates, and reports
//! every deviation from RFC 7643 and RFC 7644.
//!
//! The other SCIM tests send the requests Microsoft Entra ID and Okta are
//! known to send, written from their documentation. This one sends what an
//! independent implementation of the standard sends. (Entra ID's own SCIM
//! validator and Okta's SCIM tester are hosted tools that need a public
//! address and an account: they stay the owner's to run.)
//!
//! This runs in the integration workflow, which installs scim2-cli and sets
//! `NAVREE_TEST_SCIM2_CLI` to the command; it is `#[ignore]`d in the other
//! suites. Run it with `-- --ignored`. A missing variable fails the test when
//! `NAVREE_TEST_REQUIRE_SERVICES=1` and prints `SKIPPED` otherwise.

use std::process::Command;

use navree_directory::Owner;

use crate::support::harness;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs scim2-cli, installed by the integrations workflow"]
async fn a_scim_compliance_client_finds_no_deviation() {
    let test = "a_scim_compliance_client_finds_no_deviation";
    let cli = match std::env::var("NAVREE_TEST_SCIM2_CLI") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: NAVREE_TEST_SCIM2_CLI is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            println!("SKIPPED {test}: set NAVREE_TEST_SCIM2_CLI");
            return;
        }
    };

    // The address first: the `location` of every resource the server writes
    // is built from it, and the client follows those.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("address").port();
    let h = harness(|builder| {
        builder
            .config("app.url", format!("http://127.0.0.1:{port}"))
            // A compliance run sends hundreds of requests from one address.
            .config("http.per_ip_rate_limit", serde_json::Value::Null)
    })
    .await;
    let token = h
        .scim(Owner::App)
        .await
        .scim_token
        .expect("a SCIM token")
        .expose()
        .clone();
    let router = navree_http::build_router(h.app.ctx())
        .await
        .expect("the router");
    let server = tokio::spawn(async move {
        let _ = navree_http::serve(listener, router).await;
    });

    let url = format!("http://127.0.0.1:{port}/scim/v2");
    let output = tokio::task::spawn_blocking(move || {
        Command::new(&cli)
            .arg("--url")
            .arg(&url)
            .arg("--header")
            .arg(format!("Authorization: Bearer {token}"))
            .args(["test", "-v"])
            .output()
    })
    .await
    .expect("the task ends")
    .expect("scim2 starts (pip install scim2-cli)");
    server.abort();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "scim2 test reported deviations ({}):\n{stdout}\n{stderr}",
        output.status
    );
    // The last line counts what passed, as in "300 passed (seed 2416347125)".
    assert!(stdout.contains(" passed"), "{stdout}\n{stderr}");
}
