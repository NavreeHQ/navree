//! The OTLP exporter (feature `otel`): spans to an OpenTelemetry collector
//! or backend over gRPC or HTTP/protobuf, through the OpenTelemetry SDK's
//! exporter.

use std::borrow::Cow;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use navree_kernel::BoxFuture;
use navree_telemetry::TraceId;
use opentelemetry::trace::{
    Link, SpanContext, SpanId as OtelSpanId, SpanKind as OtelSpanKind, Status, TraceFlags,
    TraceId as OtelTraceId, TraceState,
};
use opentelemetry::{InstrumentationScope, KeyValue};
use opentelemetry_otlp::tonic_types::metadata::MetadataMap;
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig, WithTonicConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{SpanData, SpanEvents, SpanExporter as _, SpanLinks};
use serde_json::Value;

use crate::export::SpanExport;
use crate::{OtlpProtocol, RecordedSpan, SpanKind, SpanStatus, TracingOptions};

/// Sends batches through `opentelemetry_otlp::SpanExporter`.
pub(crate) struct OtlpExport {
    exporter: opentelemetry_otlp::SpanExporter,
    scope: InstrumentationScope,
}

impl OtlpExport {
    /// The exporter for `options` (its endpoint, protocol, headers and
    /// timeout), with the service's resource. Makes no connection yet.
    pub(crate) fn new(
        options: &TracingOptions,
        service_version: Option<String>,
    ) -> Result<OtlpExport, String> {
        // reqwest and tonic use rustls with the ring provider; an error
        // means a provider is already installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let endpoint = options
            .traces_endpoint()
            .ok_or("observe.tracing.endpoint is not set")?;
        let timeout = Duration::from_millis(options.timeout_ms);
        let built = match options.protocol {
            OtlpProtocol::Grpc => {
                let mut headers = http::HeaderMap::new();
                for (name, value) in &options.headers {
                    let name = http::HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                        format!("observe.tracing.headers: `{name}` is not a valid header name")
                    })?;
                    let value = http::HeaderValue::from_str(value).map_err(|_| {
                        format!("observe.tracing.headers: the value of `{name}` is not a valid header value")
                    })?;
                    headers.insert(name, value);
                }
                opentelemetry_otlp::SpanExporter::builder()
                    .with_tonic()
                    .with_endpoint(endpoint)
                    .with_timeout(timeout)
                    .with_metadata(MetadataMap::from_headers(headers))
                    .build()
            }
            OtlpProtocol::Http => opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_endpoint(endpoint)
                .with_timeout(timeout)
                .with_headers(
                    options
                        .headers
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<HashMap<_, _>>(),
                )
                .build(),
        };
        let mut exporter =
            built.map_err(|error| format!("cannot build the OTLP span exporter: {error}"))?;
        exporter.set_resource(&resource(options, service_version));
        Ok(OtlpExport {
            exporter,
            scope: InstrumentationScope::builder("navree")
                .with_version(env!("CARGO_PKG_VERSION"))
                .build(),
        })
    }
}

impl SpanExport for OtlpExport {
    fn export(&self, batch: Vec<RecordedSpan>) -> BoxFuture<'_, Result<(), String>> {
        let spans: Vec<SpanData> = batch
            .into_iter()
            .map(|span| span_data(span, &self.scope))
            .collect();
        Box::pin(async move {
            self.exporter
                .export(spans)
                .await
                .map_err(|error| error.to_string())
        })
    }

    fn shutdown(&self) -> BoxFuture<'_, ()> {
        if let Err(error) = self.exporter.shutdown() {
            tracing::debug!(target: "navree::tracing", %error, "OTLP exporter shutdown");
        }
        Box::pin(async {})
    }
}

/// The resource of every span: `service.name`, `service.instance.id`
/// (unique per process), `service.version` when known, then
/// `observe.tracing.resource`.
fn resource(options: &TracingOptions, service_version: Option<String>) -> Resource {
    let service = options
        .service_name
        .clone()
        .unwrap_or_else(|| "navree-app".to_owned());
    let mut attributes = vec![KeyValue::new(
        "service.instance.id",
        TraceId::random().to_string(),
    )];
    if let Some(version) = service_version {
        attributes.push(KeyValue::new("service.version", version));
    }
    attributes.extend(
        options
            .resource
            .iter()
            .map(|(key, value)| KeyValue::new(key.clone(), value.clone())),
    );
    Resource::builder_empty()
        .with_service_name(service)
        .with_attributes(attributes)
        .build()
}

/// A recorded span in the SDK's export shape.
pub(crate) fn span_data(span: RecordedSpan, scope: &InstrumentationScope) -> SpanData {
    let context = |trace: TraceId, id: navree_telemetry::SpanId, remote: bool| {
        SpanContext::new(
            OtelTraceId::from_bytes(trace.to_bytes()),
            OtelSpanId::from_bytes(id.to_bytes()),
            TraceFlags::SAMPLED,
            remote,
            TraceState::default(),
        )
    };
    let mut links = SpanLinks::default();
    links.links = span
        .links
        .iter()
        .map(|link| Link::with_context(context(link.trace_id, link.span_id, true)))
        .collect();
    SpanData {
        span_context: context(span.trace_id, span.span_id, false),
        parent_span_id: span.parent_span_id.map_or(OtelSpanId::INVALID, |id| {
            OtelSpanId::from_bytes(id.to_bytes())
        }),
        parent_span_is_remote: span.kind == SpanKind::Server && span.parent_span_id.is_some(),
        span_kind: match span.kind {
            SpanKind::Server => OtelSpanKind::Server,
            SpanKind::Client => OtelSpanKind::Client,
            SpanKind::Producer => OtelSpanKind::Producer,
            SpanKind::Consumer => OtelSpanKind::Consumer,
            SpanKind::Internal => OtelSpanKind::Internal,
        },
        name: Cow::Owned(span.name),
        start_time: SystemTime::from(span.start),
        end_time: SystemTime::from(span.end),
        attributes: span
            .attributes
            .into_iter()
            .map(|(key, value)| key_value(key, value))
            .collect(),
        dropped_attributes_count: 0,
        events: SpanEvents::default(),
        links,
        status: match span.status {
            SpanStatus::Unset => Status::Unset,
            SpanStatus::Error { message } => Status::error(message),
        },
        instrumentation_scope: scope.clone(),
    }
}

/// A JSON attribute value as an OpenTelemetry one.
fn key_value(key: String, value: Value) -> KeyValue {
    match value {
        Value::Bool(flag) => KeyValue::new(key, flag),
        Value::Number(number) => match number.as_i64() {
            Some(integer) => KeyValue::new(key, integer),
            None => KeyValue::new(key, number.as_f64().unwrap_or_default()),
        },
        Value::String(text) => KeyValue::new(key, text),
        other => KeyValue::new(key, other.to_string()),
    }
}
