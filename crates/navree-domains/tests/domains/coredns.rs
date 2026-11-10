//! `SystemDns` (the hickory resolver) asking a real DNS server: CoreDNS
//! serving the zone `acme.test` from
//! `scripts/services/integrations/coredns/acme.test.zone`. TXT, CNAME and
//! address lookups, a TXT record of two strings, a name with no records, a
//! name that does not exist, and a name the server will not answer for.
//!
//! These run in the integration workflow, which starts CoreDNS with
//! `scripts/integration-services.sh start infra` (variable
//! `NAVREE_TEST_DNS_SERVER`, a socket address); they are `#[ignore]`d in the
//! other suites. Run them with `-- --ignored`. A missing variable fails the
//! test when `NAVREE_TEST_REQUIRE_SERVICES=1` and prints `SKIPPED` otherwise.
//!
//! The domain checks of `verification.rs` keep using `DnsFake`: the token a
//! claim needs is made when the domain is added, so a static zone cannot hold
//! it.

use std::net::{IpAddr, SocketAddr};

use navree_domains::{DnsLookup, DomainsError, SystemDns};

fn server(test: &str) -> Option<SystemDns> {
    match std::env::var("NAVREE_TEST_DNS_SERVER") {
        Ok(value) if !value.trim().is_empty() => {
            let address: SocketAddr = value
                .trim()
                .parse()
                .unwrap_or_else(|error| panic!("{test}: NAVREE_TEST_DNS_SERVER: {error}"));
            Some(SystemDns::with_nameserver(address))
        }
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: NAVREE_TEST_DNS_SERVER is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            println!("SKIPPED {test}: set NAVREE_TEST_DNS_SERVER");
            None
        }
    }
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn txt_records_are_read_from_a_real_name_server() {
    let Some(dns) = server("txt_records_are_read_from_a_real_name_server") else {
        return;
    };

    assert_eq!(
        dns.txt("_domain-verification.crm.acme.test").await.unwrap(),
        ["navree-verification=4f9c2b7d"]
    );
    // Case and a trailing dot do not matter to DNS.
    assert_eq!(
        dns.txt("_Domain-Verification.CRM.acme.test.")
            .await
            .unwrap(),
        ["navree-verification=4f9c2b7d"]
    );
    // A record of two strings is one value.
    assert_eq!(
        dns.txt("split.acme.test").await.unwrap(),
        ["part one, part two"]
    );
    // Two records at one name.
    let mut apex = dns.txt("acme.test").await.unwrap();
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
async fn a_name_with_no_such_record_or_no_such_name_is_an_empty_answer() {
    let Some(dns) = server("a_name_with_no_such_record_or_no_such_name_is_an_empty_answer") else {
        return;
    };

    // The name exists and has no TXT record (NODATA).
    assert!(dns.txt("ns.acme.test").await.unwrap().is_empty());
    // The name does not exist (NXDOMAIN).
    assert!(dns.txt("nobody.acme.test").await.unwrap().is_empty());
    assert_eq!(dns.cname("nobody.acme.test").await.unwrap(), None);
    assert!(dns.addresses("nobody.acme.test").await.unwrap().is_empty());
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_cname_and_the_addresses_behind_it_are_followed() {
    let Some(dns) = server("a_cname_and_the_addresses_behind_it_are_followed") else {
        return;
    };

    assert_eq!(
        dns.cname("crm.acme.test").await.unwrap().as_deref(),
        Some("domains.acme.test")
    );
    assert_eq!(dns.cname("domains.acme.test").await.unwrap(), None);
    let mut addresses = dns.addresses("crm.acme.test").await.unwrap();
    addresses.sort();
    assert_eq!(
        addresses,
        [
            "203.0.113.10".parse::<IpAddr>().unwrap(),
            "2001:db8::10".parse::<IpAddr>().unwrap(),
        ]
    );
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_server_that_refuses_a_zone_is_a_resolver_failure_not_an_empty_answer() {
    let Some(dns) =
        server("a_server_that_refuses_a_zone_is_a_resolver_failure_not_an_empty_answer")
    else {
        return;
    };

    // CoreDNS serves acme.test only and refuses everything else: that is not
    // "the record is not there", and a domain must not be verified or failed
    // on it.
    let error = dns
        .txt("_domain-verification.crm.unserved.test")
        .await
        .expect_err("the server refuses");
    assert!(matches!(error, DomainsError::Dns { .. }), "{error}");
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_server_nobody_answers_is_a_resolver_failure() {
    let Some(_) = server("a_server_nobody_answers_is_a_resolver_failure") else {
        return;
    };
    // Nothing listens on this port of the loopback interface.
    let dead = SystemDns::with_nameserver("127.0.0.1:1".parse().unwrap());

    let error = dead
        .txt("_domain-verification.crm.acme.test")
        .await
        .expect_err("nobody answers");

    assert!(matches!(error, DomainsError::Dns { .. }), "{error}");
}
