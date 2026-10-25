//! The SSRF policy on real and fake clients.

use http::{Method, StatusCode};
use keel_http_client::{FakeResponse, HttpClient, HttpClientError};

use crate::support::{OK, TestServer, client_allowing};

#[tokio::test]
async fn ssrf_blocks_private_and_metadata_ips() {
    let client = client_allowing(&[]);
    for url in [
        "http://169.254.169.254/latest/meta-data/",
        "http://127.0.0.1:9/",
        "http://10.0.0.1/",
        "http://192.168.1.10/admin",
        "http://100.64.0.1/",
        "http://[::1]/",
        "http://[fd00:ec2::254]/",
        "http://[::ffff:169.254.169.254]/",
        "http://0x7f.1/",
        "http://2130706433/",
    ] {
        let error = client.get(url).send().await.unwrap_err();
        assert!(
            matches!(error, HttpClientError::Blocked { .. }),
            "{url}: {error:?}"
        );
    }
}

#[tokio::test]
async fn ssrf_blocks_literal_ips_in_fake_mode_too() {
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "*", FakeResponse::new(200));

    let error = client
        .get("http://169.254.169.254/latest/meta-data/")
        .send()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::Blocked { .. }),
        "{error:?}"
    );
    assert!(
        server.recorded().is_empty(),
        "a blocked request is never sent"
    );
}

#[tokio::test]
async fn ssrf_blocks_hostname_resolving_to_loopback() {
    let server = TestServer::start(OK).await;
    let client = client_allowing(&[]);

    let error = client
        .get(format!("http://localhost:{}/", server.addr.port()))
        .send()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::Blocked { .. }),
        "{error:?}"
    );
    assert!(server.heads().is_empty(), "nothing reached the server");
}

#[tokio::test]
async fn ssrf_allows_allowlisted_host() {
    let server = TestServer::start(OK).await;

    let by_ip = client_allowing(&["127.0.0.1"])
        .get(server.url("/by-ip"))
        .send()
        .await
        .unwrap();
    let by_name = client_allowing(&["localhost"])
        .get(format!("http://localhost:{}/by-name", server.addr.port()))
        .send()
        .await
        .unwrap();

    assert_eq!(by_ip.status(), StatusCode::OK);
    assert_eq!(by_ip.text(), "ok");
    assert_eq!(by_name.status(), StatusCode::OK);
    assert_eq!(server.heads().len(), 2);
}

#[tokio::test]
async fn redirect_to_blocked_address_is_refused() {
    let server = TestServer::start(
        "HTTP/1.1 302 Found\r\nlocation: http://169.254.169.254/latest/meta-data/\r\n\
         content-length: 0\r\nconnection: close\r\n\r\n",
    )
    .await;

    let error = client_allowing(&["127.0.0.1"])
        .get(server.url("/redirect"))
        .send()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::Blocked { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn non_http_schemes_are_refused() {
    let client = client_allowing(&[]);

    let error = client.get("file:///etc/passwd").send().await.unwrap_err();

    assert!(matches!(error, HttpClientError::Build(_)), "{error:?}");
}
