//! W3C Trace Context: [`TraceId`], [`SpanId`] and [`TraceContext`]
//! (`traceparent` and `tracestate`), and ratio sampling.

use std::fmt;
use std::str::FromStr;

use rand::RngExt as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The longest `tracestate` passed on (W3C: 512 characters is the minimum
/// a system must propagate; longer values may be dropped).
const MAX_TRACE_STATE: usize = 512;

/// A trace id: 16 bytes, never all zero, written as 32 lowercase hex digits.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TraceId([u8; 16]);

/// A span id: 8 bytes, never all zero, written as 16 lowercase hex digits.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpanId([u8; 8]);

/// The error of parsing a [`TraceId`] or [`SpanId`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidId;

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a valid id: expected lowercase hex digits, not all zero")
    }
}

impl std::error::Error for InvalidId {}

macro_rules! id_type {
    ($name:ident, $len:expr) => {
        impl $name {
            /// A new random id.
            pub fn random() -> Self {
                let mut rng = rand::rng();
                loop {
                    let mut bytes = [0u8; $len];
                    rng.fill(&mut bytes[..]);
                    if let Some(id) = Self::from_bytes(bytes) {
                        return id;
                    }
                }
            }

            /// The id with these bytes; `None` when they are all zero.
            pub fn from_bytes(bytes: [u8; $len]) -> Option<Self> {
                (bytes != [0; $len]).then_some(Self(bytes))
            }

            /// The id's bytes.
            pub fn to_bytes(self) -> [u8; $len] {
                self.0
            }
        }

        impl FromStr for $name {
            type Err = InvalidId;

            /// Parses lowercase hex digits (exactly twice the byte length).
            fn from_str(text: &str) -> Result<Self, InvalidId> {
                let mut bytes = [0u8; $len];
                decode_hex(text, &mut bytes)?;
                Self::from_bytes(bytes).ok_or(InvalidId)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                for byte in self.0 {
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({self})", stringify!($name))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

id_type!(TraceId, 16);
id_type!(SpanId, 8);

/// Decodes `text` (lowercase hex, exactly `out.len() * 2` digits) into `out`.
fn decode_hex(text: &str, out: &mut [u8]) -> Result<(), InvalidId> {
    let digits = text.as_bytes();
    if digits.len() != out.len() * 2 {
        return Err(InvalidId);
    }
    // The length check above leaves no remainder.
    let (pairs, _) = digits.as_chunks::<2>();
    for (byte, [high, low]) in out.iter_mut().zip(pairs) {
        *byte = (nibble(*high)? << 4) | nibble(*low)?;
    }
    Ok(())
}

fn nibble(digit: u8) -> Result<u8, InvalidId> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        _ => Err(InvalidId),
    }
}

/// A span in another trace that a span refers to without being its child:
/// the caller's span when an incoming `traceparent` is not trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanLink {
    /// The other span's trace.
    pub trace_id: TraceId,
    /// The other span.
    pub span_id: SpanId,
}

/// The trace a unit of work (a request, a job run) belongs to, and its own
/// span in it.
///
/// - [`continue_from`](TraceContext::continue_from) continues an incoming
///   `traceparent` (the caller's span becomes the parent);
/// - [`root`](TraceContext::root) and
///   [`root_sampled`](TraceContext::root_sampled) start a new trace;
/// - [`child`](TraceContext::child) is a span under this one (what an
///   outgoing call sends in its `traceparent`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceContext {
    /// The trace.
    pub trace_id: TraceId,
    /// This unit of work's span.
    pub span_id: SpanId,
    /// The parent span, when the trace was continued from a caller.
    pub parent_span_id: Option<SpanId>,
    /// Whether the trace is recorded (exported); the W3C `sampled` flag.
    pub sampled: bool,
    /// The vendor `tracestate`, passed on unchanged.
    pub trace_state: Option<String>,
    /// The caller's span when its `traceparent` was not trusted
    /// ([`linked_to`](TraceContext::linked_to)).
    pub link: Option<SpanLink>,
}

/// The fields of a valid `traceparent` header.
struct Parent {
    trace_id: TraceId,
    span_id: SpanId,
    sampled: bool,
}

impl TraceContext {
    /// A new trace with a new span and no parent.
    pub fn root(sampled: bool) -> Self {
        Self {
            trace_id: TraceId::random(),
            span_id: SpanId::random(),
            parent_span_id: None,
            sampled,
            trace_state: None,
            link: None,
        }
    }

    /// A new trace, sampled when [`sample_trace`] says so for `ratio`.
    pub fn root_sampled(ratio: f64) -> Self {
        let mut root = Self::root(true);
        root.sampled = sample_trace(root.trace_id, ratio);
        root
    }

    /// Continues the caller's trace from its `traceparent` (and
    /// `tracestate`) header: same trace, a new span whose parent is the
    /// caller's, the caller's sampling decision. `None` when the header is
    /// not a valid W3C `traceparent`.
    pub fn continue_from(traceparent: &str, tracestate: Option<&str>) -> Option<Self> {
        let parent = parse_traceparent(traceparent)?;
        Some(Self {
            trace_id: parent.trace_id,
            span_id: SpanId::random(),
            parent_span_id: Some(parent.span_id),
            sampled: parent.sampled,
            trace_state: tracestate.and_then(clean_trace_state),
            link: None,
        })
    }

    /// A new trace (sampled by `ratio`) that keeps the caller's span as a
    /// [`link`](TraceContext::link): for public edges, where callers must
    /// not choose trace ids or force sampling. `None` when the header is
    /// not a valid `traceparent`.
    pub fn linked_to(traceparent: &str, ratio: f64) -> Option<Self> {
        let parent = parse_traceparent(traceparent)?;
        let mut root = Self::root_sampled(ratio);
        root.link = Some(SpanLink {
            trace_id: parent.trace_id,
            span_id: parent.span_id,
        });
        Some(root)
    }

    /// A span under this one: same trace, sampling and `tracestate`, a new
    /// span id, this span as the parent.
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id,
            span_id: SpanId::random(),
            parent_span_id: Some(self.span_id),
            sampled: self.sampled,
            trace_state: self.trace_state.clone(),
            link: None,
        }
    }

    /// The `traceparent` header value for this span:
    /// `00-<trace id>-<span id>-<01 sampled | 00>`.
    pub fn traceparent(&self) -> String {
        let flags = if self.sampled { "01" } else { "00" };
        format!("00-{}-{}-{flags}", self.trace_id, self.span_id)
    }
}

/// How a service starts the trace of each request it receives: the
/// propagation keys of the `observe.tracing` configuration section
/// (`enabled`, `trust_incoming`, `sample_ratio`), which navree-http reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TracePolicy {
    /// Trace and propagate at all (default `true`).
    pub enabled: bool,
    /// Continue traces from incoming `traceparent` headers (default
    /// `true`); `false` starts a new trace linked to the caller's span.
    pub trust_incoming: bool,
    /// The share of new traces that are sampled, 0 to 1 (default `1.0`).
    pub sample_ratio: f64,
}

impl Default for TracePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            trust_incoming: true,
            sample_ratio: 1.0,
        }
    }
}

/// The variable `observe.tracing.sample_ratio` falls back to when the section
/// leaves it out: OpenTelemetry's sampler argument.
pub const SAMPLE_RATIO_ENV: &str = "OTEL_TRACES_SAMPLER_ARG";

/// The keys of the `observe.tracing` section a [`TracePolicy`] is made of,
/// as the configuration's typed path reads them (`None` for a key the
/// section leaves out):
/// `config.section_or_default::<TracePolicySection>(TracePolicy::SECTION)`.
/// The section's other keys (the exporter's) are left to the observe module.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct TracePolicySection {
    /// `enabled`.
    pub enabled: Option<bool>,
    /// `trust_incoming`.
    pub trust_incoming: Option<bool>,
    /// `sample_ratio`.
    pub sample_ratio: Option<f64>,
}

impl TracePolicy {
    /// The configuration section the policy is read from.
    pub const SECTION: &'static str = "observe.tracing";

    /// The policy of `section`, the keys of `observe.tracing` as the
    /// configuration read them: navree-http, navree-queue, navree-schedule,
    /// navree-micro and the observe module's `TracingOptions` all read the
    /// keys this way, so they cannot disagree. A section that leaves
    /// `sample_ratio` out takes it from [`SAMPLE_RATIO_ENV`] (read through
    /// `env`; a blank value is ignored). A ratio outside 0 to 1 is an error
    /// naming its key or variable.
    pub fn from_section(
        section: TracePolicySection,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<TracePolicy, String> {
        let defaults = TracePolicy::default();
        let in_range = |ratio: &f64| (0.0..=1.0).contains(ratio);
        let sample_ratio = match section.sample_ratio {
            Some(ratio) if in_range(&ratio) => ratio,
            Some(ratio) => {
                return Err(format!(
                    "{}.sample_ratio must be from 0 to 1, got {ratio}",
                    Self::SECTION
                ));
            }
            None => match env(SAMPLE_RATIO_ENV).filter(|value| !value.trim().is_empty()) {
                Some(value) => value
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(in_range)
                    .ok_or_else(|| {
                        format!("{SAMPLE_RATIO_ENV} must be a number from 0 to 1, got `{value}`")
                    })?,
                None => defaults.sample_ratio,
            },
        };
        Ok(TracePolicy {
            enabled: section.enabled.unwrap_or(defaults.enabled),
            trust_incoming: section.trust_incoming.unwrap_or(defaults.trust_incoming),
            sample_ratio,
        })
    }

    /// The trace of a new unit of work that arrived with these
    /// `traceparent` and `tracestate` headers: the caller's trace when it
    /// is trusted and valid, else a new one (linked to an untrusted
    /// caller); `None` when tracing is off.
    pub fn start(
        &self,
        traceparent: Option<&str>,
        tracestate: Option<&str>,
    ) -> Option<TraceContext> {
        if !self.enabled {
            return None;
        }
        let incoming = traceparent.and_then(|header| {
            if self.trust_incoming {
                TraceContext::continue_from(header, tracestate)
            } else {
                TraceContext::linked_to(header, self.sample_ratio)
            }
        });
        Some(incoming.unwrap_or_else(|| TraceContext::root_sampled(self.sample_ratio)))
    }

    /// The trace of work the app handed to itself (a queued job) with the
    /// `traceparent` it recorded when it handed it over: that trace,
    /// continued whatever `trust_incoming` says (the header never came
    /// from outside), else a new one; `None` when tracing is off.
    pub fn resume(&self, traceparent: Option<&str>) -> Option<TraceContext> {
        if !self.enabled {
            return None;
        }
        let resumed = traceparent.and_then(|header| TraceContext::continue_from(header, None));
        Some(resumed.unwrap_or_else(|| TraceContext::root_sampled(self.sample_ratio)))
    }
}

/// Parses a `traceparent` header: `version-traceid-parentid-flags`.
/// Version `ff` is invalid; version `00` has exactly four fields; a later
/// version is read by its first four fields.
fn parse_traceparent(header: &str) -> Option<Parent> {
    let header = header.trim();
    let mut fields = header.split('-');
    let version = fields.next()?;
    let trace = fields.next()?;
    let span = fields.next()?;
    let flags = fields.next()?;
    let mut version_byte = [0u8; 1];
    decode_hex(version, &mut version_byte).ok()?;
    if version_byte[0] == 0xff || (version_byte[0] == 0 && fields.next().is_some()) {
        return None;
    }
    let mut flag_byte = [0u8; 1];
    decode_hex(flags, &mut flag_byte).ok()?;
    Some(Parent {
        trace_id: trace.parse().ok()?,
        span_id: span.parse().ok()?,
        sampled: flag_byte[0] & 1 == 1,
    })
}

/// A `tracestate` worth passing on: trimmed, non-empty, at most 512 characters.
fn clean_trace_state(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= MAX_TRACE_STATE).then(|| value.to_owned())
}

/// Whether a new trace with id `trace_id` is recorded when a share `ratio`
/// (clamped to 0..=1) of traces is: decided from the trace id's last eight
/// bytes, so every service makes the same decision for the same trace.
pub fn sample_trace(trace_id: TraceId, ratio: f64) -> bool {
    if ratio.is_nan() || ratio <= 0.0 {
        return false;
    }
    if ratio >= 1.0 {
        return true;
    }
    let bytes = trace_id.to_bytes();
    let mut low = [0u8; 8];
    low.copy_from_slice(&bytes[8..]);
    // 63 bits, so the bound below fits without overflow.
    let value = u64::from_be_bytes(low) >> 1;
    let bound = (ratio * (1u64 << 63) as f64) as u64;
    value < bound
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_digits_decode() {
        let mut out = [0u8; 2];
        decode_hex("0aff", &mut out).unwrap();
        assert_eq!(out, [0x0a, 0xff]);
        assert!(decode_hex("0aF0", &mut out).is_err());
        assert!(decode_hex("0a", &mut out).is_err());
    }

    #[test]
    fn ids_serialize_as_hex_strings() {
        let id: TraceId = "4bf92f3577b34da6a3ce929d0e0e4736".parse().unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"4bf92f3577b34da6a3ce929d0e0e4736\"");
        let back: TraceId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
        assert!(serde_json::from_str::<SpanId>("\"zz\"").is_err());
    }
}
