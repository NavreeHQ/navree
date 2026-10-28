//! The HTTPS listener: the app's routes over TLS with the certificate of
//! the server name each client asks for, and the TLS probe against it.

use std::net::SocketAddr;
use std::sync::Arc;

use chrono::TimeDelta;
use navree_domains::{CertificateProbe, HttpsServer, TlsProbe};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;

use crate::support::World;

/// Sends `GET path` to `addr` over TLS for `host`, trusting `ca_pem`;
/// returns the raw response.
async fn get(addr: SocketAddr, host: &str, path: &str, ca_pem: &str) -> Result<String, String> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(ca_pem.as_bytes()) {
        roots.add(cert.unwrap()).unwrap();
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let stream = TcpStream::connect(addr).await.unwrap();
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(ServerName::try_from(host.to_owned()).unwrap(), stream)
        .await
        .map_err(|error| error.to_string())?;
    tls.write_all(
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .await
    .unwrap();
    let mut response = Vec::new();
    tls.read_to_end(&mut response).await.unwrap();
    Ok(String::from_utf8_lossy(&response).into_owned())
}

#[tokio::test]
async fn the_app_is_served_over_https_with_each_domains_certificate() {
    let world = World::with(|builder| {
        builder
            .config("domains.tls", "acme")
            .config("domains.acme.terms_agreed", true)
    })
    .await;
    let acme = world.workspace(Uuid::now_v7(), "Acme").await;
    let globex = world.workspace(Uuid::now_v7(), "Globex").await;
    world.verified(acme, "crm.acme-corp.test").await;
    world.verified(globex, "portal.globex.test").await;
    let domains = world.domains().await;
    assert_eq!(domains.issue_pending().await.unwrap().len(), 2);
    let ca = world.fakes.ca.ca_pem();

    let server = HttpsServer::bind(world.app.ctx(), "127.0.0.1:0").await.unwrap();
    let addr = server.local_addr();
    tokio::spawn(server.serve());

    let response = get(addr, "crm.acme-corp.test", "/landing", &ca).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains(&json!(acme).to_string()), "{response}");
    let response = get(addr, "portal.globex.test", "/landing", &ca).await.unwrap();
    assert!(response.contains(&json!(globex).to_string()), "{response}");
    // A name without a certificate gets no handshake.
    assert!(get(addr, "unknown.test", "/landing", &ca).await.is_err());

    // The probe reads the certificate a host presents.
    let probe = TlsProbe::new().connect_to("crm.acme-corp.test", addr);
    let found = probe.probe("crm.acme-corp.test").await.unwrap();
    let expected = domains
        .certificates()
        .expires_at("crm.acme-corp.test")
        .unwrap();
    assert_eq!(found.not_after, expected);
    assert_eq!(found.not_after - found.not_before, TimeDelta::days(90));
    let error = TlsProbe::new()
        .connect_to("unknown.test", addr)
        .probe("unknown.test")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("handshake"), "{error}");
    world.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_probe_of_a_closed_port_fails() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let error = TlsProbe::new()
        .timeout(std::time::Duration::from_secs(2))
        .connect_to("shop.acme.test", addr)
        .probe("shop.acme.test")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("shop.acme.test"), "{error}");
}
