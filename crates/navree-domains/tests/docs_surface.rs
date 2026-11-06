//! Every public item `docs/packages/custom-domains` names (types, fields,
//! methods, configuration keys and their defaults, events and their names,
//! tables, routes, commands, codes and the fakes' helpers), so a rename
//! fails the build or this test. The behavior is exercised in
//! `tests/domains`; the page's examples run in the facade's
//! `tests/domains_docs.rs`.

use std::net::SocketAddr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_contracts::Secret;
use navree_domains::{
    AcmeClient, AcmeFake, AcmeOptions, AcmeOrder, AskQuery, CertificateOptions, CertificateProbe,
    CertificateRun, CloudflareDns, CloudflareOptions, CurrentDomain, CustomDomain,
    DOMAINS_MIGRATIONS, DnsFake, DnsInstruction, DnsLookup, DnsOptions, DnsProvider,
    DnsProviderFake, DomainAdded, DomainCertificateExpiring, DomainCertificateIssued,
    DomainCertificates, DomainClaimedElsewhere, DomainRemoved, DomainStatus,
    DomainVerificationFailed, DomainVerified, DomainWorkspace, Domains, DomainsController,
    DomainsError, DomainsModule, DomainsOptions, DomainsPublicController, HttpsServer, InstantAcme,
    IssuedCertificate, LETS_ENCRYPT, LETS_ENCRYPT_STAGING, MANAGE_PERMISSION, NewDomain, ProbeFake,
    ProbedCertificate, RecordKind, SystemDns, TlsMode, TlsProbe, TlsStatus, VerificationOptions,
    models, normalize_host, request_host,
};
use navree_events::Event;
use navree_kernel::Module;
use navree_orm::Model as _;
use uuid::Uuid;

#[test]
fn the_configuration_keys_have_their_documented_defaults() {
    let DomainsOptions {
        routes,
        tls,
        ask_path,
        target,
        addresses,
        reserved,
        max_per_workspace,
        cache_secs,
        verification,
        certificates,
        acme,
        dns,
    } = DomainsOptions::default();
    assert!(routes);
    assert_eq!(tls, TlsMode::OnDemand);
    assert_eq!(ask_path, "/_domains/allow");
    assert_eq!(target, None);
    assert!(addresses.is_empty() && reserved.is_empty());
    assert_eq!(max_per_workspace, 10);
    assert_eq!(cache_secs, 60);
    let VerificationOptions {
        prefix,
        interval_mins,
        give_up_after_hours,
        failed_retention_hours,
        recheck_hours,
        require_routing,
    } = verification;
    assert_eq!(prefix, "_domain-verification");
    assert_eq!(interval_mins, 5);
    assert_eq!(give_up_after_hours, 72);
    assert_eq!(failed_retention_hours, 168);
    assert_eq!(recheck_hours, 24);
    assert!(require_routing);
    let CertificateOptions {
        warn_days,
        check_hours,
        probe,
    } = certificates;
    assert_eq!(warn_days, 14);
    assert_eq!(check_hours, 24);
    assert!(probe);
    let AcmeOptions {
        directory_url,
        contact,
        terms_agreed,
        https_addr,
        renew_days,
        hosts,
        ca_file,
        reload_secs,
    } = acme;
    assert_eq!(directory_url, LETS_ENCRYPT);
    assert_eq!(
        LETS_ENCRYPT,
        "https://acme-v02.api.letsencrypt.org/directory"
    );
    assert_eq!(
        LETS_ENCRYPT_STAGING,
        "https://acme-staging-v02.api.letsencrypt.org/directory"
    );
    assert!(contact.is_empty() && hosts.is_empty());
    assert!(!terms_agreed);
    assert_eq!(https_addr, None);
    assert_eq!(renew_days, 30);
    assert_eq!(ca_file, None);
    assert_eq!(reload_secs, 60);
    let DnsOptions {
        provider,
        zone,
        cloudflare,
    } = dns;
    assert_eq!(provider, None);
    assert_eq!(zone, None);
    let CloudflareOptions {
        api_token,
        zone_id,
        api_url,
        proxied,
        ttl,
    } = cloudflare;
    assert_eq!(api_token, None);
    assert_eq!(zone_id, None);
    assert_eq!(api_url, "https://api.cloudflare.com/client/v4");
    assert!(!proxied);
    assert_eq!(ttl, 1);
    for (mode, name) in [
        (TlsMode::OnDemand, "on_demand"),
        (TlsMode::Acme, "acme"),
        (TlsMode::Off, "off"),
    ] {
        assert_eq!(mode.as_str(), name);
    }
}

#[test]
fn statuses_events_codes_and_names() {
    for (status, name) in [
        (DomainStatus::Pending, "pending"),
        (DomainStatus::Verified, "verified"),
        (DomainStatus::Failed, "failed"),
    ] {
        assert_eq!(status.as_str(), name);
        assert_eq!(DomainStatus::parse(name), status);
    }
    for (status, name) in [
        (TlsStatus::Pending, "pending"),
        (TlsStatus::Active, "active"),
        (TlsStatus::Failed, "failed"),
        (TlsStatus::Off, "off"),
    ] {
        assert_eq!(status.as_str(), name);
        assert_eq!(TlsStatus::parse(name), status);
    }
    assert_eq!(DomainAdded::NAME, "domains.added");
    assert_eq!(DomainVerified::NAME, "domains.verified");
    assert_eq!(
        DomainVerificationFailed::NAME,
        "domains.verification_failed"
    );
    assert_eq!(DomainClaimedElsewhere::NAME, "domains.claimed_elsewhere");
    assert_eq!(DomainRemoved::NAME, "domains.removed");
    assert_eq!(DomainCertificateIssued::NAME, "domains.certificate_issued");
    assert_eq!(
        DomainCertificateExpiring::NAME,
        "domains.certificate_expiring"
    );
    assert_eq!(MANAGE_PERMISSION, "domains.manage");
    assert_eq!(DOMAINS_MIGRATIONS, "domains");
    for (kind, name) in [
        (RecordKind::A, "A"),
        (RecordKind::Aaaa, "AAAA"),
        (RecordKind::Cname, "CNAME"),
        (RecordKind::Txt, "TXT"),
    ] {
        assert_eq!(kind.as_str(), name);
    }
    let codes = [
        (DomainsError::InvalidDomain("x".into()), "invalid_domain"),
        (DomainsError::NotAllowed("x".into()), "domain_not_allowed"),
        (DomainsError::DomainNotFound, "domain_not_found"),
        (DomainsError::Taken("x".into()), "domain_taken"),
        (DomainsError::LimitReached(10), "domain_limit_reached"),
        (DomainsError::NotVerified("x".into()), "domain_not_verified"),
        (DomainsError::dns("x", "y"), "dns_lookup_failed"),
        (DomainsError::DnsProvider("x".into()), "dns_provider_failed"),
        (DomainsError::acme("x", "y"), "certificate_not_issued"),
    ];
    for (error, code) in codes {
        assert_eq!(error.into_app_error().code, code);
    }
}

#[test]
fn the_public_shapes_have_their_documented_fields() {
    let now: DateTime<Utc> = Utc::now();
    let id = Uuid::now_v7();
    let domain = CustomDomain {
        id,
        workspace_id: id,
        host: "crm.acme-corp.com".into(),
        purpose: "app".into(),
        status: DomainStatus::Pending,
        primary: false,
        managed: false,
        points_to_app: None,
        tls: TlsStatus::Pending,
        certificate_expires_at: None,
        tls_error: None,
        last_error: None,
        last_checked_at: None,
        next_check_at: Some(now),
        verified_at: None,
        dns_records: vec![DnsInstruction {
            kind: "TXT".into(),
            name: "_domain-verification.crm.acme-corp.com".into(),
            value: "token".into(),
            purpose: "verification".into(),
        }],
        created_by: None,
        created_at: now,
        updated_at: now,
    };
    let json = serde_json::to_value(&domain).unwrap();
    for key in [
        "id",
        "workspace_id",
        "host",
        "purpose",
        "status",
        "primary",
        "managed",
        "points_to_app",
        "tls",
        "certificate_expires_at",
        "tls_error",
        "last_error",
        "last_checked_at",
        "next_check_at",
        "verified_at",
        "dns_records",
        "created_by",
        "created_at",
        "updated_at",
    ] {
        assert!(json.get(key).is_some(), "{key}");
    }
    assert_eq!(json["dns_records"][0]["type"], "TXT");
    assert!(!domain.is_verified());
    let NewDomain {
        host,
        purpose,
        created_by,
    } = NewDomain::new("crm.acme-corp.com")
        .purpose("links")
        .created_by(id);
    assert_eq!(
        (host.as_str(), purpose.as_deref(), created_by),
        ("crm.acme-corp.com", Some("links"), Some(id))
    );
    let CurrentDomain {
        id: _,
        workspace_id: _,
        host: _,
        purpose: _,
    } = CurrentDomain {
        id,
        workspace_id: id,
        host: "crm.acme-corp.com".into(),
        purpose: "app".into(),
    };
    let DomainWorkspace {
        workspace_id: _,
        host: _,
    } = DomainWorkspace {
        workspace_id: id,
        host: "crm.acme-corp.com".into(),
    };
    let CertificateRun {
        issued,
        renewed,
        probed,
        failed,
        expiring,
    } = CertificateRun::default();
    assert!(issued.is_empty() && renewed.is_empty() && probed.is_empty());
    assert!(failed.is_empty() && expiring.is_empty());
    let ProbedCertificate {
        not_before: _,
        not_after: _,
    } = ProbedCertificate {
        not_before: now,
        not_after: now,
    };
    let IssuedCertificate {
        chain_pem: _,
        private_key_pem: _,
    } = IssuedCertificate {
        chain_pem: String::new(),
        private_key_pem: String::new(),
    };
    let AskQuery { domain: _ } = AskQuery::default();
    let event = DomainCertificateExpiring {
        host: "crm.acme-corp.com".into(),
        domain_id: Some(id),
        expires_at: now,
        days_left: 9,
    };
    let _ = (
        DomainAdded {
            domain: domain.clone(),
        },
        DomainVerified {
            domain: domain.clone(),
        },
        DomainClaimedElsewhere {
            domain: domain.clone(),
            claimed_by: id,
        },
        DomainVerificationFailed {
            domain,
            reason: String::new(),
        },
        DomainRemoved {
            domain_id: id,
            workspace_id: id,
            host: String::new(),
        },
        DomainCertificateIssued {
            host: String::new(),
            domain_id: None,
            expires_at: now,
            renewal: false,
        },
        event,
    );
}

#[test]
fn hosts_tables_and_models() {
    assert_eq!(
        normalize_host("CRM.Acme-Corp.com.").unwrap(),
        "crm.acme-corp.com"
    );
    assert_eq!(
        normalize_host("bücher.example").unwrap(),
        "xn--bcher-kva.example"
    );
    let (parts, ()) = http::Request::builder()
        .header("host", "crm.acme-corp.com:443")
        .body(())
        .unwrap()
        .into_parts();
    assert_eq!(request_host(&parts).as_deref(), Some("crm.acme-corp.com"));
    assert_eq!(models::Domain::table_name(), "custom_domains");
    assert_eq!(
        models::Certificate::table_name(),
        "custom_domain_certificates"
    );
    assert_eq!(models::Challenge::table_name(), "custom_domain_challenges");
    assert_eq!(
        models::AcmeAccount::table_name(),
        "custom_domain_acme_accounts"
    );
}

#[tokio::test]
async fn the_drivers_fakes_and_services_exist() {
    // The drivers, as their traits.
    let _: Arc<dyn DnsLookup> = Arc::new(SystemDns::new());
    let _: Arc<dyn AcmeClient> = Arc::new(InstantAcme::new().unwrap());
    let _: Arc<dyn CertificateProbe> = Arc::new(
        TlsProbe::new()
            .port(8443)
            .timeout(std::time::Duration::from_secs(1))
            .connect_to("x.test", SocketAddr::from(([127, 0, 0, 1], 9))),
    );
    let _: Arc<dyn DnsProvider> = Arc::new(
        CloudflareDns::new(&CloudflareOptions {
            api_token: Some(Secret::new("t".to_owned())),
            zone_id: Some("z".into()),
            ..CloudflareOptions::default()
        })
        .unwrap(),
    );
    // The fakes' helpers.
    let dns = DnsFake::new()
        .with_txt("a.test", "v")
        .with_cname("b.test", "c.test")
        .with_address("c.test", "203.0.113.10");
    dns.set_txt("a.test", "w");
    dns.set_cname("d.test", "c.test");
    dns.set_address("e.test", "203.0.113.11");
    dns.remove("e.test");
    dns.fail("f.test", "SERVFAIL");
    assert!(dns.lookups().is_empty());
    let ca = AcmeFake::new();
    assert!(ca.ca_pem().contains("BEGIN CERTIFICATE"));
    ca.expire_at(Utc::now());
    ca.fail_next("no");
    ca.keep_challenges();
    assert!(ca.orders().is_empty() && ca.challenges().is_empty());
    let probe = ProbeFake::new();
    probe.expires("a.test", Utc::now());
    probe.fail("b.test", "no");
    assert!(probe.probed().is_empty());
    let zone = DnsProviderFake::new();
    zone.fail_next("no");
    assert!(zone.records(RecordKind::Cname, "a.test").is_empty() && zone.all().is_empty());
    // The certificate store.
    let certificates = Arc::new(DomainCertificates::new());
    assert!(certificates.hosts().is_empty());
    assert!(!certificates.contains("a.test"));
    assert_eq!(certificates.expires_at("a.test"), None);
    certificates.server_config().unwrap();
    // The module and the routes.
    let _ = DomainsModule::for_root(DomainsOptions::default());
    let _ = DomainsModule::from_config();
    let _ = DomainsModule.id();
    let _ = std::any::type_name::<(DomainsController, DomainsPublicController)>();
    let _ = std::any::type_name::<(Domains, AcmeOrder, HttpsServer)>();
    // The service's methods the page names.
    let _ = (
        Domains::add,
        Domains::list,
        Domains::all,
        Domains::get,
        Domains::get_for,
        Domains::find,
        Domains::claims,
        Domains::transfer,
        Domains::resolve,
        Domains::allows,
        Domains::verify,
        Domains::verify_due,
        Domains::make_primary,
        Domains::primary,
        Domains::primary_url,
        Domains::remove,
        Domains::remove_workspace,
    );
    let _ = (
        Domains::issue_certificate,
        Domains::issue_pending,
        Domains::check_certificates,
        Domains::reload_certificates,
        Domains::challenge_answer,
        Domains::certificates,
        Domains::dns_records,
        Domains::options,
        Domains::resolves_workspaces,
    );
    let _ = (
        AcmeOrder::host,
        AcmeOrder::directory_url,
        AcmeOrder::contact,
        AcmeOrder::account,
        AcmeOrder::save_account,
        AcmeOrder::publish_challenge,
        AcmeOrder::withdraw_challenge,
    );
    let _ = (
        HttpsServer::bind,
        HttpsServer::local_addr,
        HttpsServer::serve,
    );
    let _ = (InstantAcme::with_root_pem, InstantAcme::timeout);
}
