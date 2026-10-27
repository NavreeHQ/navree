//! Sending-domain checks: SPF, DKIM and DMARC.

use std::sync::Arc;

use navree_kernel::Navree;
use navree_mail::{
    CheckStatus, DkimSigner, DnsResolver, DomainExpectations, MailModule, SendingDomains,
    StaticResolver,
};

use crate::support::{array_options, view_module};

const RSA_KEY: &str = include_str!("../fixtures/dkim-rsa-2048.pem");

fn domains(dns: StaticResolver) -> SendingDomains {
    SendingDomains::new(Arc::new(dns))
}

fn ready_dns() -> StaticResolver {
    let signer = DkimSigner::from_pem("acme.test", "mail", RSA_KEY).expect("signer");
    StaticResolver::new()
        .txt("acme.test", "google-site-verification=abc")
        .txt("acme.test", "v=spf1 include:amazonses.com ~all")
        .txt("mail._domainkey.acme.test", &signer.dns_record())
        .txt(
            "_dmarc.acme.test",
            "v=DMARC1; p=reject; rua=mailto:dmarc@acme.test",
        )
}

fn expect() -> DomainExpectations {
    DomainExpectations::new()
        .selector("mail")
        .spf_include("amazonses.com")
}

#[tokio::test]
async fn a_ready_domain_passes_every_check() {
    let report = domains(ready_dns())
        .check("ACME.test.", &expect())
        .await
        .expect("check");

    assert_eq!(report.domain, "acme.test");
    assert_eq!(report.spf.status, CheckStatus::Pass, "{:?}", report.spf);
    assert_eq!(
        report.spf.record.as_deref(),
        Some("v=spf1 include:amazonses.com ~all")
    );
    assert_eq!(report.dkim.len(), 1);
    assert_eq!(report.dkim[0].selector, "mail");
    assert_eq!(report.dkim[0].check.status, CheckStatus::Pass, "{:?}", report.dkim);
    assert_eq!(report.dmarc.status, CheckStatus::Pass, "{:?}", report.dmarc);
    assert!(report.ready());
}

#[tokio::test]
async fn missing_records_fail() {
    let report = domains(StaticResolver::new())
        .check("acme.test", &expect())
        .await
        .expect("check");

    assert_eq!(report.spf.status, CheckStatus::Fail);
    assert_eq!(report.spf.problems, ["no SPF record (`v=spf1 ...`)"]);
    assert_eq!(report.dkim[0].check.status, CheckStatus::Fail);
    assert_eq!(report.dkim[0].check.name, "mail._domainkey.acme.test");
    assert_eq!(report.dmarc.status, CheckStatus::Fail);
    assert!(!report.ready());
}

#[tokio::test]
async fn spf_problems() {
    let check = |record: &'static str| async move {
        domains(StaticResolver::new().txt("acme.test", record))
            .check("acme.test", &DomainExpectations::new().spf_include("amazonses.com"))
            .await
            .expect("check")
            .spf
    };

    let missing = check("v=spf1 include:mailgun.org -all").await;
    assert_eq!(missing.status, CheckStatus::Fail);
    assert_eq!(missing.problems, ["the record does not include `include:amazonses.com`"]);

    let open = check("v=spf1 include:amazonses.com +all").await;
    assert_eq!(open.status, CheckStatus::Fail);
    assert!(open.problems[0].contains("anyone"));

    let neutral = check("v=spf1 include:amazonses.com ?all").await;
    assert_eq!(neutral.status, CheckStatus::Warn);

    let no_all = check("v=spf1 include:amazonses.com").await;
    assert_eq!(no_all.status, CheckStatus::Warn);

    let redirect = check("v=spf1 include:amazonses.com redirect=_spf.acme.test").await;
    assert_eq!(redirect.status, CheckStatus::Pass);

    let many = check(
        "v=spf1 include:amazonses.com include:a.test include:b.test include:c.test \
         include:d.test include:e.test include:f.test include:g.test a mx ptr -all",
    )
    .await;
    assert_eq!(many.status, CheckStatus::Fail);
    assert!(many.problems[0].contains("11 DNS lookups"), "{:?}", many.problems);

    let two = domains(
        StaticResolver::new()
            .txt("acme.test", "v=spf1 -all")
            .txt("acme.test", "v=spf1 include:amazonses.com -all"),
    )
    .check("acme.test", &DomainExpectations::new())
    .await
    .expect("check")
    .spf;
    assert_eq!(two.status, CheckStatus::Fail);
    assert!(two.problems[0].contains("more than one"));
}

#[tokio::test]
async fn dkim_problems() {
    let check = |record: String| async move {
        domains(StaticResolver::new().txt("s1._domainkey.acme.test", &record))
            .check("acme.test", &DomainExpectations::new().selector("s1"))
            .await
            .expect("check")
            .dkim
            .remove(0)
            .check
    };

    let revoked = check("v=DKIM1; k=rsa; p=".to_owned()).await;
    assert_eq!(revoked.status, CheckStatus::Fail);
    assert_eq!(revoked.problems, ["the key is revoked (empty `p=`)"]);

    // 96 bytes: the size of a 512-bit key.
    let short = check(format!("v=DKIM1; k=rsa; p={}", "QUFB".repeat(32))).await;
    assert_eq!(short.status, CheckStatus::Warn, "{:?}", short.problems);

    let garbage = check("v=DKIM1; p=not base64!".to_owned()).await;
    assert_eq!(garbage.status, CheckStatus::Fail);

    let ed25519 = check(
        "v=DKIM1; k=ed25519; p=11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=".to_owned(),
    )
    .await;
    assert_eq!(ed25519.status, CheckStatus::Pass, "{:?}", ed25519.problems);
}

#[tokio::test]
async fn dmarc_problems() {
    let check = |record: &'static str| async move {
        domains(StaticResolver::new().txt("_dmarc.acme.test", record))
            .check("acme.test", &DomainExpectations::new())
            .await
            .expect("check")
            .dmarc
    };

    let monitoring = check("v=DMARC1; p=none; rua=mailto:d@acme.test").await;
    assert_eq!(monitoring.status, CheckStatus::Warn);
    assert!(monitoring.problems[0].contains("p=none"));

    let partial = check("v=DMARC1; p=quarantine; pct=50; rua=mailto:d@acme.test").await;
    assert_eq!(partial.status, CheckStatus::Warn);

    let no_reports = check("v=DMARC1; p=reject").await;
    assert_eq!(no_reports.status, CheckStatus::Warn);
    assert!(no_reports.problems[0].contains("rua="));

    let no_policy = check("v=DMARC1; rua=mailto:d@acme.test").await;
    assert_eq!(no_policy.status, CheckStatus::Fail);
}

#[tokio::test]
async fn a_name_without_a_dot_is_refused() {
    let error = domains(StaticResolver::new())
        .check("localhost", &DomainExpectations::new())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not a domain name"), "{error}");
}

#[tokio::test]
async fn the_module_provides_sending_domains_with_the_apps_resolver() {
    let app = Navree::app()
        .module(view_module())
        .module(MailModule::for_root(array_options()))
        .override_value::<dyn DnsResolver>(Arc::new(ready_dns()))
        .boot()
        .await
        .expect("boot");

    let report = app
        .ctx()
        .get::<SendingDomains>()
        .await
        .expect("service")
        .check("acme.test", &expect())
        .await
        .expect("check");

    assert!(report.ready());
}
