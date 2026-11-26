//! Azure Blob Storage signing, credential-chain order, user delegation SAS,
//! block uploads with token refresh, and store operations.

#![cfg(feature = "azure")]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use chrono::{TimeZone, Utc};
use futures_util::TryStreamExt;
use navree_crypto::hmac_sha256;
use navree_storage::cloud::azure::{
    AuthKind, AzureStore, SharedKey, UserDelegationKey, VERSION, http_date,
};
use navree_storage::cloud::http::Request;
use navree_storage::cloud::retry::RetryPolicy;
use navree_storage::cloud::xml;
use navree_storage::store::{HeaderName, HeaderValue, Method, Store, StoreError, Url};
use navree_storage::{AzureConfig, DiskConfig, Storage, StorageError, StorageOptions};
use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The development key published for Azurite.
const AZURITE_KEY: &str =
    "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

fn env_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn quick_retry() -> RetryPolicy {
    RetryPolicy {
        max_retries: 2,
        first_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(2),
        ..RetryPolicy::default()
    }
}

/// A controllable clock starting at a fixed instant in 2026.
#[derive(Clone)]
struct Hands(Arc<AtomicU64>);

impl Hands {
    fn new() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }

    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH
            + Duration::from_secs(1_795_680_000)
            + Duration::from_secs(self.0.load(Ordering::SeqCst))
    }

    fn advance_minutes(&self, minutes: u64) {
        self.0.fetch_add(minutes * 60, Ordering::SeqCst);
    }
}

#[test]
fn shared_key_signature_matches_the_documented_example() {
    // Microsoft's documented Shared Key example (service versions 2015-02-21
    // and later): `Content-Length: 0` is signed as an empty line, `Date` is
    // left blank when `x-ms-date` is present, `x-ms-*` headers are sorted
    // in lower case, and query parameters are sorted in lower case.
    let signer = SharedKey::new("myaccount", AZURITE_KEY).unwrap();
    let url = Url::parse(
        "https://myaccount.blob.core.windows.net/mycontainer?restype=container&timeout=30",
    )
    .unwrap();
    let request = Request::new(Method::PUT, url)
        .header("x-ms-version", "2015-02-21")
        .unwrap()
        .header("x-ms-date", "Fri, 26 Jun 2015 23:39:12 GMT")
        .unwrap()
        .header("content-length", "0")
        .unwrap();

    let expected_string_to_sign = "\
PUT\n\
\n\
\n\
\n\
\n\
\n\
\n\
\n\
\n\
\n\
\n\
\n\
x-ms-date:Fri, 26 Jun 2015 23:39:12 GMT\n\
x-ms-version:2015-02-21\n\
/myaccount/mycontainer\n\
restype:container\n\
timeout:30";

    assert_eq!(signer.string_to_sign(&request), expected_string_to_sign);
    let key_bytes = STANDARD.decode(AZURITE_KEY).unwrap();
    let expected_sig = STANDARD.encode(hmac_sha256(&key_bytes, expected_string_to_sign.as_bytes()));
    assert_eq!(signer.signature(&request), expected_sig);
    assert_eq!(
        signer.authorization(&request),
        format!("SharedKey myaccount:{expected_sig}")
    );

    // Non-zero Content-Length, Range, If-Match, unfolded x-ms-* headers,
    // Azurite path-style resource, and `+` preserved in base64 blockid query.
    let azurite = SharedKey::new("devstoreaccount1", AZURITE_KEY).unwrap();
    let block_url = Url::parse(
        "http://127.0.0.1:10000/devstoreaccount1/ludhiana/2026/INV%201101.pdf?blockid=AA%2BB%3D%3D&comp=block",
    )
    .unwrap();
    let put_block = Request::new(Method::PUT, block_url)
        .header("content-type", "application/pdf")
        .unwrap()
        .header("x-ms-meta-city", "  Ludhiana   Punjab ")
        .unwrap()
        .with_body("challan-9");
    let at = SystemTime::from(Utc.with_ymd_and_hms(2026, 11, 26, 6, 0, 0).unwrap());
    assert_eq!(http_date(at), "Thu, 26 Nov 2026 06:00:00 GMT");

    let signed = azurite.sign(put_block, at).unwrap();
    assert_eq!(
        azurite.string_to_sign(&signed),
        format!(
            "PUT\n\n\n9\n\napplication/pdf\n\n\n\n\n\n\n\
             x-ms-date:Thu, 26 Nov 2026 06:00:00 GMT\n\
             x-ms-meta-city:Ludhiana Punjab\n\
             x-ms-version:{VERSION}\n\
             /devstoreaccount1/devstoreaccount1/ludhiana/2026/INV%201101.pdf\n\
             blockid:AA+B==\n\
             comp:block"
        )
    );
    assert!(signed.headers["authorization"].is_sensitive());
    let debug = format!("{signer:?} {signed:?}");
    assert!(!debug.contains(AZURITE_KEY), "{debug}");
    assert!(!debug.contains(&expected_sig), "{debug}");
}

#[tokio::test]
async fn a_user_delegation_sas_is_signed_with_the_delegation_key() {
    let delegation_secret = STANDARD.encode(b"delegation-key-bytes-for-amritsar");
    let key = UserDelegationKey::new(
        "11111111-2222-3333-4444-555555555555",
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        "2026-11-26T00:00:00Z",
        "2026-11-26T12:00:00Z",
        "b",
        VERSION,
        &delegation_secret,
    )
    .unwrap();

    let start = Utc.with_ymd_and_hms(2026, 11, 26, 6, 0, 0).unwrap();
    let expiry = Utc.with_ymd_and_hms(2026, 11, 26, 6, 15, 0).unwrap();
    let mut hosted = Url::parse(
        "https://agrastorage.blob.core.windows.net/challans/%E0%A8%B2%E0%A9%81%E0%A8%A7%E0%A8%BF%E0%A8%86%E0%A8%A3%E0%A8%BE%20%E2%82%B9100%2B18%25.pdf",
    )
    .unwrap();

    let expected_to_sign = format!(
        "r\n\
         2026-11-26T06:00:00Z\n\
         2026-11-26T06:15:00Z\n\
         /blob/agrastorage/challans/ਲੁਧਿਆਣਾ ₹100+18%.pdf\n\
         11111111-2222-3333-4444-555555555555\n\
         aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\n\
         2026-11-26T00:00:00Z\n\
         2026-11-26T12:00:00Z\n\
         b\n\
         {VERSION}\n\
         \n\
         \n\
         \n\
         \n\
         https\n\
         {VERSION}\n\
         b\n\
         \n\
         \n\
         \n\
         \n\
         \n\
         \n"
    );
    assert_eq!(
        key.string_to_sign(
            "agrastorage",
            "challans",
            "ਲੁਧਿਆਣਾ ₹100+18%.pdf",
            &hosted,
            "r",
            start,
            expiry,
        ),
        expected_to_sign
    );

    key.sign(
        "agrastorage",
        "challans",
        "ਲੁਧਿਆਣਾ ₹100+18%.pdf",
        &mut hosted,
        "r",
        start,
        expiry,
    );
    let params: BTreeMap<String, String> = hosted.query_pairs().into_owned().collect();
    assert_eq!(params["sv"], VERSION);
    assert_eq!(params["sp"], "r");
    assert_eq!(params["st"], "2026-11-26T06:00:00Z");
    assert_eq!(params["se"], "2026-11-26T06:15:00Z");
    assert_eq!(params["sr"], "b");
    assert_eq!(params["spr"], "https");
    assert_eq!(params["skoid"], "11111111-2222-3333-4444-555555555555");
    assert_eq!(params["sktid"], "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    assert_eq!(params["skt"], "2026-11-26T00:00:00Z");
    assert_eq!(params["ske"], "2026-11-26T12:00:00Z");
    assert_eq!(params["sks"], "b");
    assert_eq!(params["skv"], VERSION);
    let expected_sig = STANDARD.encode(hmac_sha256(
        b"delegation-key-bytes-for-amritsar",
        expected_to_sign.as_bytes(),
    ));
    assert_eq!(params["sig"], expected_sig);
    assert!(!format!("{key:?}").contains(&delegation_secret));

    // End-to-end through AzureStore: OAuth token -> GetUserDelegationKey
    // (cached across calls) -> user delegation SAS for GET and PUT.
    let oauth_server = MockServer::start().await;
    let blob_server = MockServer::start().await;
    let delegation_calls = Arc::new(AtomicUsize::new(0));
    let counted = delegation_calls.clone();
    let xml_key = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <UserDelegationKey>\
           <SignedOid>oid-jaipur</SignedOid>\
           <SignedTid>tid-jaipur</SignedTid>\
           <SignedStart>2026-11-26T00:00:00Z</SignedStart>\
           <SignedExpiry>2026-11-27T00:00:00Z</SignedExpiry>\
           <SignedService>b</SignedService>\
           <SignedVersion>{VERSION}</SignedVersion>\
           <Value>{delegation_secret}</Value>\
         </UserDelegationKey>"
    );

    Mock::given(method("POST"))
        .and(path("/tid-jaipur/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "entra-token-jaipur",
            "expires_in": 3600
        })))
        .expect(1)
        .mount(&oauth_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(query_param("restype", "service"))
        .and(query_param("comp", "userdelegationkey"))
        .and(header("authorization", "Bearer entra-token-jaipur"))
        .respond_with(move |req: &wiremock::Request| {
            counted.fetch_add(1, Ordering::SeqCst);
            let parsed = xml::parse(&req.body).unwrap();
            assert_eq!(parsed.name(), "KeyInfo");
            assert!(parsed.value("Start").unwrap().ends_with('Z'));
            assert!(parsed.value("Expiry").unwrap().ends_with('Z'));
            ResponseTemplate::new(200).set_body_string(xml_key.clone())
        })
        .expect(1)
        .mount(&blob_server)
        .await;

    let hands = Hands::new();
    let vars = env_map(&[
        ("AZURE_TENANT_ID", "tid-jaipur"),
        ("AZURE_CLIENT_ID", "cid-jaipur"),
        ("AZURE_CLIENT_SECRET", "csecret-jaipur"),
        ("AZURE_AUTHORITY_HOST", &oauth_server.uri()),
    ]);
    let config = AzureConfig {
        endpoint: Some(blob_server.uri()),
        ..AzureConfig::new("jaipurstorage", "invoices")
    };
    let clock = hands.clone();
    let store = AzureStore::from_env(config, &vars)
        .unwrap()
        .with_clock(move || clock.now())
        .with_retry_policy(quick_retry());

    let get_link = store
        .signed_url("2026/INV 1101.pdf", Method::GET, Duration::from_secs(600))
        .await
        .unwrap();
    let put_link = store
        .signed_url("2026/INV 1102.pdf", Method::PUT, Duration::from_secs(600))
        .await
        .unwrap();
    assert_eq!(delegation_calls.load(Ordering::SeqCst), 1);

    let get_params: BTreeMap<String, String> = get_link.query_pairs().into_owned().collect();
    let put_params: BTreeMap<String, String> = put_link.query_pairs().into_owned().collect();
    assert_eq!(get_params["sp"], "r");
    assert_eq!(get_params["skoid"], "oid-jaipur");
    assert_eq!(put_params["sp"], "w");
    assert_eq!(put_params["sktid"], "tid-jaipur");

    // Azure SAS cannot bind headers, refuses unsupported methods, and caps
    // user delegation SAS lifetimes at 7 days.
    let bound = [(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/pdf"),
    )];
    assert!(matches!(
        store
            .signed_url_with("a.pdf", Method::PUT, Duration::from_secs(60), &bound)
            .await,
        Err(StoreError::Failed(_))
    ));
    assert!(matches!(
        store
            .signed_url("a.pdf", Method::DELETE, Duration::from_secs(60))
            .await,
        Err(StoreError::Failed(_))
    ));
    assert!(matches!(
        store
            .signed_url("a.pdf", Method::GET, Duration::from_secs(8 * 86_400))
            .await,
        Err(StoreError::Failed(_))
    ));
}

#[tokio::test]
async fn the_credential_chain_order_is_todays() {
    let oauth_server = MockServer::start().await;
    let app_identity_server = MockServer::start().await;
    let imds_server = MockServer::start().await;
    let blob_server = MockServer::start().await;

    let token_dir = tempfile::tempdir().unwrap();
    let token_file = token_dir.path().join("federated.jwt");
    std::fs::write(&token_file, "k8s-federated-jwt-agra\n").unwrap();
    let token_file_str = token_file.to_str().unwrap().to_owned();

    // Base environment with every fallback populated at once; we peel one
    // layer off at a time and check both `auth_kind()` and the wire request.
    let oauth_uri = oauth_server.uri();
    let app_identity_uri = app_identity_server.uri();
    let imds_uri = imds_server.uri();
    let all_vars = vec![
        ("AZURE_STORAGE_ACCOUNT_NAME", "envaccount"),
        ("AZURE_STORAGE_ACCOUNT_KEY", AZURITE_KEY),
        ("AZURE_TENANT_ID", "tid-agra"),
        ("AZURE_CLIENT_ID", "cid-agra"),
        ("AZURE_CLIENT_SECRET", "sp-secret-agra"),
        ("AZURE_AUTHORITY_HOST", oauth_uri.as_str()),
        ("AZURE_FEDERATED_TOKEN_FILE", token_file_str.as_str()),
        ("IDENTITY_ENDPOINT", app_identity_uri.as_str()),
        ("IDENTITY_HEADER", "app-service-secret"),
        ("AZURE_IMDS_ENDPOINT", imds_uri.as_str()),
    ];

    // 1. Explicit `config.access_key` wins over every environment variable,
    //    and an invalid base64 key fails options validation as today.
    let bad_disk = AzureConfig {
        access_key: Some("not-valid-base64!!!".to_owned().into()),
        ..AzureConfig::new("agrastorage", "archive")
    };
    let bad_options = StorageOptions {
        default: "archive".into(),
        disks: BTreeMap::from([("archive".to_owned(), DiskConfig::Azure(bad_disk))]),
        base_url: None,
    };
    match Storage::new(&bad_options, b"signing-key") {
        Err(StorageError::InvalidOptions(msg)) => {
            assert_eq!(
                msg,
                "storage.disks.archive.access_key is not a base64 account key"
            );
            assert!(!msg.contains("not-valid-base64"), "{msg}");
            assert!(!msg.contains("symbol") && !msg.contains("offset"), "{msg}");
        }
        other => panic!("expected InvalidOptions, got {other:?}"),
    }

    let custom_key = STANDARD.encode(b"0123456789abcdef0123456789abcdef");
    let step1_config = AzureConfig {
        access_key: Some(custom_key.clone().into()),
        endpoint: Some(blob_server.uri()),
        ..AzureConfig::new("diskaccount", "archive")
    };
    let step1 = AzureStore::from_env(step1_config, &env_map(&all_vars)).unwrap();
    assert_eq!(step1.account(), "diskaccount");
    assert_eq!(step1.auth_kind(), AuthKind::SharedKey);

    // 2. Environment Shared Key (`AZURE_STORAGE_ACCOUNT_KEY`) wins when the
    //    disk leaves `access_key` unset; account comes from
    //    `AZURE_STORAGE_ACCOUNT_NAME` (or Azurite endpoint when unset).
    let no_disk_key = AzureConfig {
        account: None,
        container: "archive".into(),
        endpoint: Some(blob_server.uri()),
        ..AzureConfig::default()
    };
    let step2 = AzureStore::from_env(no_disk_key.clone(), &env_map(&all_vars)).unwrap();
    assert_eq!(step2.account(), "envaccount");
    assert_eq!(step2.auth_kind(), AuthKind::SharedKey);

    let azurite_inferred = AzureStore::from_env(
        AzureConfig {
            account: None,
            container: "archive".into(),
            endpoint: Some("http://127.0.0.1:10000/devstoreaccount1".into()),
            ..AzureConfig::default()
        },
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(azurite_inferred.account(), "devstoreaccount1");

    // 3. Service Principal (`AZURE_TENANT_ID` + `AZURE_CLIENT_ID` +
    //    `AZURE_CLIENT_SECRET`) wins over Workload Identity and Managed Identity.
    let without_shared_key: Vec<(&str, &str)> = all_vars
        .iter()
        .copied()
        .filter(|(k, _)| *k != "AZURE_STORAGE_ACCOUNT_KEY")
        .collect();
    Mock::given(method("POST"))
        .and(path("/tid-agra/oauth2/v2.0/token"))
        .respond_with(|req: &wiremock::Request| {
            let form: BTreeMap<String, String> = url::form_urlencoded::parse(&req.body)
                .into_owned()
                .collect();
            assert_eq!(form["grant_type"], "client_credentials");
            assert_eq!(form["client_id"], "cid-agra");
            assert_eq!(form["scope"], "https://storage.azure.com/.default");
            if let Some(secret) = form.get("client_secret") {
                assert_eq!(secret, "sp-secret-agra");
                ResponseTemplate::new(200).set_body_json(json!({
                    "access_token": "sp-bearer-token",
                    "expires_in": 3600
                }))
            } else {
                assert_eq!(
                    form["client_assertion_type"],
                    "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"
                );
                assert_eq!(form["client_assertion"], "k8s-federated-jwt-agra");
                ResponseTemplate::new(200).set_body_json(json!({
                    "access_token": "wi-bearer-token",
                    "expires_in": 3600
                }))
            }
        })
        .mount(&oauth_server)
        .await;

    Mock::given(method("PUT"))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "\"0x1\"")
                .insert_header("last-modified", "Thu, 26 Nov 2026 06:00:00 GMT"),
        )
        .mount(&blob_server)
        .await;

    let step3 = AzureStore::from_env(no_disk_key.clone(), &env_map(&without_shared_key))
        .unwrap()
        .with_retry_policy(quick_retry());
    assert_eq!(step3.auth_kind(), AuthKind::ClientSecret);
    step3
        .put("sp.txt", Bytes::from_static(b"ok"))
        .await
        .unwrap();

    // 4. Workload Identity (`AZURE_FEDERATED_TOKEN_FILE`) wins when
    //    `AZURE_CLIENT_SECRET` is absent.
    let without_client_secret: Vec<(&str, &str)> = without_shared_key
        .iter()
        .copied()
        .filter(|(k, _)| *k != "AZURE_CLIENT_SECRET")
        .collect();
    let step4 = AzureStore::from_env(no_disk_key.clone(), &env_map(&without_client_secret))
        .unwrap()
        .with_retry_policy(quick_retry());
    assert_eq!(step4.auth_kind(), AuthKind::WorkloadIdentity);
    step4
        .put("wi.txt", Bytes::from_static(b"ok"))
        .await
        .unwrap();

    // 5a. App Service Managed Identity (`IDENTITY_ENDPOINT` + `IDENTITY_HEADER`)
    //     wins over IMDS and forwards `client_id` when `AZURE_CLIENT_ID` is set.
    let without_wi: Vec<(&str, &str)> = without_client_secret
        .iter()
        .copied()
        .filter(|(k, _)| *k != "AZURE_FEDERATED_TOKEN_FILE" && *k != "AZURE_TENANT_ID")
        .collect();
    Mock::given(method("GET"))
        .and(query_param("api-version", "2019-08-01"))
        .and(query_param("resource", "https://storage.azure.com/"))
        .and(query_param("client_id", "cid-agra"))
        .and(header("x-identity-header", "app-service-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "app-mi-bearer-token",
            "expires_on": "1995683600"
        })))
        .expect(1)
        .mount(&app_identity_server)
        .await;

    let step5a = AzureStore::from_env(no_disk_key.clone(), &env_map(&without_wi))
        .unwrap()
        .with_retry_policy(quick_retry());
    assert_eq!(step5a.auth_kind(), AuthKind::AppServiceIdentity);
    step5a
        .put("app-mi.txt", Bytes::from_static(b"ok"))
        .await
        .unwrap();

    // 5b. IMDS Managed Identity is the final fallback (`Metadata: true`,
    //     `api-version=2018-02-01`).
    let only_imds: Vec<(&str, &str)> = without_wi
        .iter()
        .copied()
        .filter(|(k, _)| *k != "IDENTITY_ENDPOINT" && *k != "IDENTITY_HEADER")
        .collect();
    Mock::given(method("GET"))
        .and(path("/metadata/identity/oauth2/token"))
        .and(query_param("api-version", "2018-02-01"))
        .and(query_param("resource", "https://storage.azure.com/"))
        .and(query_param("client_id", "cid-agra"))
        .and(header("metadata", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "imds-bearer-token",
            "expires_in": "3600"
        })))
        .expect(1)
        .mount(&imds_server)
        .await;

    let step5b = AzureStore::from_env(no_disk_key, &env_map(&only_imds))
        .unwrap()
        .with_retry_policy(quick_retry());
    assert_eq!(step5b.auth_kind(), AuthKind::ImdsIdentity);
    step5b
        .put("imds.txt", Bytes::from_static(b"ok"))
        .await
        .unwrap();

    // Verify each store request carried the bearer token of its own step.
    let received = blob_server.received_requests().await.unwrap();
    let auth_headers: Vec<&str> = received
        .iter()
        .map(|req| req.headers["authorization"].to_str().unwrap())
        .collect();
    assert_eq!(
        auth_headers,
        [
            "Bearer sp-bearer-token",
            "Bearer wi-bearer-token",
            "Bearer app-mi-bearer-token",
            "Bearer imds-bearer-token",
        ]
    );
}

#[tokio::test]
async fn an_expired_token_is_refreshed_between_blocks() {
    let oauth_server = MockServer::start().await;
    let blob_server = MockServer::start().await;

    let token_dir = tempfile::tempdir().unwrap();
    let token_file = token_dir.path().join("projected-sa-token");
    std::fs::write(&token_file, "assertion-v1").unwrap();

    let token_fetches = Arc::new(AtomicUsize::new(0));
    let counted = token_fetches.clone();
    Mock::given(method("POST"))
        .and(path("/tid-amritsar/oauth2/v2.0/token"))
        .respond_with(move |req: &wiremock::Request| {
            let n = counted.fetch_add(1, Ordering::SeqCst) + 1;
            let form: BTreeMap<String, String> = url::form_urlencoded::parse(&req.body)
                .into_owned()
                .collect();
            let expected_assertion = format!("assertion-v{n}");
            assert_eq!(form["client_assertion"], expected_assertion);
            ResponseTemplate::new(200).set_body_json(json!({
                "access_token": format!("amritsar-token-{n}"),
                "expires_in": 3600
            }))
        })
        .mount(&oauth_server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/ledgers/2026/amritsar.bin"))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "\"0x99\"")
                .insert_header("last-modified", "Thu, 26 Nov 2026 06:00:00 GMT"),
        )
        .mount(&blob_server)
        .await;

    let hands = Hands::new();
    let vars = env_map(&[
        ("AZURE_TENANT_ID", "tid-amritsar"),
        ("AZURE_CLIENT_ID", "cid-amritsar"),
        ("AZURE_FEDERATED_TOKEN_FILE", token_file.to_str().unwrap()),
        ("AZURE_AUTHORITY_HOST", &oauth_server.uri()),
    ]);
    let config = AzureConfig {
        endpoint: Some(blob_server.uri()),
        ..AzureConfig::new("amritsarstorage", "ledgers")
    };
    let clock = hands.clone();
    let store = AzureStore::from_env(config, &vars)
        .unwrap()
        .with_clock(move || clock.now())
        .with_block_size(4)
        .with_retry_policy(quick_retry());

    let mut upload = store.begin_upload("2026/amritsar.bin").await.unwrap();
    // First 4-byte block stages immediately under `amritsar-token-1`.
    upload.write(Bytes::from_static(b"AAAA")).await.unwrap();
    assert_eq!(token_fetches.load(Ordering::SeqCst), 1);

    // Advance 56 minutes (past the 5-minute refresh threshold of the 60-minute
    // token) and rotate the projected workload-identity token file on disk.
    hands.advance_minutes(56);
    std::fs::write(&token_file, "assertion-v2").unwrap();

    // Second 4-byte block refreshes the token and stages under `amritsar-token-2`.
    upload.write(Bytes::from_static(b"BBBB")).await.unwrap();
    assert_eq!(token_fetches.load(Ordering::SeqCst), 2);

    // Trailing 2 bytes flush on finish, then `comp=blocklist` commits all three.
    upload.write(Bytes::from_static(b"CC")).await.unwrap();
    upload.finish().await.unwrap();
    assert_eq!(token_fetches.load(Ordering::SeqCst), 2);

    let requests = blob_server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 4);

    let auths: Vec<&str> = requests
        .iter()
        .map(|req| req.headers["authorization"].to_str().unwrap())
        .collect();
    assert_eq!(
        auths,
        [
            "Bearer amritsar-token-1",
            "Bearer amritsar-token-2",
            "Bearer amritsar-token-2",
            "Bearer amritsar-token-2",
        ]
    );

    let mut staged_ids = Vec::new();
    for req in &requests[..3] {
        let params: BTreeMap<String, String> = req.url.query_pairs().into_owned().collect();
        assert_eq!(params["comp"], "block");
        staged_ids.push(params["blockid"].clone());
    }
    assert_eq!(staged_ids[0].len(), staged_ids[1].len());
    assert_eq!(staged_ids[1].len(), staged_ids[2].len());

    let commit_params: BTreeMap<String, String> =
        requests[3].url.query_pairs().into_owned().collect();
    assert_eq!(commit_params["comp"], "blocklist");
    let block_list = xml::parse(&requests[3].body).unwrap();
    assert_eq!(block_list.name(), "BlockList");
    let committed: Vec<&str> = block_list.all("Latest").map(xml::Element::text).collect();
    assert_eq!(committed, staged_ids);
}

#[tokio::test]
async fn blob_operations_pagination_and_errors_keep_secrets_out_of_messages() {
    let server = MockServer::start().await;
    let config = AzureConfig {
        access_key: Some(AZURITE_KEY.to_owned().into()),
        endpoint: Some(format!("{}/devstoreaccount1", server.uri())),
        ..AzureConfig::new("devstoreaccount1", "archive")
    };
    let store = AzureStore::from_env(config, &BTreeMap::new())
        .unwrap()
        .with_retry_policy(quick_retry());
    assert_eq!(store.to_string(), "Azure(devstoreaccount1/archive)");
    assert!(!format!("{store:?}").contains(AZURITE_KEY));

    // HEAD, GET (whole, bounded range, 416 range past EOF), open stream, DELETE.
    Mock::given(method("HEAD"))
        .and(path("/devstoreaccount1/archive/docs/challan.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "8")
                .insert_header("etag", "\"0x10\"")
                .insert_header("last-modified", "Thu, 26 Nov 2026 06:00:00 GMT"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/devstoreaccount1/archive/docs/challan.txt"))
        .respond_with(|req: &wiremock::Request| match req.headers.get("range") {
            Some(range) if range == "bytes=0-3" => ResponseTemplate::new(206)
                .insert_header("content-range", "bytes 0-3/8")
                .insert_header("etag", "\"0x10\"")
                .set_body_string("Agra"),
            Some(range) if range == "bytes=50-60" => {
                ResponseTemplate::new(416).insert_header("content-range", "bytes */8")
            }
            None => ResponseTemplate::new(200)
                .insert_header("etag", "\"0x10\"")
                .set_body_string("Agra-108"),
            other => panic!("unexpected range: {other:?}"),
        })
        .mount(&server)
        .await;

    Mock::given(method("DELETE"))
        .and(path("/devstoreaccount1/archive/docs/challan.txt"))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("DELETE"))
        .and(path("/devstoreaccount1/archive/docs/missing.txt"))
        .respond_with(ResponseTemplate::new(404).insert_header("x-ms-error-code", "BlobNotFound"))
        .expect(1)
        .mount(&server)
        .await;

    let meta = store.head("docs/challan.txt").await.unwrap();
    assert_eq!((meta.path.as_str(), meta.size), ("docs/challan.txt", 8));
    assert_eq!(
        store.read("docs/challan.txt", None).await.unwrap(),
        "Agra-108"
    );
    assert_eq!(
        store.read("docs/challan.txt", Some(0..4)).await.unwrap(),
        "Agra"
    );
    assert_eq!(
        store.read("docs/challan.txt", Some(4..4)).await.unwrap(),
        ""
    );
    assert_eq!(
        store.read("docs/challan.txt", Some(50..61)).await.unwrap(),
        ""
    );

    let opened = store.open("docs/challan.txt").await.unwrap();
    assert_eq!(opened.size, 8);
    let streamed: Vec<Bytes> = opened.stream.try_collect().await.unwrap();
    assert_eq!(streamed.concat(), b"Agra-108");

    store.delete("docs/challan.txt").await.unwrap();
    assert!(matches!(
        store.delete("docs/missing.txt").await,
        Err(StoreError::NotFound)
    ));

    // Paginated List Blobs with directory prefix and NextMarker.
    Mock::given(method("GET"))
        .and(path("/devstoreaccount1/archive"))
        .and(query_param("restype", "container"))
        .and(query_param("comp", "list"))
        .and(query_param("prefix", "challans/"))
        .respond_with(|req: &wiremock::Request| {
            let params: BTreeMap<String, String> = req.url.query_pairs().into_owned().collect();
            if params.get("marker").map(String::as_str) == Some("page-2") {
                ResponseTemplate::new(200).set_body_string(
                    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                     <EnumerationResults>\
                       <Blobs>\
                         <Blob>\
                           <Name>challans/CH 0043 &amp; 0044.pdf</Name>\
                           <Properties>\
                             <Last-Modified>Thu, 26 Nov 2026 06:05:00 GMT</Last-Modified>\
                             <Content-Length>250</Content-Length>\
                           </Properties>\
                         </Blob>\
                       </Blobs>\
                       <NextMarker />\
                     </EnumerationResults>",
                )
            } else {
                ResponseTemplate::new(200).set_body_string(
                    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                     <EnumerationResults>\
                       <Blobs>\
                         <Blob>\
                           <Name>challans/CH-0042.pdf</Name>\
                           <Properties>\
                             <Last-Modified>Thu, 26 Nov 2026 06:00:00 GMT</Last-Modified>\
                             <Content-Length>120</Content-Length>\
                           </Properties>\
                         </Blob>\
                         <Blob>\
                           <Name>challans/subdir</Name>\
                           <Properties>\
                             <Last-Modified>Thu, 26 Nov 2026 06:00:00 GMT</Last-Modified>\
                             <Content-Length>0</Content-Length>\
                             <ResourceType>directory</ResourceType>\
                           </Properties>\
                         </Blob>\
                       </Blobs>\
                       <NextMarker>page-2</NextMarker>\
                     </EnumerationResults>",
                )
            }
        })
        .mount(&server)
        .await;

    let listed = store.list("challans").await.unwrap();
    let summary: Vec<(&str, u64)> = listed.iter().map(|m| (m.path.as_str(), m.size)).collect();
    assert_eq!(
        summary,
        [
            ("challans/CH-0042.pdf", 120),
            ("challans/CH 0043 & 0044.pdf", 250),
        ]
    );

    // XML `<Error><Code>` is extracted while `<AuthenticationErrorDetail>`
    // and `<Message>` never reach `Display` or `Debug`.
    Mock::given(method("PUT"))
        .and(path("/devstoreaccount1/archive/denied.txt"))
        .respond_with(ResponseTemplate::new(403).set_body_string(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <Error>\
               <Code>AuthenticationFailed</Code>\
               <Message>Server failed to authenticate the request.</Message>\
               <AuthenticationErrorDetail>StringToSign-with-secret-detail</AuthenticationErrorDetail>\
             </Error>",
        ))
        .mount(&server)
        .await;

    let err = store
        .put("denied.txt", Bytes::from_static(b"x"))
        .await
        .unwrap_err();
    let rendered = format!("{err} {err:?}");
    assert!(rendered.contains("AuthenticationFailed"), "{rendered}");
    assert!(
        !rendered.contains("StringToSign-with-secret-detail"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("Server failed to authenticate"),
        "{rendered}"
    );
    assert!(!rendered.contains(AZURITE_KEY), "{rendered}");
}

#[tokio::test]
async fn sas_canonical_resource_partial_credentials_and_legacy_msi_behave_strictly() {
    use navree_storage::cloud::ErrorKind;

    // 1. Custom domain where `container == account` (`acmefiles`) never strips
    //    the container segment from the signed SAS canonical resource, and pins
    //    `spr=https` on HTTPS endpoints.
    let hands = Hands::new();
    let clock = hands.clone();
    let custom = AzureStore::from_env(
        AzureConfig {
            access_key: Some(AZURITE_KEY.to_owned().into()),
            endpoint: Some("https://blobs.amritsar.example".to_owned()),
            ..AzureConfig::new("acmefiles", "acmefiles")
        },
        &BTreeMap::new(),
    )
    .unwrap()
    .with_clock(move || clock.now());

    let signed = custom
        .signed_url(
            "other-container/secret.pdf",
            Method::GET,
            Duration::from_secs(300),
        )
        .await
        .unwrap();
    let params: BTreeMap<String, String> = signed.query_pairs().into_owned().collect();
    assert_eq!(params["spr"], "https");
    let expected_sts = format!(
        "r\n{}\n{}\n/blob/acmefiles/acmefiles/other-container/secret.pdf\n\n\nhttps\n{VERSION}\nb\n\n\n\n\n\n\n",
        params["st"], params["se"],
    );
    let key_bytes = STANDARD.decode(AZURITE_KEY).unwrap();
    assert_eq!(
        params["sig"],
        STANDARD.encode(hmac_sha256(&key_bytes, expected_sts.as_bytes()))
    );

    // 2. Empty keys, remote plain-HTTP authorities, remote plain-HTTP MSI, and
    //    partial credential tuples fail closed rather than falling through to IMDS.
    assert!(SharedKey::new("agrastorage", "").is_err());
    assert!(SharedKey::new("agrastorage", "   ").is_err());

    let base_cfg = AzureConfig::new("agrastorage", "archive");
    for partial in [
        vec![("AZURE_CLIENT_SECRET", "only-secret")],
        vec![
            ("AZURE_TENANT_ID", "tid-agra"),
            ("AZURE_CLIENT_SECRET", "only-secret"),
        ],
        vec![("AZURE_FEDERATED_TOKEN_FILE", "/tmp/tok")],
        vec![("AZURE_TENANT_ID", "tid-agra")],
        vec![("AZURE_STORAGE_CLIENT_SECRET", "storage-secret")],
        vec![
            ("AZURE_TENANT_ID", "tid-agra"),
            ("AZURE_CLIENT_ID", "cid-agra"),
            ("AZURE_STORAGE_CLIENT_SECRET", "storage-secret"),
        ],
        vec![("IDENTITY_ENDPOINT", "http://127.0.0.1:9999/msi")],
        vec![("IDENTITY_HEADER", "only-header")],
        vec![("MSI_ENDPOINT", "http://127.0.0.1:9999/msi")],
        vec![("MSI_SECRET", "only-secret")],
    ] {
        let err = match AzureStore::from_env(base_cfg.clone(), &env_map(&partial)) {
            Err(err) => err,
            Ok(_) => panic!("expected partial Azure env {partial:?} to fail closed"),
        };
        assert_eq!(err.kind(), ErrorKind::Credential, "{partial:?}");
    }

    for refused_env in [
        vec![
            ("AZURE_TENANT_ID", "tid-agra"),
            ("AZURE_CLIENT_ID", "cid-agra"),
            ("AZURE_CLIENT_SECRET", "sec"),
            ("AZURE_AUTHORITY_HOST", "http://login.amritsar.example"),
        ],
        vec![
            ("IDENTITY_ENDPOINT", "http://203.0.113.10/msi"),
            ("IDENTITY_HEADER", "hdr"),
        ],
        vec![(
            "AZURE_STORAGE_ENDPOINT",
            "http://evil.amritsar.example/otheraccount",
        )],
    ] {
        let err = match AzureStore::from_env(base_cfg.clone(), &env_map(&refused_env)) {
            Err(err) => err,
            Ok(_) => panic!("expected untrusted plain HTTP env {refused_env:?} to be refused"),
        };
        assert_eq!(err.kind(), ErrorKind::Refused, "{refused_env:?}");
    }

    // 3. `AZURE_STORAGE_*` service principal takes cohesive precedence over
    //    `AZURE_*` without mixing principals, and legacy `MSI_ENDPOINT` +
    //    `MSI_SECRET` uses `api-version=2017-09-01`, `secret` header and
    //    `clientid` query parameter.
    let storage_oauth = MockServer::start().await;
    let msi_server = MockServer::start().await;
    let blob_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/tid-storage/oauth2/v2.0/token"))
        .respond_with(|req: &wiremock::Request| {
            let form: BTreeMap<String, String> = url::form_urlencoded::parse(&req.body)
                .into_owned()
                .collect();
            assert_eq!(form["client_id"], "cid-storage");
            assert_eq!(form["client_secret"], "sec-storage");
            ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "storage-sp-token",
                "expires_in": u64::MAX
            }))
        })
        .expect(1)
        .mount(&storage_oauth)
        .await;

    Mock::given(method("GET"))
        .and(query_param("api-version", "2017-09-01"))
        .and(query_param("resource", "https://storage.azure.com/"))
        .and(query_param("clientid", "cid-legacy"))
        .and(header("secret", "legacy-msi-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "legacy-msi-token",
            "expires_on": "253402300799"
        })))
        .expect(1)
        .mount(&msi_server)
        .await;

    Mock::given(method("PUT"))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "\"0x1\"")
                .insert_header("last-modified", "Thu, 26 Nov 2026 06:00:00 GMT"),
        )
        .mount(&blob_server)
        .await;

    let sp_store = AzureStore::from_env(
        AzureConfig {
            endpoint: Some(blob_server.uri()),
            ..AzureConfig::new("agrastorage", "archive")
        },
        &env_map(&[
            ("AZURE_STORAGE_TENANT_ID", "tid-storage"),
            ("AZURE_STORAGE_CLIENT_ID", "cid-storage"),
            ("AZURE_STORAGE_CLIENT_SECRET", "sec-storage"),
            ("AZURE_STORAGE_AUTHORITY_HOST", &storage_oauth.uri()),
            ("AZURE_TENANT_ID", "tid-pod"),
            ("AZURE_CLIENT_ID", "cid-pod"),
        ]),
    )
    .unwrap();
    sp_store
        .put("a.txt", Bytes::from_static(b"1"))
        .await
        .unwrap();

    let legacy_msi_store = AzureStore::from_env(
        AzureConfig {
            endpoint: Some(blob_server.uri()),
            ..AzureConfig::new("agrastorage", "archive")
        },
        &env_map(&[
            ("AZURE_CLIENT_ID", "cid-legacy"),
            ("MSI_ENDPOINT", &msi_server.uri()),
            ("MSI_SECRET", "legacy-msi-secret"),
        ]),
    )
    .unwrap();
    legacy_msi_store
        .put("b.txt", Bytes::from_static(b"2"))
        .await
        .unwrap();

    // 4. Invalid `<SignedExpiry>` or short-lived `<UserDelegationKey>` is
    //    refused, and 2-step `NextMarker` cycles fail cleanly.
    Mock::given(method("POST"))
        .and(query_param("restype", "service"))
        .and(query_param("comp", "userdelegationkey"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <UserDelegationKey>\
               <SignedOid>oid-1</SignedOid>\
               <SignedTid>tid-1</SignedTid>\
               <SignedStart>2026-11-26T00:00:00Z</SignedStart>\
               <SignedExpiry>not-a-timestamp</SignedExpiry>\
               <SignedService>b</SignedService>\
               <SignedVersion>{VERSION}</SignedVersion>\
               <Value>{AZURITE_KEY}</Value>\
             </UserDelegationKey>"
        )))
        .mount(&blob_server)
        .await;
    assert!(
        sp_store
            .signed_url("a.txt", Method::GET, Duration::from_secs(300))
            .await
            .is_err()
    );

    Mock::given(method("GET"))
        .and(query_param("restype", "container"))
        .and(query_param("comp", "list"))
        .respond_with(|req: &wiremock::Request| {
            let params: BTreeMap<String, String> = req.url.query_pairs().into_owned().collect();
            let next = if params.get("marker").map(String::as_str) == Some("m1") {
                "m2"
            } else {
                "m1"
            };
            ResponseTemplate::new(200).set_body_string(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <EnumerationResults><Blobs /><NextMarker>{next}</NextMarker></EnumerationResults>"
            ))
        })
        .mount(&blob_server)
        .await;
    assert!(sp_store.list("docs").await.is_err());
}
