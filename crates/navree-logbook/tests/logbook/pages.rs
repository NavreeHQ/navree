//! The pages: the shell with the entry types down the side, the list and the open
//! entry in two panes, the state of the list in every address, and what hostile
//! recorded data does to all of it.

use std::time::Duration;

use chrono::Utc;
use http::StatusCode;
use navree_logbook::{EntryKind, Logbook, LogbookModule};
use navree_telemetry::{
    AiCall, EventContext, JobProcessed, MailSent, SignInEvent, SignInMethod, StorageOp,
    StorageOperation, TelemetryEvent, TelemetryRecord, TelemetrySink,
};
use serde_json::json;

use crate::app::{FixturesModule, app, builder};
use crate::support::{feed, in_job, in_request, query_event, request_event};
use crate::ui::unescape;

/// An app that has served two requests and recorded a failed one, some work and a mail.
async fn busy() -> navree_testing::TestApp {
    let app = app().await;
    for n in 0..2 {
        app.client()
            .post("/echo")
            .json(&json!({ "n": n }))
            .send()
            .await
            .assert_status(200);
    }
    app.client().get("/boom").send().await.assert_status(500);
    app.client()
        .post("/work")
        .json(&json!({}))
        .send()
        .await
        .assert_status(200);
    let logbook = app.get::<Logbook>().await;
    logbook.record(&TelemetryRecord {
        at: Utc::now(),
        context: EventContext::default(),
        event: TelemetryEvent::MailSent(MailSent {
            to: vec!["anu@example.com".into()],
            cc: Vec::new(),
            bcc: Vec::new(),
            from: None,
            subject: "Welcome".into(),
            text: Some("Hi".into()),
            html: Some("<h1>Hi Anu</h1>".into()),
            transport: "log".into(),
            duration: Duration::from_millis(1),
        }),
    });
    app
}

async fn get(app: &navree_testing::TestApp, path: &str) -> String {
    unescape(
        &app.client()
            .get(path)
            .send()
            .await
            .assert_status(200)
            .text(),
    )
}

async fn head_id(app: &navree_testing::TestApp, filter: &str) -> String {
    let logbook = app.get::<Logbook>().await;
    logbook
        .entries(&navree_logbook::ListQuery::new(
            navree_logbook::Filter::parse(filter).unwrap(),
            1,
        ))
        .await
        .unwrap()
        .remove(0)
        .id
}

/// The `<a data-slot="sidebar-item" ...>` tags of a page.
fn sidebar_items(html: &str) -> Vec<&str> {
    html.split("<a data-slot=\"sidebar-item\"")
        .skip(1)
        .map(|rest| &rest[..rest.find('>').unwrap_or(rest.len())])
        .collect()
}

/// The attributes of the opening tag that starts `html`.
fn tag_of(html: &str) -> &str {
    &html[..html.find('>').unwrap_or(html.len())]
}

/// The names of the attributes of an opening tag's text, skipping what is inside quotes.
fn attribute_names(tag: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut quote: Option<char> = None;
    let mut word = String::new();
    let mut after_equals = false;
    for c in tag.chars().chain(std::iter::once(' ')) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c.is_whitespace() => {
                if !word.is_empty() && !after_equals {
                    names.push(word.to_ascii_lowercase());
                }
                word.clear();
                after_equals = false;
            }
            None if c == '=' => {
                if !word.is_empty() {
                    names.push(word.to_ascii_lowercase());
                }
                word.clear();
                after_equals = true;
            }
            None => word.push(c),
        }
    }
    names
}

fn urlencode(text: &str) -> String {
    form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

#[tokio::test]
async fn the_page_is_the_toolkits_shell_with_every_entry_type_in_it() {
    let app = busy().await;
    let html = get(&app, "/_logbook").await;
    assert!(html.contains("data-slot=\"sidebar-wrapper\""), "{html}");
    let items = sidebar_items(&html);
    assert_eq!(
        items.len(),
        1 + EntryKind::ALL.len(),
        "All, and one for each type"
    );
    for kind in EntryKind::ALL {
        assert!(
            items
                .iter()
                .any(|item| item.contains(&format!("title=\"{}\"", kind.label()))
                    && item.contains(&format!("type={}", kind.as_str()))),
            "{}: {items:?}",
            kind.label()
        );
    }
    let requests = items
        .iter()
        .find(|i| i.contains("title=\"Requests\""))
        .unwrap();
    assert!(
        requests.contains("href=\"/_logbook?type=request\""),
        "{requests}"
    );
    // The counts are the store's: four requests, one mail.
    assert!(
        html.contains("data-slot=\"sidebar-count\">4<"),
        "four requests: {html}"
    );
    assert!(
        html.contains("data-slot=\"sidebar-count\">1<"),
        "one mail: {html}"
    );
    let current: Vec<&&str> = items
        .iter()
        .filter(|i| i.contains("aria-current"))
        .collect();
    assert_eq!(current.len(), 1, "only All is current: {current:?}");
    assert!(current[0].contains("title=\"All\""));

    // A type is current when the address says so, and the filter text goes along.
    let html = get(&app, "/_logbook?q=status%3A2xx&type=query").await;
    let items = sidebar_items(&html);
    let current: Vec<&&str> = items
        .iter()
        .filter(|i| i.contains("aria-current"))
        .collect();
    assert_eq!(current.len(), 1, "{current:?}");
    assert!(current[0].contains("title=\"Queries\""), "{current:?}");
    assert!(
        items
            .iter()
            .any(|i| i.contains("href=\"/_logbook?q=status%3A2xx&type=request\"")),
        "{items:?}"
    );
}

#[tokio::test]
async fn an_entry_has_a_url_of_its_own_and_opens_beside_the_list() {
    let app = busy().await;
    let id = head_id(&app, "type:request").await;
    let page = get(&app, &format!("/_logbook/entries/{id}?type=request")).await;
    // Beside the list: the list's rows, and the entry's sections.
    assert!(page.contains("id=\"rows-body\""), "{page}");
    for section in [
        "id=\"request\"",
        "id=\"response\"",
        "id=\"timeline\"",
        "id=\"replay\"",
    ] {
        assert!(page.contains(section), "{section}");
    }
    // The way back keeps the list as it was.
    assert!(
        page.contains("href=\"/_logbook?type=request\""),
        "the way back to the list as it was: {page}"
    );
    // Its row is marked, and the link of every row keeps the list's filter.
    let marked: Vec<&str> = page
        .split("<a data-slot=\"item\"")
        .skip(1)
        .filter(|row| tag_of(row).contains("aria-current=\"true\""))
        .collect();
    assert_eq!(marked.len(), 1, "{page}");
    assert!(
        tag_of(marked[0]).contains(&format!("/_logbook/entries/{id}?type=request")),
        "{}",
        tag_of(marked[0])
    );
    // On a phone the entry has the page: the pane with the rows is hidden, the one
    // with the entry is not.
    let panes: Vec<&str> = page.split("data-pane").skip(1).collect();
    assert_eq!(panes.len(), 2, "{page}");
    assert!(tag_of(panes[0]).contains("data-hide-mobile"), "{page}");
    assert!(!tag_of(panes[1]).contains("data-hide-mobile"), "{page}");

    // With nothing chosen the list has the phone and the other pane waits.
    let list = get(&app, "/_logbook").await;
    assert!(
        list.contains("Choose an entry to see what it did."),
        "{list}"
    );
    let list_pane = list.split("data-pane").nth(1).unwrap();
    assert!(!tag_of(list_pane).contains("data-hide-mobile"), "{list}");
    assert!(!list.contains("id=\"timeline\""));
    assert!(
        !list.contains("<table"),
        "a table is what a phone clips: {list}"
    );
}

#[tokio::test]
async fn an_entry_that_is_not_there_says_so_beside_the_list() {
    let app = busy().await;
    let response = app
        .client()
        .get("/_logbook/entries/nope?type=request")
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let page = unescape(&response.text());
    assert!(page.contains("There is no entry with this id"), "{page}");
    assert!(
        page.contains("id=\"rows-body\""),
        "the list is still there: {page}"
    );
    assert!(
        page.contains("data-slot=\"sidebar\""),
        "so is the shell: {page}"
    );
}

#[tokio::test]
async fn the_filter_submits_to_the_page_it_is_on() {
    let app = busy().await;
    let id = head_id(&app, "type:request").await;
    let page = get(
        &app,
        &format!("/_logbook/entries/{id}?type=request&q=status%3A2xx"),
    )
    .await;
    let form = page.split("role=\"search\"").nth(1).expect("a search form");
    let form = &form[..form.find("</form>").unwrap()];
    assert!(
        form.contains(&format!("action=\"/_logbook/entries/{id}\"")),
        "{form}"
    );
    assert!(form.contains("name=\"type\" value=\"request\""), "{form}");
    assert!(form.contains("value=\"status:2xx\""), "{form}");
    assert!(form.contains("data-ui-target=\"rows\""), "{form}");
}

#[tokio::test]
async fn older_entries_keep_the_entry_open() {
    let app = app().await;
    let logbook = app.get::<Logbook>().await;
    for n in 0..60 {
        let id = format!("req-{n}");
        feed(
            &logbook,
            &in_request(&id),
            request_event(&id, "GET", &format!("/p/{n}"), 200),
        );
    }
    let open = head_id(&app, "type:request").await;
    let page = get(&app, &format!("/_logbook/entries/{open}")).await;
    let older = page
        .split("href=\"")
        .map(|part| &part[..part.find('"').unwrap_or(0)])
        .find(|href| href.contains("before="))
        .expect("an Older entries link");
    assert!(
        older.starts_with(&format!("/_logbook/entries/{open}?before=")),
        "{older}"
    );
    let older_page = get(&app, older).await;
    assert!(older_page.contains("GET /p/"), "{older_page}");
}

#[tokio::test]
async fn jobs_say_what_became_of_them_and_what_they_did_inside() {
    let app = app().await;
    let logbook = app.get::<Logbook>().await;
    let job = in_job("job-1");
    feed(&logbook, &job, query_event("SELECT * FROM deals", 3));
    feed(
        &logbook,
        &job,
        TelemetryEvent::JobProcessed(JobProcessed {
            queue: "default".into(),
            job_type: "deals.notify-owner".into(),
            job_id: "job-1".into(),
            attempt: 1,
            duration: Duration::from_millis(112),
        }),
    );
    let page = get(&app, "/_logbook").await;
    assert!(page.contains("deals.notify-owner"), "{page}");
    assert!(page.contains(">processed<"), "the outcome: {page}");
    assert!(page.contains("1 q"), "what it did inside: {page}");
    assert!(page.contains("112 ms"), "{page}");
}

#[tokio::test]
async fn nothing_is_fetched_ahead_of_a_click() {
    let app = busy().await;
    let id = head_id(&app, "type:request").await;
    let mail = head_id(&app, "type:mail").await;
    for path in [
        "/_logbook".to_owned(),
        format!("/_logbook/entries/{id}"),
        format!("/_logbook/entries/{mail}"),
    ] {
        let page = get(&app, &path).await;
        // A link is fetched ahead of a click only when its page asks, and Logbook asks
        // for none: its pages show what was just recorded and are never cached.
        for tag in page.split('<').skip(1).map(tag_of) {
            assert!(
                !tag.contains("data-ui-prefetch"),
                "{path}: <{tag}> is fetched ahead of a click"
            );
        }
        // What changes anything is a form that posts, never a link.
        assert!(
            page.contains("method=\"post\" action=\"/_logbook/clear\""),
            "{path}"
        );
        assert!(!page.contains("href=\"/_logbook/clear\""), "{path}");
    }
}

#[tokio::test]
async fn the_colours_are_the_apps_and_not_the_systems() {
    let app = busy().await;
    let page = get(&app, "/_logbook").await;
    assert!(
        page.contains("<meta name=\"color-scheme\" content=\"dark\">"),
        "{page}"
    );
    let sheets: Vec<&str> = page
        .split("<link rel=\"stylesheet\" href=\"")
        .skip(1)
        .collect();
    assert_eq!(sheets.len(), 1, "the toolkit's stylesheet and nothing else");
    let href = sheets[0].split('"').next().unwrap();
    assert!(href.starts_with("/_navree/ui/navree-ui-"), "{href}");
    let css = app
        .client()
        .get(href)
        .send()
        .await
        .assert_status(200)
        .text();
    // The stylesheet follows the system only on a page in system mode, and Logbook's
    // page names no mode of its own: it stays dark.
    let root = &page[page.find("<html").unwrap()..];
    let root = &root[..root.find('>').unwrap()];
    assert!(!root.contains("data-mode"), "{root}");
    assert!(
        css.match_indices("prefers-color-scheme")
            .all(|(at, _)| css[at..]
                .starts_with("prefers-color-scheme:light){:where([data-mode=system])")),
        "the stylesheet switches to light with the system only in system mode"
    );
    assert!(css.contains("color-scheme:dark"));
}

#[tokio::test]
async fn hostile_recorded_data_is_escaped_everywhere_it_is_shown() {
    let app = app().await;
    let logbook = app.get::<Logbook>().await;
    let script = "<script>alert(1)</script>";
    let image = "\"><img src=x onerror=alert(1)>";
    let svg = "</code></pre><svg onload=alert(1)>";
    let mut context = in_request("hostile-1");
    context.user_id = Some(format!("u{image}"));
    feed(
        &logbook,
        &context,
        query_event(&format!("SELECT '{script}' -- {svg}"), 1),
    );
    feed(
        &logbook,
        &context,
        TelemetryEvent::MailSent(MailSent {
            to: vec![format!("{image}@example.com")],
            cc: Vec::new(),
            bcc: Vec::new(),
            from: Some(script.into()),
            subject: format!("{script}{image}"),
            text: Some(svg.into()),
            html: Some(format!("<h1>{script}</h1>")),
            transport: "log".into(),
            duration: Duration::from_millis(1),
        }),
    );
    logbook.dump(svg, &json!({ "a": script, "b": image }));
    feed(
        &logbook,
        &context,
        request_event("hostile-1", "GET", &format!("/{script}?q={image}"), 200),
    );

    let first = head_id(&app, "type:request").await;
    let mail = head_id(&app, "type:mail").await;
    let dump = head_id(&app, "type:dump").await;
    let pages = [
        "/_logbook".to_owned(),
        format!("/_logbook?q={}", urlencode(&format!("{script}{image}"))),
        format!("/_logbook?type=mail&q=route:{}", urlencode(image)),
        format!("/_logbook/entries/{first}"),
        format!("/_logbook/entries/{mail}"),
        format!("/_logbook/entries/{dump}"),
        format!("/_logbook/entries/{}", urlencode(script)),
        format!(
            "/_logbook/entries/hostile-1?type=request&q={}",
            urlencode(svg)
        ),
        format!("/_logbook/rows?after=0&q={}", urlencode(image)),
        format!("/_logbook/rows?after=0&type={}", urlencode(image)),
    ];
    for path in pages {
        let response = app.client().get(&path).send().await;
        let kind = response
            .header("content-type")
            .unwrap_or_default()
            .to_owned();
        let raw = response.text();
        if kind.starts_with("application/json") {
            // An error that names the bad input is data, and a browser doesn't draw it.
            continue;
        }
        assert!(kind.starts_with("text/html"), "{path}: {kind}");
        for payload in [
            "<script>alert",
            "<img src=x",
            "<svg onload",
            "\"><img",
            "</code></pre><svg",
        ] {
            assert!(
                !raw.contains(payload),
                "{path} writes {payload} as markup:\n{raw}"
            );
        }
        // The only scripts are the toolkit's own files.
        for script in raw.split("<script").skip(1) {
            assert!(
                script.starts_with(" type=\"module\" src=\"/_navree/ui/"),
                "{path}: {script}"
            );
        }
        // No tag has an event handler: escaped text has no `<`, so these are the real tags.
        for tag in raw.split('<').skip(1) {
            let tag = tag_of(tag);
            for name in attribute_names(tag) {
                assert!(!name.starts_with("on"), "{path}: <{tag}> has `{name}`");
            }
        }
        assert!(!raw.contains(" style="), "{path}: no style attribute");
    }
    // The page really shows the text, so the test is not passing on an empty page.
    let page = app
        .client()
        .get(&format!("/_logbook/entries/{first}"))
        .send()
        .await
        .text();
    assert!(page.contains("&lt;script&gt;alert(1)&lt;"), "{page}");
}

#[tokio::test]
async fn the_mail_view_keeps_its_sandbox() {
    let app = app().await;
    let logbook = app.get::<Logbook>().await;
    logbook.record(&TelemetryRecord {
        at: Utc::now(),
        context: EventContext::default(),
        event: TelemetryEvent::MailSent(MailSent {
            to: vec!["anu@example.com".into()],
            cc: Vec::new(),
            bcc: Vec::new(),
            from: None,
            subject: "Hostile".into(),
            text: None,
            html: Some("<script>alert(1)</script><h1>Hi</h1>".into()),
            transport: "log".into(),
            duration: Duration::from_millis(1),
        }),
    });
    let mail = head_id(&app, "type:mail").await;
    let page = app
        .client()
        .get(&format!("/_logbook/mail/{mail}"))
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        page.header("content-security-policy"),
        Some(
            "default-src 'none'; img-src * data:; style-src 'unsafe-inline'; font-src * data:; sandbox"
        )
    );
    assert_eq!(page.header("cache-control"), Some("no-store"));
}

#[tokio::test]
async fn a_logbook_that_is_off_adds_no_route_and_no_layer() {
    // Off: the default of a test app, as in a release build.
    let app = navree_testing::TestApp::builder()
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    assert!(!app.get::<Logbook>().await.is_enabled());
    let routes = navree_http::route_table(app.ctx()).await.unwrap();
    let ours: Vec<String> = routes
        .iter()
        .filter(|route| {
            route.full_path.starts_with("/_logbook")
                || route
                    .name
                    .as_deref()
                    .is_some_and(|name| name.starts_with("logbook"))
                || route.full_path.starts_with("/_navree")
        })
        .map(|route| format!("{} {}", route.method, route.full_path))
        .collect();
    assert!(ours.is_empty(), "{ours:?}");
    for path in [
        "/_logbook",
        "/_logbook?type=query",
        "/_logbook/entries/x",
        "/_logbook/rows",
        "/_logbook/mail/x",
        "/_logbook/api/entries",
        "/_logbook/api/entries/x",
        "/_logbook/api/batches/x",
        "/_logbook/api/requests/x/curl",
    ] {
        app.client().get(path).send().await.assert_status(404);
    }
    for path in [
        "/_logbook/clear",
        "/_logbook/entries/x/replay",
        "/_logbook/api/requests/x/replay",
    ] {
        app.client().post(path).send().await.assert_status(404);
    }
    app.client()
        .delete("/_logbook/api/entries")
        .send()
        .await
        .assert_status(404);
    // The same app, on, has all of them: the test above is not passing on a typo.
    let on = builder()
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    let routes = navree_http::route_table(on.ctx()).await.unwrap();
    let count = routes
        .iter()
        .filter(|r| r.full_path.starts_with("/_logbook"))
        .count();
    assert!(count >= 11, "{count}");
}

#[tokio::test]
async fn the_newer_kinds_show_in_the_list_and_in_the_request_that_made_them() {
    let app = app().await;
    let logbook = app.get::<Logbook>().await;
    let context = in_request("req-platform");
    feed(
        &logbook,
        &context,
        TelemetryEvent::AiCall(
            AiCall::new(
                "anthropic",
                "claude-sonnet-4-5",
                "generate",
                Duration::from_millis(900),
            )
            .tokens(1200, 340)
            .cost_micros(5_400),
        ),
    );
    feed(
        &logbook,
        &context,
        TelemetryEvent::SignIn(SignInEvent::refused(
            SignInMethod::Password,
            "invalid_credentials",
        )),
    );
    feed(
        &logbook,
        &context,
        TelemetryEvent::StorageOp(
            StorageOp::new(
                "uploads",
                "local",
                StorageOperation::Write,
                "kyc/pan.pdf",
                Duration::from_millis(6),
            )
            .bytes(52_314),
        ),
    );
    feed(
        &logbook,
        &context,
        request_event("req-platform", "POST", "/sign-in", 303),
    );

    // The request's row counts what it did.
    let list = get(&app, "/_logbook").await;
    assert!(list.contains("1 ai"), "{list}");
    assert!(list.contains("1 auth"), "{list}");
    assert!(list.contains("1 file"), "{list}");

    // Open, it has a section for each kind, with what each did.
    let id = head_id(&app, "type:request").await;
    let page = get(&app, &format!("/_logbook/entries/{id}")).await;
    for section in ["id=\"ai\"", "id=\"storage\"", "id=\"sign-ins\""] {
        assert!(page.contains(section), "{section}: {page}");
    }
    assert!(page.contains("claude-sonnet-4-5 generate"), "{page}");
    assert!(page.contains("1200 in, 340 out tokens"), "{page}");
    assert!(page.contains("$0.0054"), "{page}");
    assert!(page.contains("password sign-in failure"), "{page}");
    assert!(page.contains("invalid_credentials"), "{page}");
    assert!(page.contains("write kyc/pan.pdf"), "{page}");
    assert!(page.contains("52314 bytes"), "{page}");

    // Each kind has a list of its own, and a type filter reaches it.
    let ai = get(&app, "/_logbook?type=ai").await;
    assert!(ai.contains("claude-sonnet-4-5 generate"), "{ai}");
    assert!(!ai.contains("kyc/pan.pdf"), "{ai}");
}
