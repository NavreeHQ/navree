//! The sending-domain checks (SPF, DKIM, DMARC) with `SystemResolver` asking
//! a real DNS server: CoreDNS serving the zone `acme.test` from
//! `scripts/services/integrations/coredns/acme.test.zone`, whose DKIM key is a
//! 2048-bit RSA key longer than the 255 bytes one DNS string holds.
//!
//! These run in the integration workflow, which starts CoreDNS with
//! `scripts/integration-services.sh start infra` (variable
//! `NAVREE_TEST_DNS_SERVER`, a socket address); they are `#[ignore]`d in the
//! other suites. Run them with `-- --ignored`. A missing variable fails the
//! test when `NAVREE_TEST_REQUIRE_SERVICES=1` and prints `SKIPPED` otherwise.

use std::net::SocketAddr;
use std::sync::Arc;

use navree_mail::{
    CheckStatus, DkimSigner, DnsResolver, DomainExpectations, MailError, SendingDomains,
    SystemResolver,
};

use crate::mailpit::service;

const RSA_KEY: &str = include_str!("../fixtures/dkim-rsa-2048.pem");

fn resolver(test: &str) -> Option<SystemResolver> {
    let address: SocketAddr = service("NAVREE_TEST_DNS_SERVER", test)?
        .trim()
        .parse()
        .unwrap_or_else(|error| panic!("{test}: NAVREE_TEST_DNS_SERVER: {error}"));
    Some(SystemResolver::with_nameserver(address))
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_domain_with_spf_dkim_and_dmarc_passes_against_a_real_name_server() {
    let test = "a_domain_with_spf_dkim_and_dmarc_passes_against_a_real_name_server";
    let Some(dns) = resolver(test) else {
        return;
    };
    let signer = DkimSigner::from_pem("acme.test", "mail", RSA_KEY).expect("signer");
    let domains = SendingDomains::new(Arc::new(dns));

    let report = domains
        .check(
            "ACME.test.",
            &DomainExpectations::new()
                .selector("mail")
                .spf_include("amazonses.com"),
        )
        .await
        .expect("the name server answers");

    assert_eq!(report.spf.status, CheckStatus::Pass, "{:?}", report.spf);
    assert_eq!(
        report.spf.record.as_deref(),
        Some("v=spf1 include:amazonses.com ~all"),
        "only the SPF record of the two at the apex"
    );
    assert_eq!(
        report.dkim[0].check.status,
        CheckStatus::Pass,
        "{:?}",
        report.dkim
    );
    // The key is published as two DNS strings and read back as one value.
    assert_eq!(
        report.dkim[0].check.record.as_deref(),
        Some(signer.dns_record().as_str())
    );
    assert_eq!(report.dmarc.status, CheckStatus::Pass, "{:?}", report.dmarc);
    assert!(report.ready());
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_selector_nobody_published_fails_and_a_zone_nobody_serves_is_an_error() {
    let test = "a_selector_nobody_published_fails_and_a_zone_nobody_serves_is_an_error";
    let Some(dns) = resolver(test) else {
        return;
    };
    let domains = SendingDomains::new(Arc::new(dns));

    let report = domains
        .check(
            "acme.test",
            &DomainExpectations::new()
                .selector("other")
                .spf_include("sendgrid.net"),
        )
        .await
        .expect("the name server answers");

    assert_eq!(report.dkim[0].check.status, CheckStatus::Fail);
    assert_eq!(report.dkim[0].check.name, "other._domainkey.acme.test");
    assert_eq!(report.spf.status, CheckStatus::Fail, "{:?}", report.spf);
    assert!(!report.ready());

    // CoreDNS refuses zones it does not serve: a failure of the resolver,
    // which is not the same as a record that is missing.
    let error = domains
        .check("unserved.test", &DomainExpectations::new())
        .await
        .expect_err("the server refuses the zone");
    assert!(matches!(error, MailError::Dns { .. }), "{error}");
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn txt_lookups_join_the_strings_of_a_record() {
    let test = "txt_lookups_join_the_strings_of_a_record";
    let Some(dns) = resolver(test) else {
        return;
    };

    assert_eq!(
        dns.txt("split.acme.test").await.unwrap(),
        ["part one, part two"]
    );
    assert!(dns.txt("nobody.acme.test").await.unwrap().is_empty());
}
