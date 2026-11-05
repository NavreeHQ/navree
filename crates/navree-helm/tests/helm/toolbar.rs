//! The list page's toolbar (chip filters, the bulk-action menu, Export), the
//! sidebar's counts and the delta on a metric.

use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::Duration;
use navree_db::Db;
use navree_helm::{Field, ModuleBuilderHelmExt, Resource};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::{Builder, Model};
use navree_testing::{TestApp, TestClient, TestResponse};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::*;

async fn page(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

fn plain(response: TestResponse) -> String {
    response.text().replace("&#x2f;", "/")
}

/// The lines of a CSV body, each as its fields (no quotes in the test data
/// but the ones that are asked about).
fn lines(csv: &str) -> Vec<String> {
    csv.split("\r\n")
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

async fn csv(client: &TestClient, query: &str) -> TestResponse {
    client
        .get(&format!("/admin/resources/deals/export.csv{query}"))
        .header("accept", "text/html")
        .send()
        .await
}

/// `count` deals titled `Bulk 0000` upward, in one statement.
async fn many(app: &TestApp, count: usize, owner: Option<Uuid>) {
    let db = app.get::<Db>().await;
    let deals: Vec<Deal> = (0..count)
        .map(|i| Deal {
            id: Uuid::now_v7(),
            title: format!("Bulk {i:04}"),
            amount_cents: i as i64,
            currency: "USD".to_owned(),
            stage: "lead".to_owned(),
            owner_id: owner,
            created_at: now() - Duration::seconds((count - i) as i64),
            updated_at: now(),
            ..Deal::default()
        })
        .collect();
    for chunk in deals.chunks(200) {
        Deal::insert(&*db, chunk.to_vec()).await.unwrap();
    }
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

#[tokio::test]
async fn export_is_the_list_as_csv_for_its_filters() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    let acme = company(&app, "Acme, Inc.", None).await;
    for (title, cents, stage) in [
        ("Alpha", 300, "lead"),
        ("Beta", 100, "won"),
        ("Gamma \"quoted\"", 200, "won"),
    ] {
        deal(
            &app,
            DealSeed {
                title,
                amount_cents: cents,
                stage,
                company: Some(acme.id),
                ..DealSeed::default()
            },
        )
        .await;
    }
    let response = csv(&manager, "?sort=amount_cents").await.assert_status(200);
    assert_eq!(
        response.header("content-type"),
        Some("text/csv; charset=utf-8")
    );
    assert_eq!(
        response.header("content-disposition"),
        Some("attachment; filename=\"deals-2026-10-26.csv\"")
    );
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
    let body = response.text();
    assert!(body.starts_with('\u{feff}'), "{body}");
    let lines = lines(body.trim_start_matches('\u{feff}'));
    assert_eq!(
        lines[0],
        "Title,Amount,Currency,Stage,Company,Owner,Expected close date,Notes,Priority,Probability,Lost reason,Closed at,Created at"
    );
    // The list's order, the display of each value, quotes where they belong.
    assert_eq!(lines.len(), 4, "{body}");
    assert!(
        lines[1].starts_with("Beta,$1.00,US dollar,Won,\"Acme, Inc.\","),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("\"Gamma \"\"quoted\"\"\",$2.00,"),
        "{}",
        lines[2]
    );
    assert!(lines[3].starts_with("Alpha,$3.00,"), "{}", lines[3]);
    // Markdown is the text typed, not its HTML; times are ISO 8601.
    assert!(lines[3].contains("**Secret** notes"), "{}", lines[3]);
    assert!(lines[3].contains("2026-10-26T12:00:00Z"), "{}", lines[3]);

    // Only what the filters and the search show.
    let body = csv(&manager, "?filter%5Bstage%5D=won&sort=-amount_cents")
        .await
        .assert_status(200)
        .text();
    let titles: Vec<&str> = body
        .lines()
        .skip(1)
        .map(|l| l.split(',').next().unwrap())
        .collect();
    assert_eq!(titles, ["\"Gamma \"\"quoted\"\"\"", "Beta"], "{body}");
    let body = csv(&manager, "?search=alph")
        .await
        .assert_status(200)
        .text();
    assert_eq!(lines_of(&body), 2, "{body}");
    assert!(body.contains("Alpha"));
    // A filter the list would refuse is refused, as a page.
    csv(&manager, "?filter%5Bnope%5D=1")
        .await
        .assert_status(400);
    csv(&manager, "?sort=nothing").await.assert_status(400);
}

fn lines_of(csv: &str) -> usize {
    lines(csv).len()
}

#[tokio::test]
async fn export_holds_to_what_the_user_may_read() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    let (own_id, own_only) = user(&app, "own-only").await;
    let (_, no_secrets) = user(&app, "no-secrets").await;
    deal(
        &app,
        DealSeed {
            title: "Mine",
            owner: Some(own_id),
            ..DealSeed::default()
        },
    )
    .await;
    deal(
        &app,
        DealSeed {
            title: "Someone else's",
            owner: Some(rep_id),
            ..DealSeed::default()
        },
    )
    .await;
    // Rows: the access filter of the list.
    let body = csv(&own_only, "").await.assert_status(200).text();
    assert!(
        body.contains("Mine") && !body.contains("Someone else's"),
        "{body}"
    );
    let body = csv(&rep, "").await.assert_status(200).text();
    assert!(body.contains("Mine") && body.contains("Someone else's"));
    // Fields: what the rules hide is empty, whatever the rows hold.
    let body = csv(&no_secrets, "").await.assert_status(200).text();
    assert!(body.contains("Mine"), "{body}");
    assert!(!body.contains("Secret"), "{body}");
    // Whole: someone with no panel, and someone with the panel and no deals.
    let stranger = app.acting_as(navree_testing::principal(Uuid::now_v7()));
    csv(&stranger, "").await.assert_status(403);
    let id = Uuid::now_v7();
    give_role(&app, id, "empty", &["helm.view"]).await;
    let empty = app.acting_as(navree_testing::principal(id));
    csv(&empty, "").await.assert_status(403);
    // Not signed in: to the sign-in page.
    csv(&app.client(), "").await.assert_status(303);
}

#[tokio::test]
async fn a_spreadsheet_does_not_run_what_a_deal_is_called() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    for title in ["=1+1", "@SUM(1,2)", "+1 555 0100", "-3"] {
        deal(
            &app,
            DealSeed {
                title,
                ..DealSeed::default()
            },
        )
        .await;
    }
    let body = csv(&manager, "?sort=title").await.assert_status(200).text();
    // (The order is the database's collation of punctuation, which differs.)
    let mut titles: Vec<String> = body
        .lines()
        .skip(1)
        .map(|l| l.split(",$").next().unwrap().to_owned())
        .collect();
    titles.sort();
    // Numbers stay numbers; the rest open as the text they are.
    assert_eq!(
        titles,
        ["\"'@SUM(1,2)\"", "'+1 555 0100", "'=1+1", "-3"],
        "{body}"
    );
}

/// The tests that read `QUERIES` take turns.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Counts how many times the panel builds the base query of its resource: once
/// for each page of a list, each chunk of an export and each count.
static QUERIES: AtomicUsize = AtomicUsize::new(0);

struct Counted;

impl Injectable for Counted {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Counted) })
    }
}

impl Resource for Counted {
    type Model = Deal;

    const LABEL: &'static str = "Counted deals";
    const SINGULAR: &'static str = "Counted deal";
    const SLUG: &'static str = "counted";
    const TITLE: &'static str = "title";

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("title").sortable()]
    }

    fn query(&self, query: Builder<Deal>) -> Builder<Deal> {
        QUERIES.fetch_add(1, Ordering::SeqCst);
        query
    }
}

/// A resource whose count is switched off.
struct Uncounted;

impl Injectable for Uncounted {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Uncounted) })
    }
}

impl Resource for Uncounted {
    type Model = Deal;

    const LABEL: &'static str = "Uncounted deals";
    const SINGULAR: &'static str = "Uncounted deal";
    const SLUG: &'static str = "uncounted";
    const TITLE: &'static str = "title";
    const SIDEBAR_COUNT: bool = false;
    const EXPORT: bool = false;

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("title")]
    }
}

/// A list that counts its rows up to 25.
struct Capped;

impl Injectable for Capped {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Capped) })
    }
}

impl Resource for Capped {
    type Model = Deal;

    const LABEL: &'static str = "Capped deals";
    const SINGULAR: &'static str = "Capped deal";
    const SLUG: &'static str = "capped";
    const TITLE: &'static str = "title";
    const PER_PAGE: u64 = 10;
    const COUNT_CAP: u64 = 25;
    const DEFAULT_SORT: &'static str = "title";
    const SIDEBAR_COUNT: bool = false;

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("title").sortable()]
    }
}

struct CountsModule;

impl Module for CountsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.resource::<Counted>()
            .resource::<Uncounted>()
            .resource::<Capped>();
    }
}

async fn counts_app() -> TestApp {
    builder().module(CountsModule).build().await
}

async fn counts(client: &TestClient) -> Value {
    client
        .get("/admin/counts")
        .send()
        .await
        .assert_status(200)
        .json_value()["counts"]
        .clone()
}

#[tokio::test]
async fn export_reads_a_chunk_at_a_time_and_every_row_once() {
    let _alone = SERIAL.lock().await;
    let app = counts_app().await;
    let (_, manager) = user(&app, "manager").await;
    many(&app, 1_250, None).await;
    QUERIES.store(0, Ordering::SeqCst);
    let body = manager
        .get("/admin/resources/counted/export.csv?sort=-title")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    let titles: Vec<&str> = body.lines().skip(1).collect();
    assert_eq!(titles.len(), 1_250);
    let expected: Vec<String> = (0..1_250).rev().map(|i| format!("Bulk {i:04}")).collect();
    assert_eq!(
        titles,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    // 500 + 500 + 250: one query for each, none for the whole.
    assert_eq!(QUERIES.load(Ordering::SeqCst), 3);
    // A resource that says it is not exported isn't.
    manager
        .get("/admin/resources/uncounted/export.csv")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(403);
}

// ---------------------------------------------------------------------------
// The sidebar's counts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_sidebar_counts_per_user_and_stops_at_a_cap() {
    let _alone = SERIAL.lock().await;
    let app = counts_app().await;
    // Whoever is given a role changes what everyone may do, and counts are
    // read again: the users are made first.
    let (_, manager) = user(&app, "manager").await;
    let (own_id, own_only) = user(&app, "own-only").await;
    let (_, rep) = user(&app, "rep").await;
    let id = Uuid::now_v7();
    give_role(&app, id, "empty", &["helm.view"]).await;
    let empty = app.acting_as(navree_testing::principal(id));
    many(&app, 7, Some(own_id)).await;
    many(&app, 3, None).await;
    let seen = counts(&manager).await;
    assert_eq!(
        seen["deals"],
        json!({ "value": 10, "capped": false, "text": "10" })
    );
    assert_eq!(seen["counted"]["text"], "10");
    // A user whose rules show some of the rows counts those, and nobody else's.
    let theirs = counts(&own_only).await;
    assert_eq!(theirs["deals"]["text"], "7", "{theirs}");
    // What is off is not counted; what the user may not read is not either.
    assert!(seen.get("uncounted").is_none(), "{seen}");
    assert!(counts(&rep).await.get("companies").is_some());
    assert_eq!(counts(&empty).await, json!({}));

    // It is cached: rows made since are not counted yet, and one user is
    // never handed another's count.
    many(&app, 1_000, None).await;
    assert_eq!(counts(&manager).await["deals"]["text"], "10");
    assert_eq!(counts(&own_only).await["deals"]["text"], "7");
    assert_eq!(counts(&rep).await["deals"]["text"], "10");
}

#[tokio::test]
async fn a_count_stops_at_a_cap() {
    let _alone = SERIAL.lock().await;
    let app = counts_app().await;
    let (_, manager) = user(&app, "manager").await;
    many(&app, 1_001, None).await;
    let capped = counts(&manager).await;
    assert_eq!(
        capped["deals"],
        json!({ "value": 1000, "capped": true, "text": "1,000+" })
    );
    // Exactly the cap is exact.
    let app = counts_app().await;
    let (_, manager) = user(&app, "manager").await;
    many(&app, 1_000, None).await;
    assert_eq!(
        counts(&manager).await["deals"],
        json!({ "value": 1000, "capped": false, "text": "1,000" })
    );
}

#[tokio::test]
async fn counts_are_read_again_when_what_a_user_may_do_changes() {
    let _alone = SERIAL.lock().await;
    let app = counts_app().await;
    let (_, manager) = user(&app, "manager").await;
    many(&app, 4, None).await;
    assert_eq!(counts(&manager).await["deals"]["text"], "4");
    many(&app, 2, None).await;
    assert_eq!(counts(&manager).await["deals"]["text"], "4");
    // A role given to anyone makes every count be read again.
    user(&app, "rep").await;
    assert_eq!(counts(&manager).await["deals"]["text"], "6");
}

#[tokio::test]
async fn a_page_does_not_count_the_menu() {
    let _alone = SERIAL.lock().await;
    let app = counts_app().await;
    let (_, manager) = user(&app, "manager").await;
    many(&app, 3, None).await;
    QUERIES.store(0, Ordering::SeqCst);
    let html = plain(
        page(&manager, "/admin/resources/deals")
            .await
            .assert_status(200),
    );
    assert_eq!(
        QUERIES.load(Ordering::SeqCst),
        0,
        "a page of another list counted it"
    );
    // The menu has a place for each number and the script fills it.
    assert!(html.contains("data-count=\"counted\""), "{html}");
    assert!(html.contains("data-count=\"deals\""), "{html}");
    assert!(!html.contains("data-count=\"uncounted\""), "{html}");
    assert!(
        !html.contains("<span class=\"menu-count\" data-count=\"deals\" hidden>3"),
        "{html}"
    );
    let html = plain(page(&manager, "/admin").await.assert_status(200));
    assert_eq!(QUERIES.load(Ordering::SeqCst), 0);
    assert!(html.contains("data-count"), "{html}");
    // Asking for the counts is what counts.
    counts(&manager).await;
    assert_eq!(QUERIES.load(Ordering::SeqCst), 1);
    counts(&manager).await;
    assert_eq!(QUERIES.load(Ordering::SeqCst), 1, "the second was cached");
}

// ---------------------------------------------------------------------------
// The toolbar
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filters_are_chips_in_a_toolbar_not_list_boxes() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    let acme = company(&app, "Acme", None).await;
    deal(
        &app,
        DealSeed {
            company: Some(acme.id),
            ..DealSeed::default()
        },
    )
    .await;
    let html = plain(
        page(
            &manager,
            "/admin/resources/deals?filter%5Bstage%5D%5Bin%5D=won%2Clost&filter%5Bopen%5D=1&search=Deal&sort=title",
        )
        .await
        .assert_status(200),
    );
    assert!(!html.contains("multiple"), "no list boxes: {html}");
    assert!(!html.contains("class=\"card filters\""), "{html}");
    assert!(html.contains("class=\"toolbar\""), "{html}");
    // One chip for each filter, the active ones saying what they hold.
    assert_eq!(html.matches("<details class=\"chip").count(), 5, "{html}");
    assert!(html.contains("class=\"chip is-active\""), "{html}");
    assert!(
        html.contains("<span class=\"chip-value\">Won +1</span>"),
        "{html}"
    );
    assert!(
        html.contains("<span class=\"chip-value\">Yes</span>"),
        "{html}"
    );
    // The options are checkboxes (a boolean's are radios), checked as the query says.
    assert!(
        html.contains("<input type=\"checkbox\" name=\"filter[stage][in]\" value=\"won\" checked>"),
        "{html}"
    );
    assert!(
        html.contains("<input type=\"radio\" name=\"filter[open]\" value=\"1\" checked>"),
        "{html}"
    );
    assert!(html.contains("name=\"filter[min_amount]\""), "{html}");
    assert!(
        html.contains("name=\"filter[expected_close_date]\""),
        "{html}"
    );
    // The search, the sort and the way back.
    assert!(
        html.contains("type=\"search\" id=\"search\" name=\"search\" value=\"Deal\""),
        "{html}"
    );
    assert!(
        html.contains("<input type=\"hidden\" name=\"sort\" value=\"title\">"),
        "{html}"
    );
    assert!(html.contains("Reset"), "{html}");
    // A relationship's options are the records the user may read.
    assert!(html.contains(&format!("value=\"{}\"", acme.id)), "{html}");
    // Strict pages: no inline style, no inline script.
    assert!(!html.contains("style=\""), "{html}");
    assert!(!html.contains("<script>"), "{html}");
    // The form is a GET of the names every Navree list uses.
    assert!(!html.contains("filters["), "{html}");
    assert!(!html.contains("name=\"q\""), "{html}");
    // A chip's Clear link is the list without that filter, written the same way.
    assert!(
        html.contains(
            "href=\"/admin/resources/deals?filter%5Bopen%5D=1&amp;sort=title&amp;search=Deal\""
        ),
        "{html}"
    );
    assert!(
        html.contains("href=\"/admin/resources/deals?filter%5Bstage%5D%5Bin%5D=won%2Clost&amp;sort=title&amp;search=Deal\""),
        "{html}"
    );
    // So is a column header's sort link.
    assert!(
        html.contains("href=\"/admin/resources/deals?filter%5Bopen%5D=1&amp;filter%5Bstage%5D%5Bin%5D=won%2Clost&amp;sort=-title&amp;search=Deal\""),
        "{html}"
    );
    let rows = plain(
        page(&manager, "/admin/resources/deals?filter%5Bstage%5D=won")
            .await
            .assert_status(200),
    );
    assert!(rows.contains("No deals match these filters"), "{rows}");
}

#[tokio::test]
async fn the_pages_read_the_list_query_and_leave_the_rest() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    deal(
        &app,
        DealSeed {
            title: "Alpha",
            stage: "lead",
            ..DealSeed::default()
        },
    )
    .await;
    let shows = |html: &str| html.contains("Alpha") && !html.contains("match these filters");
    // `filter[stage][in]=a,b` and `filter[stage]=a` filter.
    for (query, shown) in [
        ("filter%5Bstage%5D%5Bin%5D=lead%2Cwon", true),
        (
            "filter%5Bstage%5D%5Bin%5D=lead&filter%5Bstage%5D%5Bin%5D=won",
            true,
        ),
        ("filter%5Bstage%5D=lead", true),
        ("filter%5Bstage%5D%5Bin%5D=won%2Clost", false),
        ("filter%5Bstage%5D=won", false),
    ] {
        let html = plain(
            page(&manager, &format!("/admin/resources/deals?{query}"))
                .await
                .assert_status(200),
        );
        assert_eq!(shows(&html), shown, "{query}: {html}");
    }
    // What is not the list query's, and what Helm does not read, changes nothing.
    for query in [
        "filters%5Bstage%5D=won",
        "filters%5Bstage%5D=won%2Clost",
        "q=nothing-matches-this",
        "filter%5Bstage%5D%5Bregex%5D=x",
        "filter%5Bstage%5D%5Bgte%5D=won",
        "filter%5Bstage%5D%5Bnull%5D=true",
        "cursor=abc",
        "sort=-",
    ] {
        let html = plain(
            page(&manager, &format!("/admin/resources/deals?{query}"))
                .await
                .assert_status(200),
        );
        assert!(shows(&html), "{query}: {html}");
        assert!(!html.contains("Reset"), "{query}: {html}");
    }
    // `search` searches.
    let html = plain(
        page(
            &manager,
            "/admin/resources/deals?search=nothing-matches-this",
        )
        .await
        .assert_status(200),
    );
    assert!(html.contains("No deals match these filters"), "{html}");
    // A page that is not a number is a 400 on a page too.
    page(&manager, "/admin/resources/deals?page=0")
        .await
        .assert_status(400);
    page(&manager, "/admin/resources/deals?per_page=101")
        .await
        .assert_status(400);
}

#[tokio::test]
async fn a_link_of_the_list_opens_the_same_list() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    for (title, stage) in [("Alpha", "lead"), ("Beta", "won"), ("Gamma", "lost")] {
        deal(
            &app,
            DealSeed {
                title,
                stage,
                ..DealSeed::default()
            },
        )
        .await;
    }
    let html = plain(
        page(
            &manager,
            "/admin/resources/deals?filter%5Bstage%5D%5Bin%5D=lead%2Cwon&sort=title&search=a",
        )
        .await
        .assert_status(200),
    );
    // The export link carries the filter as the page read it.
    let start = html
        .find("href=\"/admin/resources/deals/export.csv?")
        .expect("an export link")
        + "href=\"".len();
    let end = html[start..].find('"').unwrap() + start;
    let href = html[start..end].replace("&amp;", "&");
    assert_eq!(
        href,
        "/admin/resources/deals/export.csv?filter%5Bstage%5D%5Bin%5D=lead%2Cwon&sort=title&search=a"
    );
    let body = manager
        .get(&href)
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(body.contains("Alpha"), "{body}");
    assert!(body.contains("Beta"), "{body}");
    assert!(!body.contains("Gamma"), "{body}");

    // The page links keep the filter, the sort and the page size.
    let html = plain(
        page(
            &manager,
            "/admin/resources/deals?filter%5Bstage%5D%5Bin%5D=lead%2Cwon&sort=title&per_page=1",
        )
        .await
        .assert_status(200),
    );
    let next = "/admin/resources/deals?filter%5Bstage%5D%5Bin%5D=lead%2Cwon&amp;sort=title&amp;page=2&amp;per_page=1";
    assert!(
        html.contains(&format!("href=\"{next}\" rel=\"next\"")),
        "{html}"
    );
    assert!(html.contains("Alpha") && !html.contains("Beta"), "{html}");
    let second = plain(
        page(&manager, &next.replace("&amp;", "&"))
            .await
            .assert_status(200),
    );
    assert!(
        second.contains("Beta") && !second.contains("Alpha"),
        "{second}"
    );
}

#[tokio::test]
async fn bulk_actions_are_a_menu_and_a_run_button() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    let (_, reader) = user(&app, "no-secrets").await;
    let own = deal(
        &app,
        DealSeed {
            owner: Some(rep_id),
            ..DealSeed::default()
        },
    )
    .await;
    let html = plain(
        page(&rep, "/admin/resources/deals")
            .await
            .assert_status(200),
    );
    assert!(
        html.contains(
            "<form id=\"bulk\" method=\"post\" action=\"/admin/resources/deals/actions\""
        ),
        "{html}"
    );
    assert!(
        html.contains("<select name=\"action\" id=\"bulk-action\" form=\"bulk\""),
        "{html}"
    );
    assert!(
        html.contains("<option value=\"mark-won\" data-confirm=\"Mark the selected deals as won?\">Mark as won</option>"),
        "{html}"
    );
    assert!(
        html.contains("<option value=\"mark-lost\">Mark as lost</option>"),
        "{html}"
    );
    assert!(
        html.contains("form=\"bulk\" data-bulk-run>Run</button>"),
        "{html}"
    );
    assert!(!html.contains("value=\"fail\""), "{html}");
    assert!(
        html.contains(&format!("name=\"ids\" value=\"{}\"", own.id)),
        "{html}"
    );
    // Someone the rules give no action to is offered no menu, no Run and no
    // boxes (they may still be refused one by hand).
    let html = plain(
        page(&reader, "/admin/resources/deals")
            .await
            .assert_status(200),
    );
    assert!(!html.contains("id=\"bulk-action\""), "{html}");
    assert!(!html.contains("data-bulk-run"), "{html}");
    assert!(!html.contains("name=\"ids\""), "{html}");
    assert!(!html.contains("mark-won"), "{html}");
    let response = reader
        .post("/admin/resources/deals/actions")
        .header("accept", "text/html")
        .form(&[
            ("action", "mark-won"),
            ("ids", &own.id.to_string()),
            ("_confirmed", "1"),
        ])
        .send()
        .await;
    assert_ne!(response.status().as_u16(), 200);
    let kept = reload(&app, own.id).await.unwrap();
    assert_eq!(kept.stage, "lead");
    // What the menu runs, runs: the form is the one that always ran.
    let html = plain(
        rep.post("/admin/resources/deals/actions")
            .header("accept", "text/html")
            .form(&[("action", "mark-won"), ("ids", &own.id.to_string())])
            .send()
            .await
            .assert_status(200),
    );
    assert!(html.contains("Mark the selected deals as won?"), "{html}");
}

#[tokio::test]
async fn the_page_offers_export_for_what_it_shows() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    let html = plain(
        page(
            &manager,
            "/admin/resources/deals?filter%5Bstage%5D=won&sort=title&page=3",
        )
        .await
        .assert_status(200),
    );
    assert!(
        html.contains(
            "href=\"/admin/resources/deals/export.csv?filter%5Bstage%5D=won&amp;sort=title\""
        ),
        "{html}"
    );
    assert!(html.contains("Export</a>"), "{html}");
    // Nothing that changes anything sits behind a link: export is a read.
    let off = plain(
        page(&manager, "/admin/resources/companies")
            .await
            .assert_status(200),
    );
    assert!(off.contains("companies/export.csv"));
}

// ---------------------------------------------------------------------------
// The delta on a metric
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_delta_shows_only_where_a_previous_period_has_something_to_compare() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    // Today is the 26th of October: "New deals" counts the last 30 days.
    for days in [1, 2, 3, 40] {
        deal(
            &app,
            DealSeed {
                created_at: now() - Duration::days(days),
                ..DealSeed::default()
            },
        )
        .await;
    }
    let html = plain(
        page(&manager, "/admin/resources/deals")
            .await
            .assert_status(200),
    );
    // Three now, one before: up 200 %.
    assert!(html.contains("class=\"delta up\">"), "{html}");
    assert!(html.contains("+200%</span>"), "{html}");
    assert!(html.contains("On the previous 30 days"), "{html}");
    // The ranges: the 7-day range has nothing before it to compare with.
    let html = plain(
        page(&manager, "/admin/resources/deals?range=7")
            .await
            .assert_status(200),
    );
    assert!(
        !html.contains("class=\"delta"),
        "no previous period, no delta: {html}"
    );
    // A value with no range has no previous period at all: "Open pipeline".
    assert!(html.contains("Open pipeline"), "{html}");
}

async fn page_of(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

#[tokio::test]
async fn a_list_counts_its_rows_only_up_to_a_cap_and_still_pages_through_them() {
    let app = counts_app().await;
    let (_, manager) = user(&app, "manager").await;
    many(&app, 60, None).await;
    let list = |page: u32| {
        let manager = manager.clone();
        async move {
            plain(
                page_of(&manager, &format!("/admin/resources/capped?page={page}"))
                    .await
                    .assert_status(200),
            )
        }
    };
    // 60 rows, counted to 25: the page says so, and not how many pages there are.
    let first = list(1).await;
    assert!(first.contains("1–10 of 25+"), "{first}");
    assert!(first.contains("Page 1</span>"), "{first}");
    assert!(first.contains("rel=\"next\""), "{first}");
    assert!(first.contains("Bulk 0000") && first.contains("Bulk 0009"));
    assert!(!first.contains("Bulk 0010"));
    // Past the cap the pages go on, to the last row and no further.
    let fifth = list(5).await;
    assert!(fifth.contains("41–50 of 25+"), "{fifth}");
    assert!(
        fifth.contains("Bulk 0040") && fifth.contains("Bulk 0049"),
        "{fifth}"
    );
    let last = list(6).await;
    assert!(
        last.contains("Bulk 0050") && last.contains("Bulk 0059"),
        "{last}"
    );
    assert!(!last.contains("rel=\"next\""), "{last}");
    assert!(last.contains("rel=\"prev\""), "{last}");
    // The API counts them all.
    let body = manager
        .get("/admin/api/resources/capped?per_page=10")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["meta"]["total"], 60);
    assert_eq!(body["meta"]["last_page"], 6);
}

#[tokio::test]
async fn under_its_cap_a_list_is_counted_exactly() {
    let app = counts_app().await;
    let (_, manager) = user(&app, "manager").await;
    many(&app, 20, None).await;
    let html = plain(
        page_of(&manager, "/admin/resources/capped?page=2")
            .await
            .assert_status(200),
    );
    assert!(html.contains("11–20 of 20</p>"), "{html}");
    assert!(html.contains("Page 2 of 2"), "{html}");
    assert!(!html.contains("rel=\"next\""), "{html}");
    // A page past the end is empty, as it was.
    let html = plain(
        page_of(&manager, "/admin/resources/capped?page=9")
            .await
            .assert_status(200),
    );
    assert!(html.contains("No capped deals yet"), "{html}");
}

/// A resource whose query fails when it runs.
struct BrokenCount;

impl Injectable for BrokenCount {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(BrokenCount) })
    }
}

impl Resource for BrokenCount {
    type Model = Deal;

    const LABEL: &'static str = "Broken";
    const SLUG: &'static str = "a-broken";
    const TITLE: &'static str = "title";
    const GROUP: &'static str = "AAA";
    const ORDER: i32 = -100;

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("title")]
    }

    fn query(&self, query: Builder<Deal>) -> Builder<Deal> {
        query.where_(navree_orm::Filter::raw(
            "no_such_column = 1",
            Vec::<navree_orm::Value>::new(),
        ))
    }
}

struct BrokenModule;

impl Module for BrokenModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.resource::<BrokenCount>();
    }
}

#[tokio::test]
async fn one_failing_count_leaves_the_others_in_the_menu() {
    let app = builder().module(BrokenModule).build().await;
    deal(&app, DealSeed::default()).await;
    company(&app, "One", None).await;
    let (_, manager) = user(&app, "manager").await;
    let body = manager
        .get("/admin/counts")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(body["counts"].get("a-broken").is_none(), "{body}");
    assert_eq!(body["counts"]["deals"]["value"], 1, "{body}");
    assert_eq!(body["counts"]["companies"]["value"], 1, "{body}");
}
