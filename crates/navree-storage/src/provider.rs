//! [`Provider`]: URLs signed by a cloud provider, so clients download and
//! upload files without going through the app.

#[cfg(feature = "__cloud")]
pub(crate) use cloud::{Provider, ProviderKind};
#[cfg(not(feature = "__cloud"))]
pub(crate) use none::Provider;

#[cfg(feature = "__cloud")]
mod cloud {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    use navree_kernel::BoxError;

    use crate::store::{HeaderName, HeaderValue, Method, PrefixedStore, Store};

    /// The longest validity of a link S3 and Google Cloud Storage sign.
    const MAX_SIGNED_TTL_SECS: u64 = 7 * 24 * 60 * 60;

    /// Which provider signs.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum ProviderKind {
        /// SigV4 query signatures; signed headers are enforced.
        #[cfg_attr(not(feature = "s3"), allow(dead_code))]
        S3,
        /// V4 signatures; signed headers are enforced, including
        /// `x-goog-content-length-range`.
        #[cfg_attr(not(feature = "gcs"), allow(dead_code))]
        Gcs,
        /// Service or user delegation SAS; headers cannot be signed.
        #[cfg_attr(not(feature = "azure"), allow(dead_code))]
        Azure,
    }

    /// Signs links for one cloud disk through its [`Store`].
    #[derive(Clone)]
    pub(crate) struct Provider {
        kind: ProviderKind,
        store: Arc<dyn Store>,
    }

    impl Provider {
        /// Signs links through `store`'s [`Store::signed_url_with`].
        #[cfg_attr(
            not(any(feature = "s3", feature = "gcs", feature = "azure")),
            allow(dead_code)
        )]
        pub(crate) fn from_store(kind: ProviderKind, store: Arc<dyn Store>) -> Self {
            Self { kind, store }
        }

        /// The same provider for a disk whose keys live under `prefix`.
        pub(crate) fn with_prefix(mut self, prefix: String) -> Self {
            self.store = Arc::new(PrefixedStore::new(self.store, prefix));
            self
        }

        /// The longest validity a signed link may have.
        pub(crate) fn max_ttl(&self) -> Option<Duration> {
            match self.kind {
                ProviderKind::S3 | ProviderKind::Gcs => {
                    Some(Duration::from_secs(MAX_SIGNED_TTL_SECS))
                }
                ProviderKind::Azure => None,
            }
        }

        /// A link that downloads `path` for `ttl`.
        pub(crate) async fn get_url(&self, path: &str, ttl: Duration) -> Result<String, BoxError> {
            let url = self.store.signed_url(path, Method::GET, ttl).await?;
            Ok(url.into())
        }

        /// A link that uploads `path` for `ttl`, and the headers the client
        /// must send with it. S3 and Google Cloud Storage refuse an upload
        /// whose `Content-Type` differs from `content_type`. Google Cloud
        /// Storage also refuses a body outside `x-goog-content-length-range`:
        /// up to `max_bytes`, or exactly the declared `size`. S3 cannot bind
        /// a size to a presigned `PUT` (SigV4 presigned PUTs do not enforce a
        /// signed size bound); [`Disk::accept_upload`](crate::Disk::accept_upload)
        /// checks it afterwards.
        pub(crate) async fn put_url(
            &self,
            path: &str,
            ttl: Duration,
            content_type: Option<&str>,
            max_bytes: u64,
            size: Option<u64>,
        ) -> Result<(String, BTreeMap<String, String>), BoxError> {
            let mut headers = BTreeMap::new();
            if let Some(content_type) = content_type {
                headers.insert("content-type".to_owned(), content_type.to_owned());
            }
            match self.kind {
                ProviderKind::S3 | ProviderKind::Gcs => {
                    if self.kind == ProviderKind::Gcs {
                        let range = match size {
                            Some(size) => format!("{size},{size}"),
                            None => format!("0,{max_bytes}"),
                        };
                        headers.insert("x-goog-content-length-range".to_owned(), range);
                    }
                    let mut bound = Vec::with_capacity(headers.len());
                    for (name, value) in &headers {
                        let name = HeaderName::from_bytes(name.as_bytes())?;
                        let value = HeaderValue::from_str(value)?;
                        bound.push((name, value));
                    }
                    let url = self
                        .store
                        .signed_url_with(path, Method::PUT, ttl, &bound)
                        .await?;
                    Ok((url.into(), headers))
                }
                ProviderKind::Azure => {
                    headers.insert("x-ms-blob-type".to_owned(), "BlockBlob".to_owned());
                    let url = self.store.signed_url(path, Method::PUT, ttl).await?;
                    Ok((url.into(), headers))
                }
            }
        }
    }
}

#[cfg(not(feature = "__cloud"))]
mod none {
    //! Without a cloud driver there is no provider: [`Provider`] has no
    //! values, so disks never take the provider path.

    use std::collections::BTreeMap;
    use std::time::Duration;

    use navree_kernel::BoxError;

    /// No provider (no cloud driver is compiled in).
    #[derive(Clone)]
    pub(crate) enum Provider {}

    impl Provider {
        pub(crate) fn max_ttl(&self) -> Option<Duration> {
            match *self {}
        }

        pub(crate) fn with_prefix(self, _prefix: String) -> Self {
            match self {}
        }

        pub(crate) async fn get_url(
            &self,
            _path: &str,
            _ttl: Duration,
        ) -> Result<String, BoxError> {
            match *self {}
        }

        pub(crate) async fn put_url(
            &self,
            _path: &str,
            _ttl: Duration,
            _content_type: Option<&str>,
            _max_bytes: u64,
            _size: Option<u64>,
        ) -> Result<(String, BTreeMap<String, String>), BoxError> {
            match *self {}
        }
    }
}
