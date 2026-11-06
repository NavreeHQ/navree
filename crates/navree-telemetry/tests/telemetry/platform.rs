//! The platform events: AI calls, broadcasts, storage operations, webhook
//! deliveries, searches, sign-ins and Navree UI operations.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use navree_telemetry::{
    AiCall, BroadcastSent, EventKind, Outcome, SearchMode, SearchQueried, SignInEvent,
    SignInMethod, StorageOp, StorageOperation, Telemetry, TelemetryEvent, TelemetryRecorder,
    UiOperation, UiOperationKind, WebhookDelivery,
};
use serde_json::{Value, json};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn json_of(event: &TelemetryEvent) -> Value {
    serde_json::to_value(event).unwrap()
}

#[test]
fn each_kind_has_its_own_name_and_serializes_with_a_type_tag() {
    let events = [
        (
            TelemetryEvent::AiCall(AiCall::new("openai", "gpt-5", "generate", ms(5))),
            EventKind::AiCall,
            "ai_call",
        ),
        (
            TelemetryEvent::Broadcast(BroadcastSent::new(["a"], "e", 0, ms(1))),
            EventKind::Broadcast,
            "broadcast",
        ),
        (
            TelemetryEvent::StorageOp(StorageOp::new(
                "uploads",
                "local",
                StorageOperation::Read,
                "a.txt",
                ms(1),
            )),
            EventKind::StorageOp,
            "storage_op",
        ),
        (
            TelemetryEvent::WebhookDelivery(WebhookDelivery::new("ep", "e", 1, 200, ms(1))),
            EventKind::WebhookDelivery,
            "webhook_delivery",
        ),
        (
            TelemetryEvent::Search(SearchQueried::new(
                "memory",
                "deals",
                SearchMode::Text,
                "x",
                ms(1),
            )),
            EventKind::Search,
            "search",
        ),
        (
            TelemetryEvent::SignIn(SignInEvent::succeeded(
                SignInMethod::Passkey,
                "u-1",
                ms(1),
            )),
            EventKind::SignIn,
            "sign_in",
        ),
        (
            TelemetryEvent::UiOperation(UiOperation::new(
                "/x",
                UiOperationKind::Action,
                200,
                1,
                ms(1),
            )),
            EventKind::UiOperation,
            "ui_operation",
        ),
    ];
    for (event, kind, name) in events {
        assert_eq!(event.kind(), kind);
        assert_eq!(kind.as_str(), name);
        assert_eq!(kind.to_string(), name);
        assert_eq!(json_of(&event)["type"], name);
        let back: TelemetryEvent = serde_json::from_value(json_of(&event)).unwrap();
        assert_eq!(back, event, "{name} round-trips");
        assert!(
            EventKind::ALL.contains(&kind),
            "{name} is in EventKind::ALL"
        );
    }
}

#[test]
fn every_kind_has_a_distinct_interest_bit() {
    let mut interest = navree_telemetry::Interest::NONE;
    for kind in EventKind::ALL {
        assert!(!interest.contains(kind), "{kind} shares a bit");
        interest = interest.with(kind);
    }
}

#[test]
fn an_ai_call_serializes_its_tokens_cost_and_latency() {
    let call = AiCall::new("anthropic", "claude-sonnet-4-5", "generate", ms(1500))
        .tokens(1200, 340)
        .cached_tokens(800)
        .cost_micros(5_400);
    let json = json_of(&TelemetryEvent::AiCall(call));
    assert_eq!(json["provider"], "anthropic");
    assert_eq!(json["model"], "claude-sonnet-4-5");
    assert_eq!(json["operation"], "generate");
    assert_eq!(json["input_tokens"], 1200);
    assert_eq!(json["output_tokens"], 340);
    assert_eq!(json["cached_input_tokens"], 800);
    assert_eq!(json["cost_micros"], 5_400);
    assert_eq!(json["duration"], 1500.0);
    assert_eq!(json["outcome"], "success");
    assert_eq!(json["error"], Value::Null);

    let failed = AiCall::new("openai", "gpt-5", "stream", ms(20)).failed("rate_limited");
    assert_eq!(failed.outcome, Outcome::Failure);
    assert_eq!(failed.error.as_deref(), Some("rate_limited"));
    assert_eq!(failed.input_tokens, None, "unknown stays unknown");
    assert_eq!(failed.cost_micros, None);
}

#[test]
fn a_broadcast_names_its_channels_and_event_but_has_no_data_field() {
    let sent = BroadcastSent::new(["private-crm.deals", "presence-room.7"], "deal.updated", 5, ms(2));
    let json = json_of(&TelemetryEvent::Broadcast(sent));
    assert_eq!(
        json["channels"],
        json!(["private-crm.deals", "presence-room.7"])
    );
    assert_eq!(json["event"], "deal.updated");
    assert_eq!(json["delivered"], 5);
    let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    assert!(!keys.iter().any(|key| *key == "data" || *key == "payload"));
}

#[test]
fn storage_operations_use_their_snake_case_names() {
    for (operation, name) in [
        (StorageOperation::Read, "read"),
        (StorageOperation::Write, "write"),
        (StorageOperation::Delete, "delete"),
        (StorageOperation::List, "list"),
        (StorageOperation::Copy, "copy"),
        (StorageOperation::Move, "move"),
        (StorageOperation::Stat, "stat"),
    ] {
        assert_eq!(operation.as_str(), name);
        assert_eq!(serde_json::to_value(operation).unwrap(), name);
    }
    let op = StorageOp::new("media", "s3", StorageOperation::Write, "a/b.png", ms(30)).bytes(2048);
    let json = json_of(&TelemetryEvent::StorageOp(op));
    assert_eq!(json["disk"], "media");
    assert_eq!(json["driver"], "s3");
    assert_eq!(json["operation"], "write");
    assert_eq!(json["bytes"], 2048);
}

#[test]
fn a_webhook_delivery_is_a_success_only_for_a_2xx_answer() {
    for (status, outcome) in [
        (200, Outcome::Success),
        (204, Outcome::Success),
        (301, Outcome::Failure),
        (410, Outcome::Failure),
        (503, Outcome::Failure),
    ] {
        let delivery = WebhookDelivery::new("ep-1", "invoice.paid", 2, status, ms(5));
        assert_eq!(delivery.outcome, outcome, "{status}");
        assert_eq!(delivery.status, Some(status));
        assert_eq!(delivery.attempt, 2);
    }
    let down = WebhookDelivery::unanswered("ep-1", "invoice.paid", 3, ms(10_000), "timed out");
    assert_eq!(down.status, None);
    assert_eq!(down.outcome, Outcome::Failure);
    assert_eq!(down.error.as_deref(), Some("timed out"));
}

#[test]
fn a_search_records_the_length_of_the_term_never_the_term() {
    let search =
        SearchQueried::new("meilisearch", "contacts", SearchMode::Hybrid, "Harpreet Kaur", ms(6))
            .hits(12);
    assert_eq!(search.term_length, 13);
    let text = serde_json::to_string(&TelemetryEvent::Search(search)).unwrap();
    assert!(!text.contains("Harpreet"), "{text}");
    assert!(!text.contains("Kaur"), "{text}");
    assert!(text.contains("\"term_length\":13"), "{text}");
    assert!(text.contains("\"mode\":\"hybrid\""), "{text}");

    // Characters, not bytes.
    let hindi = SearchQueried::new("memory", "contacts", SearchMode::Text, "लुधियाना", ms(1));
    assert_eq!(hindi.term_length, 8);
}

#[test]
fn a_sign_in_never_holds_an_email_or_a_secret() {
    let worked = SignInEvent::succeeded(SignInMethod::Social, "0199c1d2", ms(120)).provider("github");
    let refused = SignInEvent::refused(SignInMethod::Password, "invalid_credentials", ms(80));
    let throttled = SignInEvent::refused(SignInMethod::TwoFactor, "throttled", ms(1));

    let worked_json = serde_json::to_string(&TelemetryEvent::SignIn(worked.clone())).unwrap();
    assert!(worked_json.contains("\"user_id\":\"0199c1d2\""), "{worked_json}");
    assert!(worked_json.contains("\"provider\":\"github\""), "{worked_json}");
    assert!(worked_json.contains("\"method\":\"social\""), "{worked_json}");

    // A failed sign-in names no user, so a probe for an address leaves none behind.
    assert_eq!(refused.user_id, None);
    assert_eq!(refused.outcome, Outcome::Failure);
    assert_eq!(refused.reason.as_deref(), Some("invalid_credentials"));
    assert_eq!(throttled.method.as_str(), "two_factor");

    // The event has no field that could hold an address or a secret.
    for event in [&worked, &refused, &throttled] {
        let json = serde_json::to_value(event).unwrap();
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["duration", "method", "outcome", "provider", "reason", "user_id"]
        );
    }

    let methods: Vec<&str> = [
        SignInMethod::Password,
        SignInMethod::TwoFactor,
        SignInMethod::MagicLink,
        SignInMethod::Code,
        SignInMethod::Passkey,
        SignInMethod::Social,
        SignInMethod::Sso,
        SignInMethod::TokenExchange,
    ]
    .into_iter()
    .map(SignInMethod::as_str)
    .collect();
    assert_eq!(
        methods,
        [
            "password",
            "two_factor",
            "magic_link",
            "code",
            "passkey",
            "social",
            "sso",
            "token_exchange"
        ]
    );
}

#[test]
fn a_ui_operation_has_a_name_a_kind_and_counts_but_no_values() {
    let op = UiOperation::new("/deals/{id}/stage", UiOperationKind::TableQuery, 200, 3, ms(11));
    let json = json_of(&TelemetryEvent::UiOperation(op));
    assert_eq!(json["name"], "/deals/{id}/stage");
    assert_eq!(json["kind"], "table_query");
    assert_eq!(json["ops"], 3);
    assert_eq!(json["status"], 200);
    assert_eq!(json["duration"], 11.0);
    for (kind, name) in [
        (UiOperationKind::FormPost, "form_post"),
        (UiOperationKind::Action, "action"),
        (UiOperationKind::TableQuery, "table_query"),
        (UiOperationKind::LiveRegion, "live_region"),
        (UiOperationKind::Fragment, "fragment"),
        (UiOperationKind::Upload, "upload"),
    ] {
        assert_eq!(kind.as_str(), name);
    }
}

#[test]
fn the_one_line_emits_reach_a_recorder() {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);

    telemetry.ai_call(|| AiCall::new("ollama", "llama3", "embed", ms(7)).tokens(10, 0));
    telemetry.broadcast(|| BroadcastSent::new(["room"], "message", 2, ms(1)));
    telemetry.storage_op(|| {
        StorageOp::new("local", "local", StorageOperation::Delete, "tmp/x", ms(1))
    });
    telemetry.webhook_delivery(|| WebhookDelivery::new("ep", "e", 1, 500, ms(9)));
    telemetry.search(|| SearchQueried::new("memory", "deals", SearchMode::Across, "x", ms(1)));
    telemetry.sign_in(|| SignInEvent::refused(SignInMethod::MagicLink, "expired", ms(1)));
    telemetry.ui_operation(|| UiOperation::new("/x", UiOperationKind::LiveRegion, 200, 1, ms(1)));

    assert_eq!(recorder.len(), 7);
    assert_eq!(recorder.ai_calls()[0].operation, "embed");
    assert_eq!(recorder.broadcasts()[0].channels, ["room"]);
    assert_eq!(recorder.storage_ops()[0].operation, StorageOperation::Delete);
    assert_eq!(recorder.webhook_deliveries()[0].outcome, Outcome::Failure);
    assert_eq!(recorder.searches()[0].mode, SearchMode::Across);
    assert_eq!(recorder.sign_ins()[0].reason.as_deref(), Some("expired"));
    assert_eq!(recorder.ui_operations()[0].kind, UiOperationKind::LiveRegion);
    assert_eq!(recorder.of_kind(EventKind::SignIn).len(), 1);
}

#[test]
fn the_one_line_emits_build_nothing_for_a_sink_that_does_not_want_them() {
    let built = AtomicUsize::new(0);
    let count = || {
        built.fetch_add(1, Ordering::SeqCst);
    };

    // No registry at all.
    let off = Telemetry::disabled();
    off.ai_call(|| {
        count();
        AiCall::new("a", "b", "c", ms(1))
    });
    off.sign_in(|| {
        count();
        SignInEvent::refused(SignInMethod::Password, "x", ms(1))
    });

    // A registry whose only sink wants requests.
    let recorder = TelemetryRecorder::only([EventKind::RequestHandled]);
    let narrow = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    narrow.broadcast(|| {
        count();
        BroadcastSent::new(["a"], "e", 0, ms(1))
    });
    narrow.storage_op(|| {
        count();
        StorageOp::new("d", "local", StorageOperation::List, "", ms(1))
    });
    narrow.webhook_delivery(|| {
        count();
        WebhookDelivery::new("ep", "e", 1, 200, ms(1))
    });
    narrow.search(|| {
        count();
        SearchQueried::new("memory", "i", SearchMode::Text, "t", ms(1))
    });
    narrow.ui_operation(|| {
        count();
        UiOperation::new("/x", UiOperationKind::Fragment, 200, 0, ms(1))
    });

    assert_eq!(built.load(Ordering::SeqCst), 0, "nothing was built");
    assert!(recorder.is_empty());
    assert!(!narrow.enabled(EventKind::AiCall));
}

#[test]
fn a_recorder_can_ask_for_only_the_new_kinds() {
    let recorder = TelemetryRecorder::only([EventKind::SignIn, EventKind::AiCall]);
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    telemetry.sign_in(|| SignInEvent::succeeded(SignInMethod::Sso, "u-9", ms(3)).provider("okta-1"));
    telemetry.search(|| SearchQueried::new("memory", "i", SearchMode::Text, "t", ms(1)));
    telemetry.ai_call(|| AiCall::new("fake", "fake", "generate", ms(1)));
    assert_eq!(recorder.len(), 2);
    assert_eq!(recorder.sign_ins()[0].provider.as_deref(), Some("okta-1"));
}
