//! W3C Trace Context: `traceparent` parsing and formatting, ids and sampling.

use navree_telemetry::{SpanId, TraceContext, TraceId, sample_trace};

const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const PARENT: &str = "00f067aa0ba902b7";

#[test]
fn ids_round_trip_through_lowercase_hex() {
    let trace: TraceId = TRACE.parse().expect("valid trace id");
    assert_eq!(trace.to_string(), TRACE);
    let span: SpanId = PARENT.parse().expect("valid span id");
    assert_eq!(span.to_string(), PARENT);
    assert_eq!(trace.to_bytes().len(), 16);
    assert_eq!(span.to_bytes().len(), 8);
    assert_eq!(TraceId::from_bytes(trace.to_bytes()), Some(trace));
    assert_eq!(SpanId::from_bytes(span.to_bytes()), Some(span));
}

#[test]
fn invalid_ids_are_refused() {
    for bad in [
        "",
        "4bf92f3577b34da6a3ce929d0e0e473",   // 31 digits
        "4bf92f3577b34da6a3ce929d0e0e47366", // 33 digits
        "4BF92F3577B34DA6A3CE929D0E0E4736",  // uppercase
        "4bf92f3577b34da6a3ce929d0e0e473g",  // not hex
        "00000000000000000000000000000000",  // all zero
    ] {
        assert!(bad.parse::<TraceId>().is_err(), "{bad:?} must be refused");
    }
    for bad in [
        "",
        "00f067aa0ba902b",
        "0000000000000000",
        "00F067AA0BA902B7",
    ] {
        assert!(bad.parse::<SpanId>().is_err(), "{bad:?} must be refused");
    }
    assert_eq!(TraceId::from_bytes([0; 16]), None);
    assert_eq!(SpanId::from_bytes([0; 8]), None);
}

#[test]
fn random_ids_are_valid_and_distinct() {
    let a = TraceId::random();
    let b = TraceId::random();
    assert_ne!(a, b);
    assert_ne!(SpanId::random(), SpanId::random());
    assert!(a.to_string().parse::<TraceId>().is_ok());
}

#[test]
fn an_incoming_traceparent_is_continued_with_a_new_span() {
    let header = format!("00-{TRACE}-{PARENT}-01");
    let context = TraceContext::continue_from(&header, Some("congo=t61rcWkgMzE")).expect("valid");
    assert_eq!(context.trace_id.to_string(), TRACE);
    assert_eq!(
        context.parent_span_id.map(|id| id.to_string()).as_deref(),
        Some(PARENT)
    );
    assert_ne!(
        context.span_id.to_string(),
        PARENT,
        "the server span is a new span"
    );
    assert!(context.sampled);
    assert_eq!(context.trace_state.as_deref(), Some("congo=t61rcWkgMzE"));
    assert_eq!(
        context.traceparent(),
        format!("00-{TRACE}-{}-01", context.span_id)
    );
}

#[test]
fn the_sampled_flag_follows_the_caller() {
    let unsampled = TraceContext::continue_from(&format!("00-{TRACE}-{PARENT}-00"), None).unwrap();
    assert!(!unsampled.sampled);
    assert!(unsampled.traceparent().ends_with("-00"));
    // Other flag bits are ignored; only bit 0 is "sampled".
    let other_bits = TraceContext::continue_from(&format!("00-{TRACE}-{PARENT}-09"), None).unwrap();
    assert!(other_bits.sampled);
}

#[test]
fn malformed_traceparent_headers_are_ignored() {
    for bad in [
        String::new(),
        "garbage".to_owned(),
        format!("00-{TRACE}-{PARENT}"),    // no flags
        format!("00-{TRACE}-{PARENT}-1"),  // short flags
        format!("ff-{TRACE}-{PARENT}-01"), // version ff is invalid
        format!("0-{TRACE}-{PARENT}-01"),  // short version
        format!("00-00000000000000000000000000000000-{PARENT}-01"), // zero trace id
        format!("00-{TRACE}-0000000000000000-01"), // zero parent id
        format!("00-{TRACE}-{PARENT}-01-extra"), // version 00 has exactly 4 fields
        format!("00-{}-{PARENT}-01", TRACE.to_uppercase()), // uppercase
    ] {
        assert!(
            TraceContext::continue_from(&bad, None).is_none(),
            "{bad:?} must be ignored"
        );
    }
}

#[test]
fn a_future_version_is_read_by_its_first_four_fields() {
    let context = TraceContext::continue_from(
        &format!("01-{TRACE}-{PARENT}-01-what-the-future-holds"),
        None,
    )
    .expect("higher versions are parsed as version 00");
    assert_eq!(context.trace_id.to_string(), TRACE);
}

#[test]
fn an_overlong_tracestate_is_dropped() {
    let long = "a=".to_owned() + &"b".repeat(600);
    let context =
        TraceContext::continue_from(&format!("00-{TRACE}-{PARENT}-01"), Some(&long)).unwrap();
    assert_eq!(context.trace_state, None);
    let empty =
        TraceContext::continue_from(&format!("00-{TRACE}-{PARENT}-01"), Some("  ")).unwrap();
    assert_eq!(empty.trace_state, None);
}

#[test]
fn a_root_context_has_no_parent() {
    let root = TraceContext::root(true);
    assert_eq!(root.parent_span_id, None);
    assert!(root.sampled);
    assert!(root.traceparent().starts_with("00-"));
    assert!(root.traceparent().ends_with("-01"));
    assert!(!TraceContext::root(false).sampled);
}

#[test]
fn a_child_shares_the_trace_and_points_at_its_parent() {
    let root = TraceContext::root(true);
    let child = root.child();
    assert_eq!(child.trace_id, root.trace_id);
    assert_eq!(child.parent_span_id, Some(root.span_id));
    assert_ne!(child.span_id, root.span_id);
    assert_eq!(child.sampled, root.sampled);
}

#[test]
fn a_new_trace_can_link_to_an_untrusted_caller() {
    let header = format!("00-{TRACE}-{PARENT}-01");
    let linked = TraceContext::linked_to(&header, 1.0).expect("valid header");
    assert_ne!(linked.trace_id.to_string(), TRACE, "a new trace starts");
    assert_eq!(linked.parent_span_id, None);
    let link = linked.link.expect("the caller's span is kept as a link");
    assert_eq!(link.trace_id.to_string(), TRACE);
    assert_eq!(link.span_id.to_string(), PARENT);
}

#[test]
fn sampling_is_decided_by_the_trace_id() {
    let id = TraceId::random();
    assert!(sample_trace(id, 1.0));
    assert!(!sample_trace(id, 0.0));
    assert_eq!(
        sample_trace(id, 0.5),
        sample_trace(id, 0.5),
        "deterministic"
    );
    // Out-of-range ratios are clamped.
    assert!(sample_trace(id, 7.0));
    assert!(!sample_trace(id, -1.0));
    let sampled = (0..4000)
        .filter(|_| sample_trace(TraceId::random(), 0.25))
        .count();
    assert!(
        (700..1300).contains(&sampled),
        "about a quarter of 4000 traces, got {sampled}"
    );
    let root = TraceContext::root_sampled(0.0);
    assert!(!root.sampled);
    assert!(TraceContext::root_sampled(1.0).sampled);
}

mod policy {
    use navree_telemetry::TracePolicy;

    use super::{PARENT, TRACE};

    #[test]
    fn the_default_policy_continues_trusted_callers() {
        let policy = TracePolicy::default();
        assert!(policy.enabled && policy.trust_incoming);
        assert_eq!(policy.sample_ratio, 1.0);
        let header = format!("00-{TRACE}-{PARENT}-01");
        let trace = policy.start(Some(&header), Some("k=v")).expect("a trace");
        assert_eq!(trace.trace_id.to_string(), TRACE);
        assert_eq!(trace.trace_state.as_deref(), Some("k=v"));
        let fresh = policy.start(None, None).expect("a new trace");
        assert_eq!(fresh.parent_span_id, None);
        assert!(fresh.sampled);
        let garbage = policy.start(Some("garbage"), None).expect("a new trace");
        assert_eq!(garbage.parent_span_id, None);
    }

    #[test]
    fn untrusted_callers_get_a_new_linked_trace() {
        let policy = TracePolicy {
            trust_incoming: false,
            ..TracePolicy::default()
        };
        let header = format!("00-{TRACE}-{PARENT}-00");
        let trace = policy.start(Some(&header), None).unwrap();
        assert_ne!(trace.trace_id.to_string(), TRACE);
        assert!(trace.sampled, "the caller cannot turn sampling off either");
        assert_eq!(trace.link.unwrap().span_id.to_string(), PARENT);
    }

    #[test]
    fn work_handed_over_inside_the_app_resumes_its_trace() {
        // A queued job resumes the trace it was dispatched in, even on a
        // public edge: the traceparent came from the app itself.
        let policy = TracePolicy {
            trust_incoming: false,
            sample_ratio: 0.0,
            ..TracePolicy::default()
        };
        let header = format!("00-{TRACE}-{PARENT}-01");
        let trace = policy.resume(Some(&header)).expect("a trace");
        assert_eq!(trace.trace_id.to_string(), TRACE);
        assert_eq!(trace.parent_span_id.unwrap().to_string(), PARENT);
        assert!(trace.sampled, "the dispatcher's decision holds");
        assert_eq!(trace.link, None);

        let fresh = policy.resume(None).expect("a new trace");
        assert_eq!(fresh.parent_span_id, None);
        assert!(!fresh.sampled, "a new trace follows the ratio");
        let garbage = policy.resume(Some("garbage")).expect("a new trace");
        assert_eq!(garbage.parent_span_id, None);

        let off = TracePolicy {
            enabled: false,
            ..TracePolicy::default()
        };
        assert_eq!(off.resume(Some(&header)), None);
    }

    #[test]
    fn a_disabled_policy_starts_nothing() {
        let policy = TracePolicy {
            enabled: false,
            ..TracePolicy::default()
        };
        assert_eq!(
            policy.start(Some(&format!("00-{TRACE}-{PARENT}-01")), None),
            None
        );
        assert_eq!(policy.start(None, None), None);
    }

    fn settings(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn settings_are_read_from_the_observe_tracing_keys() {
        let policy = TracePolicy::from_settings(settings(&[]), |_| None).unwrap();
        assert_eq!(policy, TracePolicy::default());
        let policy = TracePolicy::from_settings(
            settings(&[
                ("observe.tracing.enabled", "off"),
                ("observe.tracing.trust_incoming", "false"),
                ("observe.tracing.sample_ratio", "0.25"),
            ]),
            |_| None,
        )
        .unwrap();
        assert!(!policy.enabled && !policy.trust_incoming);
        assert_eq!(policy.sample_ratio, 0.25);
    }

    #[test]
    fn the_otel_sampler_arg_is_the_fallback_ratio() {
        let env = |name: &str| (name == "OTEL_TRACES_SAMPLER_ARG").then(|| "0.5".to_owned());
        let policy = TracePolicy::from_settings(settings(&[]), env).unwrap();
        assert_eq!(policy.sample_ratio, 0.5);
        let policy =
            TracePolicy::from_settings(settings(&[("observe.tracing.sample_ratio", "0.1")]), env)
                .unwrap();
        assert_eq!(policy.sample_ratio, 0.1, "the section wins");
    }

    #[test]
    fn invalid_settings_name_their_key() {
        let error = TracePolicy::from_settings(
            settings(&[("observe.tracing.sample_ratio", "1.5")]),
            |_| None,
        )
        .unwrap_err();
        assert!(error.contains("observe.tracing.sample_ratio"), "{error}");
        let error =
            TracePolicy::from_settings(settings(&[("observe.tracing.enabled", "maybe")]), |_| None)
                .unwrap_err();
        assert!(error.contains("observe.tracing.enabled"), "{error}");
        let error =
            TracePolicy::from_settings(settings(&[]), |_| Some("lots".to_owned())).unwrap_err();
        assert!(error.contains("OTEL_TRACES_SAMPLER_ARG"), "{error}");
    }

    #[test]
    fn new_traces_follow_the_ratio_and_callers_keep_theirs() {
        let policy = TracePolicy {
            sample_ratio: 0.0,
            ..TracePolicy::default()
        };
        assert!(!policy.start(None, None).unwrap().sampled);
        let sampled = format!("00-{TRACE}-{PARENT}-01");
        assert!(policy.start(Some(&sampled), None).unwrap().sampled);
    }
}
