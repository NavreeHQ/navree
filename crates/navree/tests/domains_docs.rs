//! The examples of `docs/packages/custom-domains`, through the facade with
//! the `domains` feature (in `full`): the app, adding a domain, the
//! public page, links to the workspace, the listener and the page's test
//! as written.
#![cfg(feature = "sqlite")]
#![cfg(feature = "domains")]
// The page's examples are written as an app writes them, without doc comments.
#![allow(missing_docs)]

use std::sync::Arc;

use navree::BoxFuture;
use navree::access::AccessOptions;
use navree::domains::{
    AcmeClient, AcmeFake, CertificateProbe, CurrentDomain, DnsFake, DnsLookup, DomainStatus,
    DomainVerified, Domains, DomainsModule, NewDomain, ProbeFake,
};
use navree::events::{Listener, ModuleBuilderEventsExt};
use navree::http::{ModuleBuilderHttpExt, RouteDef};
use navree::prelude::*;
use navree::testing::TestApp;
use serde_json::json;

// ---------------------------------------------------------------------------
// "Installation": the app.

pub fn app(sources: ConfigSources, access: AccessOptions) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(CryptoModule::from_config()) // seals certificate keys (tls = "acme")
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(ScheduleModule::for_root()) // runs domains:verify --due
        .module(AccessModule::for_root(access)) // the routes' permission
        .module(AuthModule::from_config())
        .module(WorkspaceModule::from_config()) // domains belong to workspaces
        .module(DomainsModule::from_config()) // section `domains`
}

// ---------------------------------------------------------------------------
// "Adding a domain".

pub async fn add_a_domain(ctx: AppContext, workspace_id: uuid::Uuid) -> Result<(), BoxError> {
    let domains = ctx.get::<Domains>().await?;
    let domain = domains
        .add(workspace_id, NewDomain::new("crm.acme-corp.com"))
        .await?;
    for record in &domain.dns_records {
        println!("{} {} {}", record.kind, record.name, record.value);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// "Who owns a domain": moving a verified domain.

pub async fn transfer_a_domain(
    domains: &Domains,
    new_workspace_id: uuid::Uuid,
) -> Result<(), BoxError> {
    let claim = domains
        .transfer("crm.acme-corp.com", new_workspace_id)
        .await?;
    if claim.is_verified() {
        println!("{} now belongs to {}", claim.host, claim.workspace_id);
    } else {
        // Publish these; the scheduler moves the domain when it finds them.
        for record in &claim.dns_records {
            println!("{} {} {}", record.kind, record.name, record.value);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// "Public pages".

async fn landing(domain: CurrentDomain) -> Result<Json<serde_json::Value>> {
    Ok(Json(json!({
        "workspace": domain.workspace_id,
        "host": domain.host,
        "purpose": domain.purpose,
    })))
}

/// Serves `landing` at `GET /`.
#[injectable]
pub struct LandingController;

impl navree::http::Controller for LandingController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![RouteDef::on(navree::axum::http::Method::GET, "!/", landing).public()]
    }
}

// ---------------------------------------------------------------------------
// "Links to the workspace".

pub async fn link(domains: &Domains, workspace_id: uuid::Uuid) -> Result<String, BoxError> {
    let base = domains
        .primary_url(workspace_id)
        .await?
        .unwrap_or_else(|| "https://app.example.com".to_owned());
    Ok(base)
}

// ---------------------------------------------------------------------------
// "Events": the listener.

#[injectable]
pub struct DomainLog;

impl Listener<DomainVerified> for DomainLog {
    fn handle<'a>(
        &'a self,
        event: &'a DomainVerified,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            tracing::info!(
                host = %event.domain.host,
                workspace = %event.domain.workspace_id,
                "the workspace's domain is live"
            );
            Ok(())
        })
    }
}

/// The listener and the landing page, registered.
pub struct SiteModule;

impl Module for SiteModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<DomainVerified, DomainLog>()
            .controller::<LandingController>();
    }
}

// ---------------------------------------------------------------------------
// "Testing": the page's test as written.

#[tokio::test]
async fn a_customer_brings_their_domain() {
    let dns = DnsFake::new().with_address("domains.example.com", "203.0.113.10");
    let app = TestApp::builder()
        .temp_database()
        .config("domains.target", "domains.example.com")
        .config("domains.routes", false)
        .config("workspace.routes", false)
        .config("workspace.users_table", "")
        .module(DatabaseModule::from_config())
        .module(WorkspaceModule::from_config())
        .module(DomainsModule::from_config())
        .override_value::<dyn DnsLookup>(Arc::new(dns.clone()))
        .override_value::<dyn AcmeClient>(Arc::new(AcmeFake::new()))
        .override_value::<dyn CertificateProbe>(Arc::new(ProbeFake::new()))
        .build()
        .await;
    let domains = app.get::<Domains>().await;
    let workspace = uuid::Uuid::now_v7();

    let domain = domains
        .add(workspace, NewDomain::new("crm.acme-corp.com"))
        .await
        .unwrap();
    assert_eq!(domain.status, DomainStatus::Pending);

    // The customer creates the records.
    dns.set_txt(&domain.dns_records[0].name, &domain.dns_records[0].value);
    dns.set_cname("crm.acme-corp.com", "domains.example.com");
    let domain = domains.verify(domain.id).await.unwrap();
    assert_eq!(domain.status, DomainStatus::Verified);

    app.client()
        .get("/_domains/allow?domain=crm.acme-corp.com")
        .send()
        .await
        .assert_status(200);
}

/// The page's second test: adding someone else's domain first does not
/// block the owner. Then `transfer` moves the verified domain.
#[tokio::test]
async fn a_squatter_does_not_block_the_owner() {
    let dns = DnsFake::new().with_address("domains.example.com", "203.0.113.10");
    let app = TestApp::builder()
        .temp_database()
        .config("domains.target", "domains.example.com")
        .config("domains.routes", false)
        .config("workspace.routes", false)
        .config("workspace.users_table", "")
        .module(DatabaseModule::from_config())
        .module(WorkspaceModule::from_config())
        .module(DomainsModule::from_config())
        .override_value::<dyn DnsLookup>(Arc::new(dns.clone()))
        .override_value::<dyn AcmeClient>(Arc::new(AcmeFake::new()))
        .override_value::<dyn CertificateProbe>(Arc::new(ProbeFake::new()))
        .build()
        .await;
    let domains = app.get::<Domains>().await;
    let squatter = uuid::Uuid::now_v7();
    let owner = uuid::Uuid::now_v7();

    let squat = domains
        .add(squatter, NewDomain::new("crm.acme-corp.com"))
        .await
        .unwrap();
    let claim = domains
        .add(owner, NewDomain::new("crm.acme-corp.com")) // not refused
        .await
        .unwrap();

    // Only the owner controls the DNS, so only their token is published.
    dns.set_txt(&claim.dns_records[0].name, &claim.dns_records[0].value);
    dns.set_cname("crm.acme-corp.com", "domains.example.com");
    let claim = domains.verify(claim.id).await.unwrap();
    assert_eq!(claim.status, DomainStatus::Verified);
    assert_eq!(
        domains.get(squat.id).await.unwrap().status,
        DomainStatus::Failed
    );

    // The page's transfer example: the new workspace is asked for its own
    // records, and the domain moves once they exist.
    let acquirer = uuid::Uuid::now_v7();
    transfer_a_domain(&domains, acquirer).await.unwrap();
    assert_eq!(
        domains
            .resolve("crm.acme-corp.com")
            .await
            .unwrap()
            .unwrap()
            .workspace_id,
        owner
    );
    let pending = domains
        .claims("crm.acme-corp.com")
        .await
        .unwrap()
        .into_iter()
        .find(|domain| domain.workspace_id == acquirer)
        .unwrap();
    dns.set_txt(&pending.dns_records[0].name, &pending.dns_records[0].value);
    transfer_a_domain(&domains, acquirer).await.unwrap();
    assert_eq!(
        domains
            .resolve("crm.acme-corp.com")
            .await
            .unwrap()
            .unwrap()
            .workspace_id,
        acquirer
    );
}

// ---------------------------------------------------------------------------
// The other examples, run: the listener, the landing page, the link, and
// the records `add` prints.

#[tokio::test]
async fn the_landing_page_the_link_and_the_listener_work() {
    let dns = DnsFake::new().with_address("domains.example.com", "203.0.113.10");
    let app = TestApp::builder()
        .temp_database()
        .config("domains.target", "domains.example.com")
        .config("domains.routes", false)
        .config("workspace.routes", false)
        .config("workspace.users_table", "")
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(WorkspaceModule::from_config())
        .module(DomainsModule::from_config())
        .module(SiteModule)
        .override_value::<dyn DnsLookup>(Arc::new(dns.clone()))
        .override_value::<dyn AcmeClient>(Arc::new(AcmeFake::new()))
        .override_value::<dyn CertificateProbe>(Arc::new(ProbeFake::new()))
        .build()
        .await;
    let workspace = uuid::Uuid::now_v7();
    add_a_domain(app.ctx().clone(), workspace).await.unwrap();
    let domains = app.get::<Domains>().await;
    assert_eq!(
        link(&domains, workspace).await.unwrap(),
        "https://app.example.com"
    );

    let domain = domains.find("crm.acme-corp.com").await.unwrap().unwrap();
    dns.set_txt(&domain.dns_records[0].name, &domain.dns_records[0].value);
    dns.set_cname("crm.acme-corp.com", "domains.example.com");
    domains.verify(domain.id).await.unwrap();
    domains.make_primary(domain.id).await.unwrap();
    assert_eq!(
        link(&domains, workspace).await.unwrap(),
        "https://crm.acme-corp.com"
    );

    app.client()
        .get("/")
        .header("host", "crm.acme-corp.com")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("workspace", json!(workspace))
        .assert_json_path("purpose", json!("app"));
    app.client()
        .get("/")
        .header("host", "elsewhere.test")
        .send()
        .await
        .assert_error_code(404, "domain_not_found");

    // The page's app builds.
    let _ = crate::app(ConfigSources::new(), AccessOptions::default());
}
