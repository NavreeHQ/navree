//! Runs the shared [`cloud_suite`] against [`AzureStore`] backed by `Azurite`
//! (`NAVREE_TEST_AZURE_URL`).

#![cfg(feature = "azure")]

mod cloud_suite;

use std::sync::Arc;
use std::time::SystemTime;

use http::Method;
use navree_storage::AzureConfig;
use navree_storage::cloud::azure::{AzureStore, SharedKey};
use navree_storage::cloud::http::{Client, Request};
use navree_storage::store::{PrefixedStore, Store, Url};
use tokio::sync::OnceCell;

use crate::cloud_suite::{Subject, emulator_url};

/// Azurite's published development storage key.
const AZURITE_KEY: &str =
    "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

static CONTAINER_READY: OnceCell<()> = OnceCell::const_new();

/// Ensures `container` exists on Azurite (`201 Created` or `409`
/// `ContainerAlreadyExists`).
async fn ensure_container(origin: &str, account: &str, container: &str) {
    let client = Client::for_endpoint(Some(origin)).expect("Azure client");
    let url = Url::parse(&format!("{origin}/{account}/{container}?restype=container"))
        .expect("container URL");
    let signer = SharedKey::new(account, AZURITE_KEY).expect("Azurite SharedKey");
    let request = Request::new(Method::PUT, url);
    let signed = signer
        .sign(request, SystemTime::now())
        .expect("sign CreateContainer");
    let answer = client.send(signed).await.expect("Azurite answers");
    let status = answer.status().as_u16();
    assert!(
        status == 201 || status == 409,
        "creating Azurite container `{container}` at `{origin}` answered {status}"
    );
}

async fn connect(test: &str) -> Option<Subject> {
    let raw_url = emulator_url("NAVREE_TEST_AZURE_URL", test)?;
    let parsed = Url::parse(&raw_url).expect("NAVREE_TEST_AZURE_URL is a valid URL");
    let mut segments = parsed.path().trim_matches('/').split('/');
    let account = segments
        .next()
        .filter(|seg| !seg.is_empty())
        .expect("NAVREE_TEST_AZURE_URL must name the account first in its path")
        .to_owned();
    let container = segments
        .next()
        .filter(|seg| !seg.is_empty())
        .expect("NAVREE_TEST_AZURE_URL must name the container after the account")
        .to_owned();
    let origin = parsed.origin().ascii_serialization();
    let endpoint = format!("{origin}/{account}");

    CONTAINER_READY
        .get_or_init(|| ensure_container(&origin, &account, &container))
        .await;

    let config = AzureConfig {
        access_key: Some(AZURITE_KEY.to_owned().into()),
        endpoint: Some(endpoint),
        ..AzureConfig::new(&account, &container)
    };
    let raw: Arc<dyn Store> = Arc::new(AzureStore::new(config).expect("AzureStore builds"));
    let prefix = format!("suite-{test}-{}", navree_crypto::random_token(10));
    let store = Arc::new(PrefixedStore::new(raw.clone(), prefix));

    Some(Subject {
        driver: "azure",
        store,
        raw,
        binds_put_headers: false,
        unsigned_put_headers: &[("x-ms-blob-type", "BlockBlob")],
        guard: None,
    })
}

declare_cloud_suite!(connect);
