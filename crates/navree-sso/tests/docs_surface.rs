//! Every public item `docs/security/single-sign-on` names (types, fields,
//! methods, configuration keys, events, routes and fake helpers), so a
//! rename fails the build or this test. The flows run in `tests/sso`; the
//! page's examples run in the facade's `tests/sso_docs.rs`.

use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_auth::{ClientMeta, TokenPair, User};
use navree_db::Uow;
use navree_events::Event;
use navree_sso::saml::{
    AuthnRequest, IdpMetadata, RequestOptions, ResponseCheck, SamlAssertion, SamlError,
    ServiceProvider, validate_response,
};
use navree_sso::{
    AttributeMapping, ConnectionSettings, ConnectionUpdate, DohResolver, DomainOptions,
    FakeResponse, MANAGE_PERMISSION, NewConnection, OidcCallback, OidcOptions, OidcSettings,
    Protocol, Provisioning, RoleRule, SSO_MIGRATIONS, SamlOptions, SamlSettings,
    ServiceProviderInfo, SignatureError, Sso, SsoConnection, SsoDiscovery, SsoDomain,
    SsoDomainVerified, SsoError, SsoFake, SsoModule, SsoOptions, SsoOwner, SsoProfile,
    SsoRedirect, SsoSignIn, SsoUserProvisioned, StaticTxtResolver, TxtResolver, XmlError,
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn the_documented_options_and_their_defaults() {
    let SsoOptions {
        base_url,
        complete_url,
        enforce_for_verified_domains,
        routes,
        management_routes,
        routes_prefix,
        state_ttl_secs,
        code_ttl_secs,
        domains,
        saml,
        oidc,
    } = SsoOptions::default();
    assert_eq!((base_url, complete_url), (None, None));
    assert!(!enforce_for_verified_domains && routes && management_routes);
    assert_eq!(routes_prefix, "/sso");
    assert_eq!((state_ttl_secs, code_ttl_secs), (600, 60));
    let DomainOptions {
        txt_prefix,
        doh_url,
    } = domains;
    assert_eq!(txt_prefix, "navree-sso-verification");
    assert_eq!(doh_url, "https://cloudflare-dns.com/dns-query");
    let SamlOptions {
        allow_idp_initiated,
        clock_skew_secs,
        metadata_ttl_secs,
        max_response_bytes,
    } = saml;
    assert!(!allow_idp_initiated);
    assert_eq!(
        (clock_skew_secs, metadata_ttl_secs, max_response_bytes),
        (120, 3600, 1_048_576)
    );
    let OidcOptions {
        scopes,
        clock_skew_secs,
    } = oidc;
    assert_eq!(scopes, ["openid", "email", "profile"]);
    assert_eq!(clock_skew_secs, 60);
    let _ = SsoModule::for_root(SsoOptions::default());
    let _ = SsoModule::from_config();
    assert_eq!(SSO_MIGRATIONS, "sso");
    assert_eq!(MANAGE_PERMISSION, "sso.manage");
}

#[test]
fn the_documented_connection_types() {
    let NewConnection {
        owner,
        name,
        slug,
        settings,
        provisioning,
        enforce,
        enabled,
    } = NewConnection::oidc(SsoOwner::App, "Acme", "https://login.acme.test", "c", "s")
        .slug("acme")
        .enforce(true)
        .provisioning(Provisioning::default());
    assert_eq!(owner, SsoOwner::App);
    assert_eq!((name.as_str(), slug.as_deref()), ("Acme", Some("acme")));
    assert_eq!((enforce, enabled), (Some(true), true));
    let ConnectionSettings::Oidc(OidcSettings {
        issuer,
        client_id,
        client_secret,
        scopes,
    }) = settings
    else {
        panic!("an OpenID Connect connection")
    };
    assert_eq!((issuer.as_str(), client_id.as_str()), ("https://login.acme.test", "c"));
    assert_eq!((client_secret.as_deref(), scopes.len()), (Some("s"), 0));

    let SamlSettings {
        metadata_url,
        metadata_xml,
        name_id_format,
        force_authn,
        require_signed_assertion,
    } = SamlSettings::default();
    assert!(metadata_url.is_none() && metadata_xml.is_none() && name_id_format.is_none());
    assert!(!force_authn && !require_signed_assertion);

    let Provisioning {
        jit,
        default_role,
        roles,
        sync_roles,
        allow_unverified_emails,
        attributes,
    } = provisioning;
    assert!(jit && sync_roles && !allow_unverified_emails && roles.is_empty());
    assert_eq!(default_role.as_deref(), Some("member"));
    let AttributeMapping {
        email,
        name,
        first_name,
        last_name,
        groups,
    } = attributes;
    for list in [&email, &name, &first_name, &last_name, &groups] {
        assert!(!list.is_empty());
    }
    let _ = RoleRule {
        group: "Acme Admins".to_owned(),
        role: "admin".to_owned(),
    };
    let ConnectionUpdate {
        name: _,
        slug: _,
        settings: _,
        provisioning: _,
        enabled: _,
        enforce: _,
        follow_default_enforcement: _,
    } = ConnectionUpdate::default();
    assert_eq!(
        (SsoOwner::Organization(Uuid::nil()).kind(), Protocol::Saml.as_str()),
        ("organization", "saml")
    );
    let _ = NewConnection::saml(SsoOwner::Workspace(Uuid::nil()), "Acme", "<xml/>");
}

/// The resources the routes answer, by their fields.
#[allow(dead_code)]
fn the_documented_resources(
    connection: SsoConnection,
    domain: SsoDomain,
    discovery: SsoDiscovery,
    redirect: SsoRedirect,
    sign_in: SsoSignIn,
    callback: OidcCallback,
    profile: SsoProfile,
) {
    let SsoConnection {
        resource_type: _,
        id: _,
        owner: _,
        slug: _,
        name: _,
        protocol: _,
        enabled: _,
        enforce: _,
        settings: _,
        has_client_secret: _,
        provisioning: _,
        service_provider,
        created_at: _,
        updated_at: _,
    } = connection;
    let ServiceProviderInfo {
        login_url: _,
        entity_id: _,
        acs_url: _,
        metadata_url: _,
        redirect_uri: _,
    } = service_provider;
    let SsoDomain {
        resource_type: _,
        id: _,
        connection_id: _,
        domain: _,
        verified: _,
        verified_at: _,
        txt_name: _,
        txt_value: _,
        created_at: _,
    } = domain;
    let SsoDiscovery {
        sso: _,
        connection: _,
        name: _,
        protocol: _,
        login_url: _,
        password_allowed: _,
    } = discovery;
    let SsoRedirect { url: _, state: _ } = redirect;
    let SsoSignIn {
        user: _,
        connection_id: _,
        return_to: _,
        created: _,
        state: _,
    } = sign_in;
    let OidcCallback {
        code: _,
        state: _,
        error: _,
        error_description: _,
    } = callback;
    let SsoProfile {
        subject: _,
        email: _,
        email_verified: _,
        name: _,
        groups: _,
        attributes: _,
    } = profile;
}

/// The service's documented methods, by their signatures.
#[allow(dead_code)]
async fn the_documented_service(sso: &Sso, uow: &Uow, meta: &ClientMeta) -> Result<(), SsoError> {
    let _: SsoConnection = sso
        .create_connection(uow, NewConnection::saml(SsoOwner::App, "A", ""))
        .await?;
    let _: SsoConnection = sso
        .update_connection(uow, "acme", ConnectionUpdate::default())
        .await?;
    sso.delete_connection(uow, "acme").await?;
    let _: SsoConnection = sso.connection("acme").await?;
    let _: Vec<SsoConnection> = sso.connections(Some(SsoOwner::App)).await?;
    let domain: SsoDomain = sso.add_domain(uow, "acme", "acme.com").await?;
    let _: SsoDomain = sso.verify_domain(uow, domain.id).await?;
    let _: SsoDomain = sso.mark_domain_verified(uow, domain.id).await?;
    sso.remove_domain(uow, domain.id).await?;
    let _: Vec<SsoDomain> = sso.domains("acme").await?;
    let _: SsoDiscovery = sso.discover("ada@acme.com").await?;
    let _: SsoRedirect = sso.login("acme", Some("/deals".to_owned())).await?;
    let sign_in: SsoSignIn = sso.complete_saml("acme", "PHNhbWw+", Some("rs"), meta).await?;
    let _: SsoSignIn = sso
        .complete_oidc("acme", &OidcCallback::default(), meta)
        .await?;
    let profile = SsoProfile {
        subject: "s".to_owned(),
        email: None,
        email_verified: None,
        name: None,
        groups: Vec::new(),
        attributes: json!({}),
    };
    let _: SsoSignIn = sso.sign_in_profile("acme", profile, meta).await?;
    let code: String = sso.issue_code(&sign_in).await?;
    let (_, _, _): (User, TokenPair, Option<String>) =
        sso.exchange(&code, Some("state"), meta).await?;
    let _: String = sso.saml_metadata("acme").await?;
    let _: &SsoOptions = sso.options();
    Ok(())
}

#[test]
fn the_documented_events() {
    assert_eq!(SsoUserProvisioned::NAME, "sso.user_provisioned");
    assert_eq!(SsoDomainVerified::NAME, "sso.domain_verified");
    let SsoUserProvisioned {
        connection_id: _,
        user_id: _,
        owner: _,
        role: _,
        created: _,
    } = SsoUserProvisioned {
        connection_id: Uuid::nil(),
        user_id: Uuid::nil(),
        owner: SsoOwner::App,
        role: None,
        created: true,
    };
    let SsoDomainVerified {
        connection_id: _,
        domain: _,
    } = SsoDomainVerified {
        connection_id: Uuid::nil(),
        domain: "acme.com".to_owned(),
    };
}

#[test]
fn the_documented_saml_building_blocks_and_fake() {
    let idp = SsoFake::new();
    assert_eq!(idp.entity_id(), SsoFake::ENTITY_ID);
    assert!(SsoFake::CERTIFICATE_PEM.starts_with("-----BEGIN CERTIFICATE-----"));
    let metadata: IdpMetadata = IdpMetadata::parse(&idp.metadata_xml()).unwrap();
    let sp = ServiceProvider {
        entity_id: "https://api.acme.test/api/sso/saml/acme/metadata".to_owned(),
        acs_url: "https://api.acme.test/api/sso/saml/acme/acs".to_owned(),
    };
    assert!(sp.metadata_xml().contains("SPSSODescriptor"));
    let now: DateTime<Utc> = Utc::now();
    let request =
        AuthnRequest::new(&sp, &metadata, "rs", now, &RequestOptions::default()).unwrap();
    let response: FakeResponse = idp
        .respond_to(&request.url)
        .unwrap()
        .name_id("ada@acme.test")
        .attribute("groups", ["Admins"])
        .sign_response(true)
        .sign_assertion(true)
        .valid_for(Duration::from_secs(60))
        .assertion_id("_a1");
    let check = ResponseCheck {
        sp: &sp,
        idp: &metadata,
        request_id: Some(&request.id),
        now,
        clock_skew: Duration::from_secs(120),
        allow_idp_initiated: false,
        require_signed_assertion: true,
    };
    let assertion: SamlAssertion = validate_response(&response.encoded(), &check).unwrap();
    assert_eq!(assertion.name_id, "ada@acme.test");
    let failure = idp.unsolicited(&sp, now).failure("urn:x", Some("no"));
    assert!(matches!(
        validate_response(&failure.encoded(), &check),
        Err(SamlError::Status { .. })
    ));
    let _ = (
        SignatureError::Missing,
        XmlError::Doctype,
        SsoError::InvalidState,
    );
}

#[tokio::test]
async fn the_documented_resolvers() {
    let dns = StaticTxtResolver::new().record("acme.com", "navree-sso-verification=t");
    dns.add("acme.com", "v=spf1 -all");
    assert_eq!(dns.txt("acme.com").await.unwrap().len(), 2);
    let (http, _server) = navree_http_client::HttpClient::fake();
    let _: Box<dyn TxtResolver> = Box::new(DohResolver::new(
        http,
        "https://cloudflare-dns.com/dns-query",
    ));
}
