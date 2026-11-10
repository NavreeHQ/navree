//! `DohResolver` (DNS over HTTPS, the JSON API) asking a real DoH server in
//! front of CoreDNS, which serves the zone `acme.test` from
//! `scripts/services/integrations/coredns/acme.test.zone`. The domain proof
//! of single sign-on reads its TXT record this way in production, from
//! Cloudflare or Google.
//!
//! These run in the integration workflow, which starts the servers with
//! `scripts/integration-services.sh start infra` (variable
//! `NAVREE_TEST_DOH_URL`); they are `#[ignore]`d in the other suites. Run them
//! with `-- --ignored`. A missing variable fails the test when
//! `NAVREE_TEST_REQUIRE_SERVICES=1` and prints `SKIPPED` otherwise.

use navree_http_client::{HttpClient, HttpClientOptions, SsrfPolicy};
use navree_sso::{DohResolver, TxtResolver};

fn resolver(test: &str) -> Option<DohResolver> {
    match std::env::var("NAVREE_TEST_DOH_URL") {
        Ok(url) if !url.trim().is_empty() => {
            let http = HttpClient::new(HttpClientOptions {
                retries: 0,
                ssrf: SsrfPolicy {
                    block_private: false,
                    ..SsrfPolicy::default()
                },
                ..HttpClientOptions::default()
            })
            .expect("a client");
            Some(DohResolver::new(http, url.trim()))
        }
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: NAVREE_TEST_DOH_URL is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            println!("SKIPPED {test}: set NAVREE_TEST_DOH_URL");
            None
        }
    }
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn txt_records_are_read_through_a_real_doh_server() {
    let Some(doh) = resolver("txt_records_are_read_through_a_real_doh_server") else {
        return;
    };

    assert_eq!(
        doh.txt("_navree-sso.acme.test").await.unwrap(),
        ["navree-sso-verification=a1b2c3d4e5f6"]
    );
    // A record of two strings is one value.
    assert_eq!(
        doh.txt("split.acme.test").await.unwrap(),
        ["part one, part two"]
    );
    // Two records at one name.
    let mut apex = doh.txt("acme.test").await.unwrap();
    apex.sort();
    assert_eq!(
        apex,
        [
            "google-site-verification=abc",
            "v=spf1 include:amazonses.com ~all"
        ]
    );
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_name_that_does_not_exist_has_no_records() {
    let Some(doh) = resolver("a_name_that_does_not_exist_has_no_records") else {
        return;
    };

    assert!(doh.txt("nobody.acme.test").await.unwrap().is_empty());
    // The name exists and has no TXT record.
    assert!(doh.txt("ns.acme.test").await.unwrap().is_empty());
}
