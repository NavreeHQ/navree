//! [`TraceContext`]: W3C trace context (`traceparent`, `tracestate`) that
//! travels with every message.

use std::fmt;

use uuid::Uuid;

/// A W3C trace context: the trace a message belongs to and the span that
/// sent it.
///
/// A message sent while an HTTP request or another message is handled
/// continues that trace: it carries the same trace id and a new span id
/// ([`child`](TraceContext::child)). A message sent outside any trace starts
/// a new one ([`new_root`](TraceContext::new_root)).
///
/// ```
/// use navree_micro::TraceContext;
///
/// let parent = TraceContext::parse(
///     "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
///     Some("vendor=1"),
/// )
/// .unwrap();
/// let child = parent.child();
/// assert_eq!(child.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
/// assert_ne!(child.span_id(), parent.span_id());
/// assert_eq!(child.tracestate(), Some("vendor=1"));
/// assert!(child.traceparent().starts_with("00-4bf92f3577b34da6a3ce929d0e0e4736-"));
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct TraceContext {
    trace_id: [u8; 16],
    span_id: [u8; 8],
    flags: u8,
    state: Option<String>,
}

impl TraceContext {
    /// Parses a `traceparent` header (version `00`) and an optional
    /// `tracestate`. `None` for anything malformed, including all-zero ids.
    pub fn parse(traceparent: &str, tracestate: Option<&str>) -> Option<TraceContext> {
        let mut parts = traceparent.trim().split('-');
        let version = parts.next()?;
        let trace = parts.next()?;
        let span = parts.next()?;
        let flags = parts.next()?;
        if version.len() != 2 || version == "ff" || !is_hex(version) {
            return None;
        }
        // Version 00 has exactly four fields.
        if version == "00" && parts.next().is_some() {
            return None;
        }
        let trace_id: [u8; 16] = decode_hex(trace)?.try_into().ok()?;
        let span_id: [u8; 8] = decode_hex(span)?.try_into().ok()?;
        let flags: [u8; 1] = decode_hex(flags)?.try_into().ok()?;
        if trace_id == [0; 16] || span_id == [0; 8] {
            return None;
        }
        Some(TraceContext {
            trace_id,
            span_id,
            flags: flags[0],
            state: tracestate
                .map(str::trim)
                .filter(|state| !state.is_empty())
                .map(str::to_owned),
        })
    }

    /// A new trace (random ids), sampled.
    pub fn new_root() -> TraceContext {
        let trace = Uuid::new_v4();
        TraceContext {
            trace_id: *trace.as_bytes(),
            span_id: new_span_id(),
            flags: 0x01,
            state: None,
        }
    }

    /// The next span of the same trace: same trace id, flags and state, a
    /// new span id.
    pub fn child(&self) -> TraceContext {
        TraceContext {
            span_id: new_span_id(),
            ..self.clone()
        }
    }

    /// The trace id, 32 lower-case hex digits.
    pub fn trace_id(&self) -> String {
        encode_hex(&self.trace_id)
    }

    /// The span id, 16 lower-case hex digits.
    pub fn span_id(&self) -> String {
        encode_hex(&self.span_id)
    }

    /// Whether the trace is sampled (flag `01`).
    pub fn is_sampled(&self) -> bool {
        self.flags & 0x01 == 0x01
    }

    /// The `traceparent` header value.
    pub fn traceparent(&self) -> String {
        format!(
            "00-{}-{}-{:02x}",
            self.trace_id(),
            self.span_id(),
            self.flags
        )
    }

    /// The `tracestate` header value, if any.
    pub fn tracestate(&self) -> Option<&str> {
        self.state.as_deref()
    }
}

impl fmt::Debug for TraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TraceContext")
            .field(&self.traceparent())
            .finish()
    }
}

fn new_span_id() -> [u8; 8] {
    loop {
        let bytes = Uuid::new_v4().into_bytes();
        let mut id = [0u8; 8];
        id.copy_from_slice(&bytes[8..]);
        if id != [0; 8] {
            return id;
        }
    }
}

fn is_hex(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || !is_hex(text) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    #[test]
    fn parses_and_formats_traceparent() {
        let trace = TraceContext::parse(VALID, None).unwrap();
        assert_eq!(trace.traceparent(), VALID);
        assert_eq!(trace.span_id(), "00f067aa0ba902b7");
        assert!(trace.is_sampled());
        assert_eq!(trace.tracestate(), None);
        let unsampled = TraceContext::parse(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00",
            Some(" "),
        )
        .unwrap();
        assert!(!unsampled.is_sampled());
        assert_eq!(unsampled.tracestate(), None, "blank state dropped");
    }

    #[test]
    fn rejects_malformed_values() {
        for bad in [
            "",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
            "00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01",
            "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
            "zz-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        ] {
            assert!(TraceContext::parse(bad, None).is_none(), "{bad}");
        }
    }

    #[test]
    fn children_share_the_trace_and_roots_are_new() {
        let root = TraceContext::new_root();
        assert!(root.is_sampled());
        assert_eq!(TraceContext::parse(&root.traceparent(), None), Some(root.clone()));
        let child = root.child();
        assert_eq!(child.trace_id(), root.trace_id());
        assert_ne!(child.span_id(), root.span_id());
        assert_ne!(TraceContext::new_root().trace_id(), root.trace_id());
    }
}
