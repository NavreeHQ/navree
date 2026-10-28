//! The Cloudflare driver against a mock of the Cloudflare DNS API.

use navree_domains::{CloudflareDns, CloudflareOptions, DnsProvider, DomainsError, RecordKind};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RECORDS: &str = "/zones/zone-1/dns_records";

fn driver(server: &MockServer) -> CloudflareDns {
    CloudflareDns::new(&CloudflareOptions {
        api_token: Some("cf-token".into()),
        zone_id: Some("zone-1".into()),
        api_url: server.uri(),
        ..CloudflareOptions::default()
    })
    .unwrap()
}

fn ok(result: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "success": true,
        "errors": [],
        "messages": [],
        "result": result
    }))
}

#[tokio::test]
async fn a_record_is_created_when_the_zone_has_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(RECORDS))
        .and(query_param("type", "CNAME"))
        .and(query_param("name", "acme.links.example.com"))
        .and(header("authorization", "Bearer cf-token"))
        .respond_with(ok(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(RECORDS))
        .and(body_json(json!({
            "type": "CNAME",
            "name": "acme.links.example.com",
            "content": "domains.example.com",
            "ttl": 1,
            "proxied": false
        })))
        .respond_with(ok(json!({ "id": "rec-1" })))
        .expect(1)
        .mount(&server)
        .await;
    driver(&server)
        .set(
            RecordKind::Cname,
            "acme.links.example.com",
            &["domains.example.com".to_owned()],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn existing_records_are_kept_replaced_or_deleted() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(RECORDS))
        .and(query_param("type", "A"))
        .respond_with(ok(json!([
            { "id": "keep", "type": "A", "name": "acme.links.example.com", "content": "203.0.113.10" },
            { "id": "stale", "type": "A", "name": "acme.links.example.com", "content": "198.51.100.1" }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("{RECORDS}/stale")))
        .respond_with(ok(json!({ "id": "stale" })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(RECORDS))
        .and(body_json(json!({
            "type": "A",
            "name": "acme.links.example.com",
            "content": "203.0.113.11",
            "ttl": 1,
            "proxied": false
        })))
        .respond_with(ok(json!({ "id": "new" })))
        .expect(1)
        .mount(&server)
        .await;
    driver(&server)
        .set(
            RecordKind::A,
            "acme.links.example.com",
            &["203.0.113.10".to_owned(), "203.0.113.11".to_owned()],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn deleting_removes_every_record_of_the_name() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(RECORDS))
        .and(query_param("type", "TXT"))
        .respond_with(ok(json!([
            { "id": "t1", "content": "\"one\"" },
            { "id": "t2", "content": "\"two\"" }
        ])))
        .mount(&server)
        .await;
    for id in ["t1", "t2"] {
        Mock::given(method("DELETE"))
            .and(path(format!("{RECORDS}/{id}")))
            .respond_with(ok(json!({ "id": id })))
            .expect(1)
            .mount(&server)
            .await;
    }
    driver(&server)
        .delete(RecordKind::Txt, "_x.acme.links.example.com")
        .await
        .unwrap();
}

#[tokio::test]
async fn api_errors_are_provider_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(RECORDS))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "success": false,
            "errors": [{ "code": 10000, "message": "Authentication error" }],
            "messages": [],
            "result": null
        })))
        .mount(&server)
        .await;
    let error = driver(&server)
        .delete(RecordKind::Cname, "acme.links.example.com")
        .await
        .unwrap_err();
    assert!(
        matches!(&error, DomainsError::DnsProvider(message) if message == "Cloudflare error 10000: Authentication error"),
        "{error}"
    );
}

#[test]
fn the_driver_needs_a_token_and_a_zone() {
    assert!(matches!(
        CloudflareDns::new(&CloudflareOptions::default()),
        Err(DomainsError::Config(_))
    ));
    let debug = format!(
        "{:?}",
        CloudflareDns::new(&CloudflareOptions {
            api_token: Some("secret-token".into()),
            zone_id: Some("zone-1".into()),
            ..CloudflareOptions::default()
        })
        .unwrap()
    );
    assert!(!debug.contains("secret-token"), "{debug}");
}
