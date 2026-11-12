//! The store as a native client gets it: which of its routes answer a screen,
//! which fall back to the web view and why, what the actions answer, and that a
//! browser asking the same routes gets HTML as before.
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
async fn an_order_is_a_screen_with_its_dialogs_and_an_action_answers_ops() {
    let (app, _dir) = boot().await;
    let asked = ask(&app, "/orders/1").await;
    let screen = envelope(&asked.native).screen.expect("a screen");
    assert!(screen.title.starts_with("Order CS-"));
    assert_eq!(screen.id.as_deref(), Some("order-page"));
    assert_eq!(screen.trail.len(), 2);
    let nodes = all(&screen.body);
    // The three actions open their dialogs by command.
    let opens: Vec<&str> = nodes
        .iter()
        .filter_map(|node| match node {
            N::Button(button) => match &button.action {
                Some(Action::Commands { list }) => match list.as_slice() {
                    [Command::Open { id }] => Some(id.as_str()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(opens, ["resend", "refund", "cancel"]);
    // Cancelling asks first, in an alert whose answers are buttons by role.
    let cancel = nodes
        .iter()
        .find_map(|node| match node {
            N::Dialog(dialog) if dialog.id == "cancel" => Some(dialog),
            _ => None,
        })
        .expect("the cancel dialog");
    assert!(cancel.alert);
    let [N::Button(keep), N::Button(go)] = cancel.actions.as_slice() else {
        panic!("not two answers: {:?}", cancel.actions);
    };
    assert_eq!(keep.role, n::Role::Cancel);
    assert_eq!(keep.action, Some(Action::Dismiss));
    assert_eq!(go.role, n::Role::Destructive);
    assert!(matches!(
        &go.action,
        Some(Action::Request { method: n::Method::Post, href, .. }) if href == "/orders/1/cancel"
    ));
    // The refund dialog holds a form of two fields.
    let refund = nodes
        .iter()
        .find_map(|node| match node {
            N::Dialog(dialog) if dialog.id == "refund" => Some(dialog),
            _ => None,
        })
        .expect("the refund dialog");
    let fields: Vec<&str> = refund
        .children
        .iter()
        .flat_map(all)
        .filter_map(|node| match node {
            N::Field(field) => Some(field.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(fields, ["amount", "reason"]);
    // The facts, the lines and the history are there. The lines are a list of
    // records, each with its picture, how many at what price, and its total, not a
    // table of four columns.
    assert!(
        nodes
            .iter()
            .any(|node| matches!(node, N::Facts(facts) if facts.items.len() == 4))
    );
    assert!(!nodes.iter().any(|node| matches!(node, N::Table(_))));
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
    assert!(lines.iter().all(|row| row.trailing.is_some()));
    // The history is a timeline, its first moment the order being placed.
    assert!(nodes.iter().any(|node| matches!(
        node,
        N::Timeline(timeline) if timeline.items.iter().any(|moment| moment.title == "Placed")
    )));

    // Resending the confirmation, as the dialog's button would.
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
    // The dialog closes, the drawer and the page are drawn again, a toast says what happened.
    assert_eq!(names[..3], ["close", "morph", "morph"], "{names:?}");
    assert_eq!(names.last(), Some(&"toast"), "{names:?}");
    let Some(Op::Morph { target, node }) = ops.get(2) else {
        panic!("no morph of the page");
    };
    assert_eq!(target, "order-page");
    assert_eq!(node.id(), Some("order-page"));
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

    // A refund the form refuses says why at its field.
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
async fn the_dashboard_gives_its_charts_as_numbers_and_what_it_cannot_say_falls_back_with_why() {
    let (app, _dir) = boot().await;
    let asked = ask(&app, "/").await;
    assert_eq!(asked.status, 200);
    assert_eq!(asked.location, None);
    let screen = envelope(&asked.native).screen.expect("a screen");
    assert_eq!(screen.title, "Dashboard");
    // Each chart is the numbers it draws, as numbers, with the words the page shows
    // them in beside them.
    let charts: Vec<&n::Chart> = all(&screen.body)
        .into_iter()
        .filter_map(|node| match node {
            N::Chart(chart) => Some(chart),
            _ => None,
        })
        .collect();
    assert!(!charts.is_empty());
    // The dashboard's key figures are figures, each with its value in the page's words.
    let figures: Vec<&n::Figure> = all(&screen.body)
        .into_iter()
        .filter_map(|node| match node {
            N::Figure(figure) => Some(figure),
            _ => None,
        })
        .collect();
    assert!(figures.len() >= 3, "{figures:?}");
    assert!(
        figures
            .iter()
            .all(|figure| !figure.label.is_empty() && !figure.value.is_empty())
    );
    for chart in &charts {
        assert!(!chart.labels.is_empty(), "{:?}", chart.title);
        for series in &chart.series {
            assert_eq!(series.values.len(), chart.labels.len());
            assert!(
                series
                    .values
                    .iter()
                    .flatten()
                    .all(|value| value.as_f64().is_some())
            );
        }
    }
    let sales = charts
        .iter()
        .find(|chart| {
            chart
                .series
                .iter()
                .any(|series| series.shown.iter().any(|words| words.contains('₹')))
        })
        .expect("a chart of rupees");
    assert!(sales.series[0].values.iter().flatten().count() > 0);
    // The live feed's new rows are the feed's own answer: a client gets them to merge into
    // its list at the top, with the cursor of the newest as the revision it is at.
    let rows = ask(&app, "/fragments/activity?since=0-0").await;
    let rows = envelope(&rows.native);
    let [
        Op::Merge {
            nodes, at, more, ..
        },
    ] = rows.ops.as_slice()
    else {
        panic!("not a merge: {:?}", rows.ops);
    };
    assert!(!nodes.is_empty());
    assert!(nodes.iter().all(|node| matches!(node, N::FeedRow(_))));
    assert_eq!(
        (*at, *more),
        (navree::ui::native::protocol::op::Edge::Start, false)
    );
    let embedded = app
        .client()
        .get("/")
        .header("x-navree-embed", "1")
        .send()
        .await
        .text();
    assert!(embedded.contains(r#"data-slot="chart""#));
    assert!(!embedded.contains(r#"data-slot="sidebar""#));
    assert!(asked.html.contains(r#"data-slot="sidebar""#));
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
    let web = app
        .client()
        .post(&format!("/inventory/{id}/restock"))
        .header("x-navree-ui", "1")
        .header("content-type", "application/x-www-form-urlencoded")
        .body("quantity=5")
        .send()
        .await;
    assert!(!web.text().contains(r#""target":"product""#));
}
