//! The store as a native client gets it: which of its routes answer a screen,
//! which fall back to the web view and why, what the actions answer, and that a
//! browser asking the same routes gets HTML as before. The orders and the
//! inventory are read from their pages; the day, an order and the shelves are
//! screens written for native clients, and their trees are held here.
//!
//! `cargo test -p store-demo --features native-preview --test native -- --nocapture`
//! prints the coverage table.
#![cfg(feature = "native-preview")]

use navree::testing::{TempDirGuard, TestApp, temp_sqlite_url};
use navree::ui::native::protocol::input::{Input, TextKind};
use navree::ui::native::protocol::node::{self as n, Node as N};
use navree::ui::native::protocol::{Action, Command, Envelope, Op};
use navree::ui::native::{Answered, Coverage, Grade};
use store_demo::{Settings, app};

const PHONE: &str =
    "platform=android; os=16; form=phone; app=0.1.0; vocab=1; locale=en-IN; tz=Asia/Kolkata";

/// The app on a database of its own, written afresh, with nothing happening by itself.
async fn boot() -> (TestApp, TempDirGuard) {
    let (url, dir) = temp_sqlite_url();
    let settings = Settings {
        simulate_every: None,
        test_hooks: false,
    };
    (TestApp::from_builder(app(0, url, settings)).await, dir)
}

/// What a native client and a browser each get for `path`.
struct Asked {
    status: u16,
    location: Option<String>,
    native: String,
    html: String,
}

async fn ask(app: &TestApp, path: &str) -> Asked {
    let native = app
        .client()
        .get(path)
        .header("x-navree-native", "1")
        .header("x-navree-client", PHONE)
        .send()
        .await;
    let status = native.status().as_u16();
    let location = native.header("location").map(str::to_owned);
    let native = native.text();
    let html = app.client().get(path).send().await.text();
    Asked {
        status,
        location,
        native,
        html,
    }
}

fn envelope(body: &str) -> Envelope {
    serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}: {body:.300}"))
}

/// Every node under `node`, itself first.
fn all(node: &N) -> Vec<&N> {
    let mut out = vec![node];
    for child in node.children() {
        out.extend(all(child));
    }
    out
}

/// Keeps the screens the store writes for native clients, and the parts their
/// regions fetch, as files: what a client is sent, for a client's own tests.
///
/// ```text
/// NAVREE_NATIVE_DIR=/tmp/store-native cargo test -p store-demo --features native-preview --test native -- --ignored
/// ```
#[tokio::test]
#[ignore = "writes files: see the lines above"]
async fn the_screens_written_for_native_clients_are_kept_as_files() {
    let dir = std::env::var("NAVREE_NATIVE_DIR").expect("NAVREE_NATIVE_DIR names the directory");
    std::fs::create_dir_all(&dir).expect("the directory");
    // The data is written and the screens are drawn at one moment, so the files
    // are the same from one run to the next.
    store_demo::live::hold_clock(1_794_457_800);
    let (app, _dir) = boot().await;
    for (name, path) in [
        ("store-today", "/"),
        ("store-order", "/orders/1"),
        ("store-shelves", "/shelves"),
        ("store-part-figures", "/fragments/figures"),
        ("store-part-sales", "/fragments/sales"),
    ] {
        let asked = ask(&app, path).await;
        let sent: serde_json::Value = serde_json::from_str(&asked.native).expect("an envelope");
        assert!(
            sent.get("screen").is_some(),
            "{path}: {:.200}",
            asked.native
        );
        let pretty = serde_json::to_string_pretty(&sent).expect("JSON") + "\n";
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.json")),
            pretty,
        )
        .expect("the file written");
    }
}

#[tokio::test]
async fn the_store_answers_a_native_client() {
    let (app, _dir) = boot().await;
    let mut coverage = Coverage::new();
    let mut native_routes = Vec::new();
    for path in [
        "/",
        "/orders",
        "/orders?filter[status][in]=open&sort=total",
        "/orders/1",
        "/orders/999999",
        "/orders/1/steps",
        "/orders/1/others",
        "/inventory",
        "/inventory?page=2",
        "/shelves",
        "/fragments/figures",
        "/fragments/sales",
        "/fragments/shelves",
        "/fragments/activity",
    ] {
        let asked = ask(&app, path).await;
        let envelope = envelope(&asked.native);
        assert_eq!(envelope.navree, 1, "{path}");
        match (&envelope.screen, &envelope.web) {
            (Some(_), None) => {}
            (None, Some(web)) => assert_eq!(web.url, path, "{path}"),
            // The new rows of the live feed: ops that merge them into its list.
            (None, None) if !envelope.ops.is_empty() => {}
            _ => panic!(
                "{path}: neither a screen nor the web view: {:.300}",
                asked.native
            ),
        }
        let answered = Answered::of(&envelope);
        if answered.native {
            native_routes.push(path);
        }
        // A browser asking the same route gets a page or a fragment of HTML.
        assert!(
            asked.html.starts_with("<!doctype html>") || asked.html.starts_with('<'),
            "{path}: {:.80}",
            asked.html
        );
        assert!(!asked.html.contains("\"navree\":"), "{path}");
        coverage.add_answer(path, asked.html.len(), asked.native.len(), &answered);
    }
    println!("{coverage}");
    // Each native screen is graded, and what keeps one from its system is said:
    // the store says what its parts are, so every native screen belongs to it.
    assert_eq!(coverage.native(), native_routes.len());
    assert_eq!(
        coverage.graded(Grade::Idiom),
        native_routes.len(),
        "{coverage}"
    );
    assert_eq!(coverage.graded(Grade::Layout), 0, "{coverage}");
    assert_eq!(
        native_routes,
        [
            "/",
            "/orders",
            "/orders?filter[status][in]=open&sort=total",
            "/orders/1",
            "/orders/999999",
            "/orders/1/steps",
            "/orders/1/others",
            "/inventory",
            "/inventory?page=2",
            "/shelves",
            "/fragments/figures",
            "/fragments/sales",
            "/fragments/shelves",
            "/fragments/activity",
        ],
        "{coverage}"
    );
    // Nothing is left to the web view.
    let reasons = coverage.reasons();
    assert!(reasons.is_empty(), "{reasons:?}");
}

#[tokio::test]
async fn the_orders_are_a_table_a_phone_can_draw_as_a_list() {
    let (app, _dir) = boot().await;
    let asked = ask(&app, "/orders").await;
    let screen = envelope(&asked.native).screen.expect("a screen");
    assert_eq!(screen.title, "Orders");
    assert_eq!(screen.nav.as_deref(), Some("/orders"));
    assert_eq!(
        screen.back.as_ref().map(|back| back.href.as_str()),
        Some("/")
    );
    let Some(N::Region(region)) = screen.body.find("orders") else {
        panic!("no table");
    };
    // The table follows the shop live: a client listens and fetches it again.
    let live = region.live.as_ref().expect("a stream");
    assert!(live.src.starts_with("/broadcasting/stream"));
    assert_eq!(live.event.as_deref(), Some("orders"));
    let [N::Table(table)] = region.children.as_slice() else {
        panic!("not one table");
    };
    assert_eq!(table.rows.len(), 25);
    // What a narrow screen can go by: the column that names the record, and the
    // columns it may leave out.
    let titles: Vec<&str> = table
        .columns
        .iter()
        .filter(|column| column.role == Some(n::ColumnRole::Title))
        .map(|column| column.label.as_str())
        .collect();
    assert_eq!(titles, ["Order"]);
    let secondary: Vec<&str> = table
        .columns
        .iter()
        .filter(|column| column.secondary)
        .map(|column| column.label.as_str())
        .collect();
    // Payment and place are left out of a narrow row; the customer is the line
    // under each order's number.
    assert_eq!(secondary, ["Payment", "Ships to", "Customer"]);
    let roles: Vec<(&str, Option<n::ColumnRole>)> = table
        .columns
        .iter()
        .filter(|column| column.role.is_some())
        .map(|column| (column.label.as_str(), column.role))
        .collect();
    assert_eq!(
        roles,
        [
            ("Order", Some(n::ColumnRole::Title)),
            ("Status", Some(n::ColumnRole::Status)),
            ("Total", Some(n::ColumnRole::Trailing)),
            ("Customer", Some(n::ColumnRole::Subtitle)),
        ]
    );
    // The form that asks for other orders.
    let query = table.query.as_ref().expect("a query");
    assert_eq!(query.action, "/orders");
    let names: Vec<&str> = query
        .children
        .iter()
        .filter_map(|node| match node {
            N::Field(field) => Some(field.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        [
            "search",
            "filter[status][in]",
            "filter[fulfilment][in]",
            "filter[payment][in]",
            "filter[state][in]",
            "filter[placed_on][gte]",
            "filter[placed_on][lte]",
            "limit",
        ]
    );
    let Some(N::Field(search)) = query.children.first() else {
        panic!("no search");
    };
    assert!(matches!(&search.input, Input::Text(text) if text.kind == TextKind::Search));
    let Some(N::Field(status)) = query.children.get(1) else {
        panic!("no status filter");
    };
    let Input::Choices(statuses) = &status.input else {
        panic!("not choices");
    };
    // Each status says how many orders hold it.
    assert!(
        statuses
            .options
            .iter()
            .all(|option| option.detail.is_some())
    );
    // A tap on a row opens its order in the drawer, as its number does.
    assert!(table.rows.iter().all(|row| matches!(
        &row.action,
        Some(Action::Request { method: n::Method::Get, href, .. })
            if Some(href.as_str()) == row.id.as_deref().map(|id| format!("/orders/{id}/peek")).as_deref()
    )));
    // The next orders are at a cursor.
    assert!(
        table
            .more
            .as_deref()
            .is_some_and(|more| more.contains("cursor="))
    );
    // A row's order number opens the order in the drawer: a form of one button.
    let first = &table.rows[0];
    let opens = all(&first.cells[0])
        .into_iter()
        .find_map(|node| match node {
            N::Form(form) => Some(form),
            _ => None,
        });
    let form = opens.expect("the form that opens the drawer");
    assert_eq!(form.method, n::Method::Get);
    assert_eq!(
        form.action,
        format!("/orders/{}/peek", first.id.as_deref().unwrap_or_default())
    );
    assert!(matches!(screen.body.find("order"), Some(N::Sheet(_))));

    // The drawer's content comes as ops: the sheet morphed, then opened.
    let peek = app
        .client()
        .get(&form.action)
        .header("x-navree-native", "1")
        .header("x-navree-client", PHONE)
        .send()
        .await;
    let ops = envelope(&peek.text()).ops;
    let [Op::Morph { target, node }, Op::Open { target: opened }] = ops.as_slice() else {
        panic!("not a morph and an open: {ops:?}");
    };
    assert_eq!((target.as_str(), opened.as_str()), ("order", "order"));
    let N::Sheet(sheet) = node else {
        panic!("not a sheet");
    };
    assert!(
        sheet
            .title
            .as_deref()
            .is_some_and(|title| title.starts_with("Order CS-"))
    );
    // Its slower parts fetch themselves once it is open.
    let lazy: Vec<&str> = all(node)
        .into_iter()
        .filter_map(|node| match node {
            N::Region(region) if region.lazy => Some(region.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(lazy, ["order-steps", "order-others"]);
}

#[tokio::test]
async fn an_action_that_cannot_be_taken_says_why_in_the_words_the_page_says_it_in() {
    let (app, _dir) = boot().await;
    // Among the first orders there are some that shipped or were cancelled.
    let mut held = None;
    for id in 1..=30 {
        let asked = ask(&app, &format!("/orders/{id}")).await;
        let screen = envelope(&asked.native).screen.expect("a screen");
        let off = screen.actions.iter().find_map(|node| match node {
            N::Button(button) if button.disabled => Some(button.clone()),
            _ => None,
        });
        if off.is_some() {
            held = off;
            break;
        }
    }
    let held = held.expect("an order with an action that is held back");
    let reason = held.reason.as_deref().expect("a reason");
    assert!(reason.contains("CS-"), "{reason}");
}

#[tokio::test]
async fn an_order_is_a_record_written_for_native_clients_and_an_action_answers_parts_written_the_same_way()
 {
    let (app, _dir) = boot().await;
    let asked = ask(&app, "/orders/1").await;
    let screen = envelope(&asked.native).screen.expect("a screen");
    assert!(screen.title.starts_with("Order CS-"));
    assert_eq!(screen.id.as_deref(), Some("order-page"));
    // Where it is in the app: under the orders, with the way back to them.
    assert_eq!(screen.nav.as_deref(), Some("/orders"));
    assert_eq!(
        screen.back.as_ref().map(|back| back.href.as_str()),
        Some("/orders")
    );
    assert_eq!(screen.trail.len(), 2);
    let nodes = all(&screen.body);
    // The three actions are the screen's own, each with its role and a name an answer
    // can find it by.
    let actions: Vec<&n::Button> = screen
        .actions
        .iter()
        .filter_map(|node| match node {
            N::Button(button) => Some(button),
            _ => None,
        })
        .collect();
    let roles: Vec<(&str, n::Role, Option<&str>)> = actions
        .iter()
        .map(|button| (button.label.as_str(), button.role, button.id.as_deref()))
        .collect();
    assert_eq!(
        roles,
        [
            (
                "Resend confirmation",
                n::Role::Secondary,
                Some("order-resend")
            ),
            ("Refund", n::Role::Secondary, Some("order-refund")),
            ("Cancel order", n::Role::Destructive, Some("order-cancel")),
        ]
    );
    // Cancelling asks first, in the system's own confirmation, and posts to the handler
    // the page's form posts to.
    let cancel = actions[2];
    let ask_first = cancel.ask.as_ref().expect("a question");
    assert!(
        ask_first.title.starts_with("Cancel CS-"),
        "{}",
        ask_first.title
    );
    assert_eq!(ask_first.confirm, "Cancel order");
    assert!(matches!(
        &cancel.action,
        Some(Action::Request { method: n::Method::Post, href, .. }) if href == "/orders/1/cancel"
    ));
    // A refund opens the dialog, whose form is sent to the page's own handler.
    assert!(matches!(
        &actions[1].action,
        Some(Action::Commands { list }) if matches!(list.as_slice(), [Command::Open { id }] if id == "refund")
    ));
    let refund = nodes
        .iter()
        .find_map(|node| match node {
            N::Dialog(dialog) if dialog.id == "refund" => Some(dialog),
            _ => None,
        })
        .expect("the refund dialog");
    let [N::Form(form)] = refund.children.as_slice() else {
        panic!("not one form: {:?}", refund.children);
    };
    assert_eq!(
        (form.id.as_deref(), form.method, form.action.as_str()),
        (Some("refund"), n::Method::Post, "/orders/1/refund")
    );
    let fields: Vec<&str> = form
        .children
        .iter()
        .filter_map(|node| match node {
            N::Field(field) => Some(field.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(fields, ["amount", "reason"]);
    let [N::Button(keep), N::Button(go)] = refund.actions.as_slice() else {
        panic!("not two answers: {:?}", refund.actions);
    };
    assert_eq!((keep.role, go.role), (n::Role::Cancel, n::Role::Confirm));
    // Its state is a card with the total set large, once on the screen.
    assert!(
        nodes
            .iter()
            .any(|node| matches!(node, N::Card(card) if card.id.as_deref() == Some("order-state")))
    );
    let large: Vec<&n::Text> = nodes
        .iter()
        .filter_map(|node| match node {
            N::Text(text) if text.style == n::TextStyle::Display => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(large.len(), 1);
    assert!(large[0].value.starts_with('₹'), "{}", large[0].value);
    // The lines are rows of the system's own list, each with how many at what price and
    // its total; the sums stand under them.
    let lines: Vec<&n::Row> = nodes
        .iter()
        .filter_map(|node| match node {
            N::Row(row)
                if row
                    .subtitle
                    .as_deref()
                    .is_some_and(|line| line.contains(" × ")) =>
            {
                Some(row)
            }
            _ => None,
        })
        .collect();
    assert!(!lines.is_empty());
    assert!(
        lines
            .iter()
            .all(|row| row.trailing.is_some() && row.leading.is_some())
    );
    assert!(nodes.iter().any(|node| matches!(
        node,
        N::Facts(facts) if facts.items.iter().any(|fact| fact.label == "Total")
    )));
    // How far it has got is a timeline, its first moment the order being placed.
    assert!(nodes.iter().any(|node| matches!(
        node,
        N::Timeline(timeline) if timeline.items.first().is_some_and(|moment| moment.title == "Placed")
    )));
    // Nothing of it is read from the page: no table, and nothing left to a web view.
    assert!(
        !nodes
            .iter()
            .any(|node| matches!(node, N::Table(_) | N::Web(_)))
    );
    // A browser gets the page it always got.
    assert!(asked.html.contains(r#"data-slot="sidebar""#));
    assert!(asked.html.contains(r#"id="order-page""#));

    // Resending the confirmation, as the action would after its question.
    let resent = app
        .client()
        .post("/orders/1/resend")
        .header("x-navree-native", "1")
        .header("x-navree-client", PHONE)
        .header("content-type", "application/x-www-form-urlencoded")
        .send()
        .await;
    assert_eq!(
        resent.header("content-type"),
        Some("application/vnd.navree.native+json")
    );
    let ops = envelope(&resent.text()).ops;
    let names: Vec<&str> = ops
        .iter()
        .map(|op| match op {
            Op::Close { .. } => "close",
            Op::Morph { .. } => "morph",
            Op::Replace { .. } => "replace",
            Op::Toast(_) => "toast",
            _ => "other",
        })
        .collect();
    // The screen's content is drawn again, then each action as the order allows it now
    // and the refund's form from what is left, and a toast says what happened.
    assert_eq!(
        names,
        [
            "close", "morph", "replace", "replace", "replace", "replace", "toast"
        ]
    );
    assert!(matches!(
        ops.get(5),
        Some(Op::Replace { target, node: N::Dialog(_) }) if target == "refund"
    ));
    let Some(Op::Morph { target, node }) = ops.get(1) else {
        panic!("no morph of the content");
    };
    assert_eq!(target, "order-page");
    assert_eq!(node.id(), Some("order-page"));
    // What comes back is written with the same parts: the state is its card again.
    assert!(
        all(node)
            .iter()
            .any(|node| matches!(node, N::Card(card) if card.id.as_deref() == Some("order-state")))
    );
    let replaced: Vec<&str> = ops
        .iter()
        .filter_map(|op| match op {
            Op::Replace {
                target,
                node: N::Button(_),
            } => Some(target.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(replaced, ["order-resend", "order-refund", "order-cancel"]);
    // It was just sent: sending it again is held back, and says why.
    let Some(Op::Replace {
        node: N::Button(again),
        ..
    }) = ops.get(2)
    else {
        panic!("no resend button");
    };
    assert!(again.disabled && again.reason.is_some());
    // The same action from a browser's runtime is ops with markup, as before.
    let web = app
        .client()
        .post("/orders/2/resend")
        .header("x-navree-ui", "1")
        .header("content-type", "application/x-www-form-urlencoded")
        .send()
        .await;
    assert_eq!(
        web.header("content-type"),
        Some("application/vnd.navree.ops+json")
    );
    assert!(
        web.text()
            .contains(r#""op":"morph","target":"order","html":"<dialog"#)
    );

    // A refund the handler refuses says why at the field of the screen's own form.
    let refused = app
        .client()
        .post("/orders/1/refund")
        .header("x-navree-native", "1")
        .header("x-navree-client", PHONE)
        .header("content-type", "application/x-www-form-urlencoded")
        .body("amount=0&reason=late")
        .send()
        .await;
    assert_eq!(refused.status().as_u16(), 422);
    let ops = envelope(&refused.text()).ops;
    assert!(
        matches!(ops.as_slice(), [Op::Errors { form, fields }] if form == "refund" && fields.contains_key("amount")),
        "{ops:?}"
    );
}

#[tokio::test]
async fn the_day_is_a_screen_written_for_native_clients_from_what_the_dashboard_is_drawn_from() {
    let (app, _dir) = boot().await;
    let asked = ask(&app, "/").await;
    assert_eq!(asked.status, 200);
    assert_eq!(asked.location, None);
    let mut sent = envelope(&asked.native);
    // Each icon the screen names comes with its shapes, said once in the answer's table.
    assert!(
        sent.icons.contains_key("shopping-bag"),
        "{:?}",
        sent.icons.keys()
    );
    // The screen stands in the app's shell: a client knows by the stamp its own is current.
    assert!(!sent.shell.is_empty());
    sent.inline_icons();
    let screen = sent.screen.expect("a screen");
    assert_eq!(screen.title, "Today");
    assert_eq!(screen.id.as_deref(), Some("today"));
    assert_eq!(screen.nav.as_deref(), Some("/"));
    assert!(screen.refresh);
    assert!(matches!(
        screen.actions.as_slice(),
        [N::Button(orders)] if orders.role == n::Role::Secondary && orders.icon.as_ref().is_some_and(|icon| !icon.paths.is_empty())
    ));
    let nodes = all(&screen.body);
    // The one figure the day is about is set large, and counts when it changes.
    let large: Vec<&n::Text> = nodes
        .iter()
        .filter_map(|node| match node {
            N::Text(text) if text.style == n::TextStyle::Display => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(large.len(), 1);
    assert_eq!(large[0].id.as_deref(), Some("revenue-today"));
    assert!(large[0].value.starts_with('₹'));
    assert_eq!(large[0].change, Some(n::Change::Count));
    // The figures and the charts follow the shop live: each is a region a client
    // fetches again when the stream speaks.
    for (id, src) in [
        ("figures", "/fragments/figures"),
        ("sales", "/fragments/sales"),
    ] {
        let Some(N::Region(region)) = screen.body.find(id) else {
            panic!("no region {id}");
        };
        assert_eq!(region.src, src);
        let live = region.live.as_ref().expect("a stream");
        assert!(live.src.starts_with("/broadcasting/stream"));
        assert_eq!(live.event.as_deref(), Some("stats"));
    }
    // The day's other figures are figures, as many beside each other as fit, each
    // leading to where it is about.
    let Some(N::Grid(tiles)) = screen.body.find("day-figures") else {
        panic!("no grid of figures");
    };
    assert_eq!(tiles.min, Some(150));
    assert_eq!(tiles.children.len(), 3);
    assert!(tiles.children.iter().all(|tile| matches!(
        tile,
        N::Figure(figure) if !figure.value.is_empty() && figure.action.is_some() && figure.icon.is_some()
    )));
    // What needs a hand is the system's own section.
    assert!(matches!(
        screen.body.find("attention"),
        Some(N::Section(section)) if section.style == n::SectionStyle::Inset
    ));
    // The charts are a run that scrolls across, each a card holding the numbers it draws.
    let Some(N::Carousel(charts)) = screen.body.find("sales-charts") else {
        panic!("no run of charts");
    };
    assert_eq!((charts.min, charts.children.len()), (Some(300), 3));
    let drawn: Vec<&n::Chart> = charts
        .children
        .iter()
        .flat_map(|card| all(card))
        .filter_map(|node| match node {
            N::Chart(chart) => Some(chart),
            _ => None,
        })
        .collect();
    assert_eq!(drawn.len(), 3);
    for chart in &drawn {
        assert!(!chart.labels.is_empty(), "{:?}", chart.title);
        for series in &chart.series {
            assert_eq!(series.values.len(), chart.labels.len());
        }
    }
    assert!(
        drawn[0].series[0]
            .shown
            .iter()
            .all(|words| words.starts_with('₹'))
    );
    // What happened last is a list that asks only for what came since its newest row.
    let Some(N::List(feed)) = screen.body.find("activity") else {
        panic!("no list of events");
    };
    assert!(feed.since.is_some());
    assert_eq!(feed.changes.as_deref(), Some("/fragments/activity"));
    assert_eq!(
        feed.live.as_ref().and_then(|live| live.event.as_deref()),
        Some("activity")
    );
    assert!(!feed.children.is_empty());
    assert!(
        feed.children
            .iter()
            .all(|row| matches!(row, N::FeedRow(row) if row.id.is_some()))
    );
    // A browser gets the dashboard it always got.
    assert!(asked.html.contains(r#"data-slot="chart""#));
    assert!(asked.html.contains(r#"data-slot="sidebar""#));
    assert!(asked.html.contains("Dashboard"));

    // What a region fetches is the part written for it: the figures, and the charts.
    let figures = envelope(&ask(&app, "/fragments/figures").await.native);
    let figures = figures.screen.expect("the figures");
    assert_eq!(figures.body.id(), Some("figures-parts"));
    assert!(
        all(&figures.body)
            .iter()
            .any(|node| matches!(node, N::Figure(_)))
    );
    let charts = envelope(&ask(&app, "/fragments/sales").await.native);
    assert!(matches!(
        charts.screen.expect("the charts").body,
        N::Carousel(_)
    ));
    // The new rows of the list are its own answer: events to merge in at the top, as
    // rows written the way the screen's are, with the cursor of the newest.
    let rows = ask(&app, "/fragments/activity?since=0-0").await;
    let rows = envelope(&rows.native);
    let [
        Op::Merge {
            target,
            nodes,
            at,
            more,
            since,
            ..
        },
    ] = rows.ops.as_slice()
    else {
        panic!("not a merge: {:?}", rows.ops);
    };
    assert_eq!(target, "activity");
    assert!(!nodes.is_empty());
    assert!(nodes.iter().all(|node| matches!(node, N::FeedRow(_))));
    assert_ne!(since, "0-0");
    assert_eq!(
        (*at, *more),
        (navree::ui::native::protocol::op::Edge::Start, false)
    );
    // The web view inside a native client still gets the page, without the chrome.
    let embedded = app
        .client()
        .get("/")
        .header("x-navree-embed", "1")
        .send()
        .await
        .text();
    assert!(embedded.contains(r#"data-slot="chart""#));
    assert!(!embedded.contains(r#"data-slot="sidebar""#));
    assert!(embedded.len() < asked.html.len());
    // The shell the client draws in the chrome's place.
    let shell = app
        .client()
        .get("/_navree/native/shell")
        .send()
        .await
        .text();
    let shell: navree::ui::native::protocol::Shell = serde_json::from_str(&shell).expect("a shell");
    assert_eq!(shell.name, "Charkha Supply Co.");
    let items: Vec<&str> = shell.nav.groups[0]
        .items
        .iter()
        .map(|item| item.href.as_str())
        .collect();
    assert_eq!(items, ["/", "/orders", "/inventory"]);
}

#[tokio::test]
async fn the_shelves_are_composed_for_native_clients_and_a_tile_opens_restocks_and_is_drawn_again()
{
    let (app, _dir) = boot().await;
    let asked = ask(&app, "/shelves").await;
    // A browser gets the two cards the dashboard shows, on a page of their own.
    assert!(asked.html.contains("Running low"));
    assert!(asked.html.contains("Top sellers today"));
    let screen = envelope(&asked.native).screen.expect("a screen");
    assert_eq!(screen.title, "Shelves");
    assert_eq!(screen.nav.as_deref(), Some("/inventory"));
    assert_eq!(
        screen.back.as_ref().map(|back| back.href.as_str()),
        Some("/")
    );
    // How many shelves hold each level: three cards, each tinted with its level's hue
    // beside the words that say it.
    let Some(N::Grid(levels)) = screen.body.find("levels") else {
        panic!("no levels");
    };
    let hues: Vec<Option<n::Hue>> = levels
        .children
        .iter()
        .map(|card| match card {
            N::Card(card) => card.colour.as_ref().map(|colour| colour.hue),
            _ => None,
        })
        .collect();
    assert_eq!(
        hues,
        [
            Some(n::Hue::Danger),
            Some(n::Hue::Warning),
            Some(n::Hue::Success)
        ]
    );
    // The emptiest shelves are a wall of tiles: as many beside each other as fit.
    let Some(N::Grid(wall)) = screen.body.find("wall") else {
        panic!("no wall: the seed has shelves running low");
    };
    assert_eq!(wall.min, Some(160));
    assert!(!wall.children.is_empty() && wall.children.len() <= 12);
    let tiles: Vec<&n::Card> = wall
        .children
        .iter()
        .map(|tile| match tile {
            N::Card(card) => card,
            other => panic!("not a card: {other:?}"),
        })
        .collect();
    for tile in &tiles {
        let id = tile
            .id
            .as_deref()
            .expect("a tile has a name an answer finds it by");
        let product = id.strip_prefix("shelf-").expect("named after its product");
        // One control with a name: a tap opens the product where the reader is, and the
        // tile grows into it.
        assert!(tile.label.as_deref().is_some_and(|label| !label.is_empty()));
        assert!(matches!(
            &tile.action,
            Some(Action::Request { method: n::Method::Get, href, .. }) if *href == format!("/inventory/{product}/peek")
        ));
        assert_eq!(tile.transition, Some(n::Transition::Zoom));
        // A long press offers the restock, which posts where the page's form posts.
        assert!(matches!(
            tile.menu.as_slice(),
            [N::Button(restock)] if matches!(
                &restock.action,
                Some(Action::Request { method: n::Method::Post, href, fields })
                    if *href == format!("/inventory/{product}/restock")
                        && fields.iter().any(|pair| pair.name == "quantity" && pair.value == "24")
            )
        ));
        // Its level is said in words beside the colour, and by the dot at its mark's corner.
        let inside = tile.children.iter().flat_map(all).collect::<Vec<_>>();
        assert!(
            inside
                .iter()
                .any(|node| matches!(node, N::Badge(badge) if !badge.text.is_empty()))
        );
        assert!(inside.iter().any(|node| matches!(
            node,
            N::Layers(layers) if layers.place == n::Place::BottomEnd
                && matches!(layers.children.last(), Some(N::Shape(dot)) if dot.label.is_some())
        )));
    }
    // What sold today scrolls across on a phone and is a list beside the wall in a wide room.
    let adaptive = all(&screen.body)
        .into_iter()
        .find_map(|node| match node {
            N::Adaptive(adaptive) => Some(adaptive),
            _ => None,
        })
        .expect("the sellers");
    assert!(
        adaptive
            .wide
            .iter()
            .any(|node| matches!(node, N::Section(_)))
    );
    assert!(!adaptive.compact.is_empty());
    // The sheet a tile's product is opened in is part of the screen.
    assert!(matches!(
        screen.body.find(store_demo::views::inventory::SHEET),
        Some(N::Sheet(_))
    ));

    // A tap on a tile: the product in the sheet, opened.
    let first = tiles[0]
        .id
        .as_deref()
        .unwrap()
        .trim_start_matches("shelf-")
        .to_owned();
    let peek = app
        .client()
        .get(&format!("/inventory/{first}/peek"))
        .header("x-navree-native", "1")
        .header("x-navree-client", PHONE)
        .send()
        .await;
    let ops = envelope(&peek.text()).ops;
    assert!(
        matches!(
            ops.as_slice(),
            [Op::Morph { target, .. }, Op::Open { target: opened }]
                if target == store_demo::views::inventory::SHEET && opened == store_demo::views::inventory::SHEET
        ),
        "{ops:?}"
    );
    // The restock of its menu: the tile is drawn again by the function that drew it,
    // with the stock it has now.
    let before = tiles[0].clone();
    let restocked = app
        .client()
        .post(&format!("/inventory/{first}/restock"))
        .header("x-navree-native", "1")
        .header("x-navree-client", PHONE)
        .header("content-type", "application/x-www-form-urlencoded")
        .body("quantity=24")
        .send()
        .await;
    assert_eq!(restocked.status().as_u16(), 200);
    let ops = envelope(&restocked.text()).ops;
    let again = ops
        .iter()
        .find_map(|op| match op {
            Op::Replace {
                target,
                node: N::Card(card),
            } if *target == format!("shelf-{first}") => Some(card),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the tile is not drawn again: {ops:?}"));
    assert_eq!(again.id, before.id);
    assert_ne!(
        again.children, before.children,
        "the tile says the stock it has now"
    );
    assert!(
        ops.iter()
            .any(|op| matches!(op, Op::Toast(toast) if toast.text.contains("24 units")))
    );
}

#[tokio::test]
async fn the_inventory_opens_a_product_with_its_restock_form_where_the_reader_is() {
    let (app, _dir) = boot().await;
    let asked = ask(&app, "/inventory").await;
    let screen = envelope(&asked.native).screen.expect("a screen");
    let Some(N::Region(region)) = screen.body.find("products") else {
        panic!("no table");
    };
    let [N::Table(table)] = region.children.as_slice() else {
        panic!("not one table");
    };
    let roles: Vec<(&str, Option<n::ColumnRole>)> = table
        .columns
        .iter()
        .filter(|column| column.role.is_some())
        .map(|column| (column.label.as_str(), column.role))
        .collect();
    assert_eq!(
        roles,
        [
            ("Product", Some(n::ColumnRole::Title)),
            ("In stock", Some(n::ColumnRole::Trailing)),
            ("Level", Some(n::ColumnRole::Status)),
            ("SKU", Some(n::ColumnRole::Subtitle)),
        ]
    );
    // A tap on a row opens the product; a browser's row is as it was.
    let first = &table.rows[0];
    let id = first.id.as_deref().expect("a key");
    assert_eq!(
        first.action,
        Some(Action::Request {
            method: n::Method::Get,
            href: format!("/inventory/{id}/peek"),
            fields: Vec::new(),
        })
    );
    assert!(matches!(screen.body.find("product"), Some(N::Sheet(_))));
    assert!(
        !asked.html.contains("/peek\""),
        "a browser's table has no peek"
    );

    // The sheet comes as ops: the sheet morphed, then opened.
    let peek = app
        .client()
        .get(&format!("/inventory/{id}/peek"))
        .header("x-navree-native", "1")
        .header("x-navree-client", PHONE)
        .send()
        .await;
    let ops = envelope(&peek.text()).ops;
    let [Op::Morph { target, node }, Op::Open { target: opened }] = ops.as_slice() else {
        panic!("not a morph and an open: {ops:?}");
    };
    assert_eq!((target.as_str(), opened.as_str()), ("product", "product"));
    let N::Sheet(sheet) = node else {
        panic!("not a sheet");
    };
    assert!(
        sheet
            .title
            .as_deref()
            .is_some_and(|title| !title.is_empty())
    );
    let fields: Vec<&str> = all(node)
        .into_iter()
        .filter_map(|node| match node {
            N::Field(field) => Some(field.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(fields, ["quantity"]);

    // Restocking from the sheet closes it; from a browser's row nothing closes.
    let restocked = app
        .client()
        .post(&format!("/inventory/{id}/restock"))
        .header("x-navree-native", "1")
        .header("x-navree-client", PHONE)
        .header("content-type", "application/x-www-form-urlencoded")
        .body("quantity=5")
        .send()
        .await;
    let ops = envelope(&restocked.text()).ops;
    assert!(
        ops.iter()
            .any(|op| matches!(op, Op::Close { target } if target == "product")),
        "{ops:?}"
    );
    // The record that changed is drawn again, with the stock it has now, besides the toast.
    assert!(
        ops.iter()
            .any(|op| matches!(op, Op::Invalidate { target } if target == "products")),
        "{ops:?}"
    );
    assert!(ops.iter().any(|op| matches!(op, Op::Toast(_))), "{ops:?}");
    let web = app
        .client()
        .post(&format!("/inventory/{id}/restock"))
        .header("x-navree-ui", "1")
        .header("content-type", "application/x-www-form-urlencoded")
        .body("quantity=5")
        .send()
        .await;
    assert!(!web.text().contains(r#""target":"product""#));
    assert!(!web.text().contains(r#""op":"invalidate""#));
}
