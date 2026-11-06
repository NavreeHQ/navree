//! [`GrpcChannels`]: the named channels to the gRPC services the app calls
//! (`grpc.clients.<name>`), which carry the current call's context.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use http::header::{AUTHORIZATION, HeaderName, HeaderValue};
use navree_http::AppError;
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use tokio::time::Instant;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};

use crate::context::{CallContext, TRACEPARENT, TRACESTATE, X_WORKSPACE};
use crate::edge::grpc_timeout;
use crate::{ClientOptions, GrpcOptions};

static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
static GRPC_TIMEOUT: HeaderName = HeaderName::from_static("grpc-timeout");

/// The channels to the services named in `grpc.clients`, shared by the
/// whole app. Inject it and build tonic's generated client on a channel:
///
/// ```ignore
/// let mut deals = DealsClient::new(self.channels.get("deals")?);
/// let reply = deals.get_deal(GetDealRequest { id: id.to_string() }).await?;
/// ```
///
/// Channels connect lazily, on the first call, and reconnect on their own.
pub struct GrpcChannels {
    channels: BTreeMap<String, GrpcChannel>,
}

impl GrpcChannels {
    /// Channels built from `clients` (what `grpc.clients` configures).
    /// Must run inside a Tokio runtime.
    pub fn new(clients: &BTreeMap<String, ClientOptions>) -> Result<Self, String> {
        let channels = clients
            .iter()
            .map(|(name, options)| Ok((name.clone(), GrpcChannel::connect_lazy(name, options)?)))
            .collect::<Result<_, String>>()?;
        Ok(Self { channels })
    }

    /// The channel named `name`; a 500 when `grpc.clients.<name>` is not
    /// configured.
    pub fn get(&self, name: &str) -> Result<GrpcChannel, AppError> {
        self.channels.get(name).cloned().ok_or_else(|| {
            AppError::internal(format!(
                "no gRPC client named `{name}`: configure [grpc.clients.{name}] with its url"
            ))
        })
    }

    /// The configured names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.channels.keys().map(String::as_str)
    }
}

impl fmt::Debug for GrpcChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrpcChannels")
            .field("names", &self.channels.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Injectable for GrpcChannels {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<GrpcOptions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let options = r.get::<GrpcOptions>().await?;
            GrpcChannels::new(&options.clients).map_err(|error| KernelError::Construction {
                provider: std::any::type_name::<GrpcChannels>().to_owned(),
                source: error.into(),
            })
        })
    }
}

/// One named channel: tonic's [`Channel`] plus what a Navree channel adds
/// to every call made while the app handles a gRPC call or an HTTP
/// request: the request id (`x-request-id`), the trace context
/// (`traceparent`, `tracestate`), the `authorization` (with
/// `forward_auth`), the workspace (`x-workspace`) and a `grpc-timeout` no
/// later than the current deadline nor than `timeout_ms`. Headers the
/// caller sets itself win.
#[derive(Clone)]
pub struct GrpcChannel {
    inner: Channel,
    settings: Arc<Settings>,
}

struct Settings {
    timeout: Duration,
    forward_auth: bool,
}

impl GrpcChannel {
    fn connect_lazy(name: &str, options: &ClientOptions) -> Result<Self, String> {
        let key = format!("grpc.clients.{name}");
        let url = options.url.trim().to_owned();
        let timeout = Duration::from_millis(options.timeout_ms);
        let mut endpoint = Endpoint::from_shared(url.clone())
            .map_err(|error| format!("{key}.url `{url}`: {error}"))?
            .connect_timeout(timeout)
            .timeout(timeout);
        if url.starts_with("https://") {
            let mut tls = ClientTlsConfig::new().with_native_roots();
            if let Some(ca) = &options.tls.ca {
                let pem = std::fs::read(ca)
                    .map_err(|error| format!("{key}.tls.ca `{}`: {error}", ca.display()))?;
                tls = tls.ca_certificate(Certificate::from_pem(pem));
            }
            if let (Some(cert), Some(private_key)) = (&options.tls.cert, &options.tls.key) {
                let cert = std::fs::read(cert)
                    .map_err(|error| format!("{key}.tls.cert `{}`: {error}", cert.display()))?;
                let private_key = std::fs::read(private_key).map_err(|error| {
                    format!("{key}.tls.key `{}`: {error}", private_key.display())
                })?;
                tls = tls.identity(Identity::from_pem(cert, private_key));
            }
            endpoint = endpoint
                .tls_config(tls)
                .map_err(|error| format!("{key}.tls: {error}"))?;
        }
        Ok(Self {
            inner: endpoint.connect_lazy(),
            settings: Arc::new(Settings {
                timeout,
                forward_auth: options.forward_auth,
            }),
        })
    }

    /// Adds the current call's context to an outgoing request.
    fn apply(&self, request: &mut http::Request<tonic::body::Body>) {
        let context = CallContext::current();
        let headers = request.headers_mut();
        let now = Instant::now();
        let mut deadline = now + self.settings.timeout;
        if let Some(own) = grpc_timeout(headers) {
            deadline = deadline.min(now + own);
        }
        if let Some(context) = &context {
            if let Some(current) = context.deadline {
                deadline = deadline.min(current);
            }
            let mut copy = |name: &HeaderName, value: &Option<HeaderValue>| {
                if let Some(value) = value
                    && !headers.contains_key(name)
                {
                    headers.insert(name.clone(), value.clone());
                }
            };
            copy(&TRACEPARENT, &context.traceparent);
            copy(&TRACESTATE, &context.tracestate);
            copy(&X_WORKSPACE, &context.workspace);
            if self.settings.forward_auth {
                copy(&AUTHORIZATION, &context.authorization);
            }
            if let Some(id) = &context.request_id
                && !headers.contains_key(&X_REQUEST_ID)
                && let Ok(value) = HeaderValue::from_str(id)
            {
                headers.insert(X_REQUEST_ID.clone(), value);
            }
        }
        let remaining = deadline
            .saturating_duration_since(now)
            .as_millis()
            .clamp(1, 99_999_999);
        if let Ok(value) = HeaderValue::from_str(&format!("{remaining}m")) {
            headers.insert(GRPC_TIMEOUT.clone(), value);
        }
    }
}

impl fmt::Debug for GrpcChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrpcChannel")
            .field("timeout", &self.settings.timeout)
            .field("forward_auth", &self.settings.forward_auth)
            .finish_non_exhaustive()
    }
}

impl tower::Service<http::Request<tonic::body::Body>> for GrpcChannel {
    type Response = http::Response<tonic::body::Body>;
    type Error = tonic::transport::Error;
    type Future = tonic::transport::channel::ResponseFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        tower::Service::poll_ready(&mut self.inner, cx)
    }

    fn call(&mut self, mut request: http::Request<tonic::body::Body>) -> Self::Future {
        self.apply(&mut request);
        tower::Service::call(&mut self.inner, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unknown_names_and_bad_settings_are_errors() {
        let mut clients = BTreeMap::new();
        clients.insert("deals".to_owned(), ClientOptions::new("http://127.0.0.1:1"));
        let channels = GrpcChannels::new(&clients).unwrap();
        assert_eq!(channels.names().collect::<Vec<_>>(), ["deals"]);
        assert!(channels.get("deals").is_ok());
        let error = channels.get("billing").unwrap_err();
        assert!(format!("{error:?}").contains("grpc.clients.billing"));

        let mut tls = ClientOptions::new("https://deals.internal:443");
        tls.tls.ca = Some("does-not-exist.pem".into());
        let mut clients = BTreeMap::new();
        clients.insert("deals".to_owned(), tls);
        let error = GrpcChannels::new(&clients).unwrap_err();
        assert!(error.contains("grpc.clients.deals.tls.ca"), "{error}");
    }

    #[tokio::test]
    async fn outgoing_requests_carry_the_current_context() {
        let channel =
            GrpcChannel::connect_lazy("deals", &ClientOptions::new("http://127.0.0.1:1")).unwrap();
        let mut headers = http::HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer user-token"));
        headers.insert(&X_WORKSPACE, HeaderValue::from_static("agra"));
        headers.insert(
            &TRACEPARENT,
            HeaderValue::from_static("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
        );
        let request_id = navree_contracts::RequestId::parse_or_generate(None);
        let deadline = Instant::now() + Duration::from_millis(1500);
        let context = CallContext::of(&headers, Some(&request_id), Some(deadline));

        let mut request = http::Request::new(tonic::body::Body::empty());
        context
            .clone()
            .scope(async { channel.apply(&mut request) })
            .await;
        let sent = request.headers();
        assert_eq!(sent[AUTHORIZATION], "Bearer user-token");
        assert_eq!(sent[&X_WORKSPACE], "agra");
        assert_eq!(sent[&X_REQUEST_ID], request_id.as_str());
        assert!(sent.contains_key(&TRACEPARENT));
        let timeout = grpc_timeout(sent).unwrap();
        assert!(timeout <= Duration::from_millis(1500), "{timeout:?}");

        // Metadata set on the request wins over the forwarded values.
        let mut request = http::Request::new(tonic::body::Body::empty());
        request.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer service-token"),
        );
        request
            .headers_mut()
            .insert(&X_WORKSPACE, HeaderValue::from_static("gulati"));
        request
            .headers_mut()
            .insert(&X_REQUEST_ID, HeaderValue::from_static("req-own"));
        context
            .clone()
            .scope(async { channel.apply(&mut request) })
            .await;
        let sent = request.headers();
        assert_eq!(sent[AUTHORIZATION], "Bearer service-token");
        assert_eq!(sent[&X_WORKSPACE], "gulati");
        assert_eq!(sent[&X_REQUEST_ID], "req-own");

        // Outside any call: only the channel's own timeout.
        let mut request = http::Request::new(tonic::body::Body::empty());
        channel.apply(&mut request);
        assert!(!request.headers().contains_key(AUTHORIZATION));
        assert_eq!(
            grpc_timeout(request.headers()),
            Some(Duration::from_millis(5000))
        );

        // Without forward_auth, the authorization stays home.
        let mut private = ClientOptions::new("http://127.0.0.1:1");
        private.forward_auth = false;
        let channel = GrpcChannel::connect_lazy("x", &private).unwrap();
        let mut request = http::Request::new(tonic::body::Body::empty());
        context.scope(async { channel.apply(&mut request) }).await;
        assert!(!request.headers().contains_key(AUTHORIZATION));
        assert_eq!(request.headers()[&X_WORKSPACE], "agra");
    }
}
