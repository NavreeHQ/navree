//! `SafeHtml` (feature `html`): rich text sanitized by the app's
//! `HtmlPolicy` while deserializing, so the handler never sees a script.

use std::sync::Arc;

use http::Method;
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::TestApp;
use navree_validation::Validate;
use navree_validation::{HtmlError, HtmlPolicy, SafeHtml, Valid, ValidationModule};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::support::{app_with, errors};

/// A comment with a rich-text body; its length is the sanitized length.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Comment {
    #[validate(length(min = 1, max = 40))]
    pub body: SafeHtml,
    #[validate(skip)]
    pub signature: Option<SafeHtml>,
}

/// The routes of [`HtmlTestModule`].
pub struct HtmlController;

impl Injectable for HtmlController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(HtmlController) })
    }
}

impl Controller for HtmlController {
    fn prefix(&self) -> &'static str {
        "!/html"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::POST,
                "/comments",
                |Valid(Json(input)): Valid<Json<Comment>>| async move {
                    Json(json!({ "ok": input }))
                },
            )
            .public(),
        ]
    }
}

/// Registers [`HtmlController`].
pub struct HtmlTestModule;

impl Module for HtmlTestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<HtmlController>();
    }
}

async fn sanitized(app: &TestApp, html: &str) -> String {
    let body = app
        .client()
        .post("/html/comments")
        .json(&json!({ "body": "ok", "signature": html }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    body["ok"]["signature"]
        .as_str()
        .expect("a sanitized string")
        .to_owned()
}

#[tokio::test]
async fn the_default_policy_keeps_rich_text_and_drops_scripts() {
    let app = app_with(HtmlTestModule).await;
    for (input, output) in [
        (
            "<p>Hello <strong>world</strong>, <em>hi</em></p>",
            "<p>Hello <strong>world</strong>, <em>hi</em></p>",
        ),
        (
            "<ul><li>one</li><li>two</li></ul><blockquote>q</blockquote>",
            "<ul><li>one</li><li>two</li></ul><blockquote>q</blockquote>",
        ),
        // Right-to-left text keeps its direction and language.
        (
            "<p dir=\"rtl\" lang=\"ar\">مرحبا بالعالم</p>",
            "<p dir=\"rtl\" lang=\"ar\">مرحبا بالعالم</p>",
        ),
        (
            "<a href=\"https://example.com/a?b=c\" title=\"t\">link</a>",
            "<a href=\"https://example.com/a?b=c\" title=\"t\" rel=\"noopener noreferrer nofollow\">link</a>",
        ),
        ("<script>alert(1)</script><p>after</p>", "<p>after</p>"),
        ("<style>body{display:none}</style>text", "text"),
        ("<img src=x onerror=alert(1)>", ""),
        (
            "<a href=\"javascript:alert(1)\">x</a>",
            "<a rel=\"noopener noreferrer nofollow\">x</a>",
        ),
        (
            "<a href=\" JaVaScRiPt:alert(1)\">x</a>",
            "<a rel=\"noopener noreferrer nofollow\">x</a>",
        ),
        (
            "<a href=\"data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==\">x</a>",
            "<a rel=\"noopener noreferrer nofollow\">x</a>",
        ),
        (
            "<p onclick=\"steal()\" style=\"color:red\" class=\"c\" id=\"i\">x</p>",
            "<p>x</p>",
        ),
        ("<svg onload=alert(1)><circle/></svg>", ""),
        ("<iframe src=\"https://evil.example\"></iframe>", ""),
        ("<form action=\"/steal\"><input name=\"pw\"></form>", ""),
        ("<!-- a comment --><b>bold</b>", "<b>bold</b>"),
        ("1 < 2 & 3 > 2", "1 &lt; 2 &amp; 3 &gt; 2"),
    ] {
        assert_eq!(sanitized(&app, input).await, output, "{input}");
    }
}

#[tokio::test]
async fn xss_payloads_never_survive() {
    let app = app_with(HtmlTestModule).await;
    for payload in [
        "<scr<script>ipt>alert(1)</script>",
        "<<script>script>alert(1)<</script>/script>",
        "<IMG SRC=\"jav&#x09;ascript:alert('XSS');\">",
        "<a href=\"jav&#x0A;ascript:alert(1)\">x</a>",
        "<body onload=alert(1)>",
        "<div style=\"background:url(javascript:alert(1))\">x</div>",
        "<math><mtext><table><mglyph><style><img src=x onerror=alert(1)>",
        "<noscript><p title=\"</noscript><img src=x onerror=alert(1)>\">",
        "<object data=\"javascript:alert(1)\"></object>",
        "<embed src=\"javascript:alert(1)\">",
        "<meta http-equiv=\"refresh\" content=\"0;url=javascript:alert(1)\">",
        "<details open ontoggle=alert(1)>",
        "<a href=\"vbscript:msgbox(1)\">x</a>",
        "\"><script>alert(document.cookie)</script>",
        "<template><script>alert(1)</script></template>",
    ] {
        let clean = sanitized(&app, payload).await.to_ascii_lowercase();
        for needle in [
            "<script",
            "javascript:",
            "vbscript:",
            "onerror",
            "onload",
            "ontoggle",
            "<img",
            "<iframe",
            "<object",
            "<embed",
            "<meta",
            "<style",
            "<svg",
            "<math",
        ] {
            assert!(
                !clean.contains(needle),
                "{payload:?} sanitized to {clean:?}, which contains {needle:?}"
            );
        }
    }
}

#[tokio::test]
async fn length_rules_measure_the_sanitized_html() {
    let app = app_with(HtmlTestModule).await;
    // Mostly a script: the body that reaches the rules is short enough.
    let script = format!("<script>{}</script><p>hi</p>", "x".repeat(500));
    app.client()
        .post("/html/comments")
        .json(&json!({ "body": script }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ok.body", json!("<p>hi</p>"));

    // Nothing left after sanitizing: too short.
    let response = app
        .client()
        .post("/html/comments")
        .json(&json!({ "body": "<script>alert(1)</script>" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "body": ["body must be longer than or equal to 1 characters"] })
    );

    let response = app
        .client()
        .post("/html/comments")
        .json(&json!({ "body": ["<p>x</p>"] }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "body": ["body must be a string"] })
    );
}

#[tokio::test]
async fn the_policy_comes_from_the_validation_html_section() {
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config(
            "validation",
            json!({
                "html": {
                    "tags": ["p", "a", "img"],
                    "tag_attributes": { "a": ["href"], "img": ["src", "alt"] },
                    "generic_attributes": [],
                    "url_schemes": ["https"],
                    "link_rel": "",
                },
            }),
        )
        .module(ValidationModule::from_config())
        .module(HtmlTestModule)
        .build()
        .await;
    for (input, output) in [
        ("<p dir=\"rtl\"><strong>x</strong></p>", "<p>x</p>"),
        (
            "<a href=\"https://a.example\">a</a><a href=\"http://b.example\">b</a>",
            "<a href=\"https://a.example\">a</a><a>b</a>",
        ),
        (
            "<img src=\"https://cdn.example/a.png\" alt=\"a\" onerror=\"x()\">",
            "<img src=\"https://cdn.example/a.png\" alt=\"a\">",
        ),
    ] {
        assert_eq!(sanitized(&app, input).await, output, "{input}");
    }
}

#[tokio::test]
async fn an_unsafe_policy_fails_at_boot() {
    for (html, key) in [
        (json!({ "tags": ["p", "script"] }), "validation.html.tags"),
        (
            json!({ "tag_attributes": { "a": ["href", "rel"] } }),
            "validation.html.tag_attributes",
        ),
        (
            json!({ "tag_attributes": { "style": ["media"] } }),
            "validation.html.tag_attributes",
        ),
    ] {
        let built = TestApp::builder()
            .config("validation", json!({ "html": html }))
            .module(ValidationModule::from_config())
            .module(HtmlTestModule)
            .try_build()
            .await;
        let Err(error) = built else {
            panic!("an unsafe policy must not boot: {key}");
        };
        assert!(error.to_string().contains(key), "{key}: {error}");
    }
}

/// What a browser builds from the output is what the sanitizer checked:
/// markup the HTML parser would rearrange (a link in a link, a blank line
/// it drops) comes out the way the parser reads it.
#[test]
fn the_output_parses_back_to_the_tree_that_was_checked() {
    let policy = HtmlPolicy::default();
    for (input, output) in [
        (
            "<a href=\"https://a.example\">1<table><a href=\"https://b.example\">2</a></table></a>",
            "<a href=\"https://a.example\" rel=\"noopener noreferrer nofollow\">1</a>\
             <a href=\"https://b.example\" rel=\"noopener noreferrer nofollow\">2</a><table></table>",
        ),
        ("<pre>\n\nx</pre>", "<pre>x</pre>"),
        ("FOO&#x000D;ZOO", "FOO\nZOO"),
        (
            "<p><button><table></table></button>x</p>",
            "<p></p><table></table>x<p></p>",
        ),
    ] {
        let clean = policy.clean(input);
        assert_eq!(clean, output, "{input}");
        assert_eq!(policy.clean(&clean), clean, "{input}: stable");
    }
}

/// SVG animation elements set another element's attributes, past the
/// allow-list: they go even when a policy lists them.
#[test]
fn svg_animation_elements_are_never_kept() {
    let policy = HtmlPolicy::default()
        .allow_tags(["svg", "circle", "animate", "set", "animateTransform"])
        .allow_attributes("a", ["href"])
        .allow_attributes("animate", ["attributeName", "values", "to", "from"])
        .allow_attributes("set", ["attributeName", "to"]);
    assert_eq!(
        policy.clean(
            "<svg><a href=\"#\"><circle></circle><set attributeName=\"href\" to=\"https://a.example\"></set>\
             <animate attributeName=\"href\" values=\"https://b.example\"></animate></a></svg>"
        ),
        "<svg><a href=\"#\" rel=\"noopener noreferrer nofollow\"><circle></circle></a></svg>"
    );
}

/// A URL attribute keeps its scheme check on any element a policy allows it
/// on.
#[test]
fn url_attributes_are_checked_on_every_element() {
    let policy = HtmlPolicy::default()
        .allow_tags(["div"])
        .allow_attributes("div", ["action", "data", "cite", "title"]);
    assert_eq!(
        policy.clean(
            "<div action=\"javascript:x()\" data=\"https://a.example\" cite=\"note:1\" title=\"note:1\">d</div>"
        ),
        "<div data=\"https://a.example\" title=\"note:1\">d</div>"
    );
}

/// After `<plaintext>` a browser reads everything as text, closing tags
/// too, so no output of it parses back the same: only the text is kept.
#[test]
fn markup_no_output_can_hold_keeps_only_its_text() {
    let policy = HtmlPolicy::default().allow_tags(["plaintext"]);
    assert_eq!(
        policy.clean("<p>a</p><plaintext><b>x</b>"),
        "a&lt;b&gt;x&lt;/b&gt;"
    );
}

/// An element in a namespace it can't be in under its kept parent goes
/// with its content: `<svg>` is gone, so its `<iframe>` (whose content HTML
/// would read as text) isn't written as HTML.
#[test]
fn foreign_elements_out_of_place_go_with_their_content() {
    let policy = HtmlPolicy::default().allow_tags(["iframe"]);
    assert_eq!(
        policy.clean(
            "<svg><iframe><a title=\"</iframe><img src onerror=alert(1)>\">test</a></iframe></svg>x"
        ),
        "x"
    );
    assert_eq!(
        policy.clean("<p><circle>hi</circle> there</p>"),
        "<p> there</p>"
    );
}

/// The content of a `<selectedcontent>` is the browser's copy of the chosen
/// option, never the input's.
#[test]
fn selected_content_is_emptied() {
    let policy = HtmlPolicy::default().allow_tags(["select", "option", "selectedcontent"]);
    assert_eq!(
        policy.clean(
            "<select><selectedcontent><b>x</b></selectedcontent><option><b>y</b></option></select>"
        ),
        "<select><selectedcontent></selectedcontent><option><b>y</b></option></select>"
    );
}

/// Each walk over the tree keeps its own stack: deep nesting can't overflow
/// the thread's.
#[test]
fn deep_nesting_is_cleaned_without_recursion() {
    let depth = 50_000;
    let html = format!("{}x", "<span>".repeat(depth));
    // Far past what a policy may allow: this is about the walks.
    let roomy = |policy: HtmlPolicy| policy.limits(1 << 20, 1 << 20, depth + 1);
    let clean = roomy(HtmlPolicy::default()).clean(&html);
    assert_eq!(
        clean,
        format!("{}x{}", "<span>".repeat(depth), "</span>".repeat(depth))
    );
    assert_eq!(roomy(HtmlPolicy::text_only()).clean(&html), "x");
}

/// `<b aN><p>x` n times: each `<p>` closes the open `<b>`s and the text makes
/// a new copy of every distinct one, so the nodes grow as n squared. 12 KB of
/// this is half a million nodes, and 100 KB would be all of a large server's
/// memory, so the parse stops when it has spent the budget.
#[test]
fn interleaved_formatting_elements_stop_at_the_budget() {
    let bomb = |n: usize| -> String { (0..n).map(|i| format!("<b a{i:x}><p>x")).collect() };
    // Each copy nests in the last, so this much reaches the depth limit first.
    assert_eq!(
        HtmlPolicy::default().try_clean(&bomb(1000)),
        Err(HtmlError::TooDeep { max: 128 })
    );
    // Within the depth limit, one text at a time pays for all the copies.
    let open: String = (0..100).map(|i| format!("<b a{i:x}>")).collect();
    let html = format!("<p>{open}{}", "<p>x".repeat(2000));
    assert_eq!(
        HtmlPolicy::default().try_clean(&html),
        Err(HtmlError::TooManyNodes { max: 128 * 1024 })
    );
    // Below the budget the same markup reads as it always did.
    let small = HtmlPolicy::default().limits(64 * 1024, 1000, 128);
    assert!(small.try_clean(&bomb(10)).is_ok());
    assert_eq!(
        small.try_clean(&bomb(100)),
        Err(HtmlError::TooManyNodes { max: 1000 })
    );
    // Copies of a tag with many attributes, or with long ones, cost their
    // attributes, which are what grows the output.
    let policy = HtmlPolicy::default().limits(1024 * 1024, 10_000, 128);
    let attributes: String = (0..200).map(|i| format!(" a{i}")).collect();
    let html = format!("<p><b{attributes}>{}", "<p>x".repeat(1000));
    assert_eq!(
        policy.try_clean(&html),
        Err(HtmlError::TooManyNodes { max: 10_000 })
    );
    let html = format!(
        "<p><b dir=\"{}\">{}",
        "x".repeat(64_000),
        "<p>x".repeat(1000)
    );
    assert_eq!(
        policy.try_clean(&html),
        Err(HtmlError::TooManyNodes { max: 10_000 })
    );
    // Whatever becomes of a parse, it never panics, and `clean` keeps the text.
    let clean = HtmlPolicy::default().clean(&bomb(1000));
    assert!(
        clean.starts_with("&lt;b a0&gt;&lt;p&gt;x"),
        "{}",
        &clean[..40]
    );
    assert!(!clean.contains('<'));
}

#[test]
fn an_input_over_the_cap_is_refused_before_it_is_parsed() {
    let policy = HtmlPolicy::default().limits(1024, 128 * 1024, 128);
    assert!(policy.try_clean(&"x".repeat(1024)).is_ok());
    assert_eq!(
        policy.try_clean(&"x".repeat(1025)),
        Err(HtmlError::InputTooLarge { max: 1024 })
    );
    let safe = SafeHtml::try_with_policy(&"x".repeat(1025), &policy);
    assert_eq!(safe, Err(HtmlError::InputTooLarge { max: 1024 }));
    // An output longer than the input isn't held to the input's cap.
    assert_eq!(
        policy.try_clean(&"&".repeat(1024)).unwrap(),
        "&amp;".repeat(1024)
    );
}

/// What goes deep: a block scans the parser's stack, so depth is time.
#[test]
fn elements_nested_past_the_limit_are_refused() {
    let policy = HtmlPolicy::default();
    let too_deep = Err(HtmlError::TooDeep { max: 128 });
    for html in [
        "<ul>".repeat(129),
        "<div>".repeat(129),
        "<span>".repeat(129),
        format!("{}{}", "<ul>".repeat(4000), "</p>".repeat(4000)),
        "<svg><foreignObject>".repeat(65),
        "<template>".repeat(129),
        format!("<table>{}", "<b>".repeat(129)),
        "<table><td>".repeat(40),
    ] {
        assert_eq!(policy.try_clean(&html), too_deep, "{}", &html[..30]);
    }
    assert!(policy.try_clean(&"<ul>".repeat(128)).is_ok());
    assert!(policy.try_clean(&"<span>".repeat(128)).is_ok());
    // It costs nothing to refuse, however long the input.
    let started = std::time::Instant::now();
    assert_eq!(policy.try_clean(&"<ul>".repeat(100_000)), too_deep);
    assert!(started.elapsed().as_secs() < 5, "{:?}", started.elapsed());
}

/// The tokenizer compares each attribute with the tag's earlier ones, so one
/// tag can hold it up for the square of its attributes.
#[test]
fn a_tag_with_too_many_attributes_is_refused() {
    let policy = HtmlPolicy::default();
    let many =
        |n: usize, write: &dyn Fn(usize) -> String| -> String { (0..n).map(write).collect() };
    let refused = Err(HtmlError::TooManyAttributes { max: 256 });
    for attribute in [
        &(|i| format!(" a{i}")) as &dyn Fn(usize) -> String,
        &(|i| format!(" a{i}=x")),
        &(|i| format!(" a{i}=\"> <\"")),
        &(|i| format!(" a{i}='>'")),
        &(|i| format!("/a{i}")),
        &(|i| format!(" a{i}=\"\"b{i}=\"\"")),
    ] {
        let attributes = many(300, attribute);
        for html in [format!("<b{attributes}>"), format!("</b{attributes}>")] {
            assert_eq!(policy.try_clean(&html), refused, "{}", &html[..30]);
        }
        // Behind a comment, a tag the parser doesn't read as one counts too:
        // what the parser's state makes of `<` is not known at this point.
        let html = format!("<!-- <b{attributes}> -->");
        assert_eq!(policy.try_clean(&html), refused, "{}", &html[..30]);
    }
    let fine = many(250, &|i| format!(" a{i}"));
    assert!(policy.try_clean(&format!("<b{fine}>x</b>")).is_ok());
    // Text with many words between tags is not a tag.
    let words = "word ".repeat(10_000);
    assert_eq!(
        policy.try_clean(&format!("<p>{words}</p>")).unwrap(),
        format!("<p>{words}</p>")
    );
    let words = "a &lt; b ".repeat(1_000);
    assert!(policy.try_clean(&format!("<p>{words}</p>")).is_ok());
}

/// A long article is what the limits are there to leave alone: it reads the
/// same however the input is cut into pieces for the parser.
#[test]
fn a_long_post_reads_as_it_always_did() {
    let section = "<h2 dir=\"ltr\">A heading &amp; more</h2>\r\n\
        <p>Some <strong>bold</strong> text, <a href=\"https://x.example/a?b=1&amp;c=2\" title=\"t\">a link</a>,\
        an entity &eacute; &#233; &#xe9; and caf\u{e9} \u{4f60}\u{597d}.</p>\
        <ul><li>one</li><li>two <em>three</em></li></ul>\
        <table><thead><tr><th scope=\"col\">a</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table>\
        <blockquote><pre>\nkept\n  code</pre></blockquote>\n";
    let expected = HtmlPolicy::default()
        .limits(1 << 20, 1 << 20, 128)
        .try_clean(section)
        .unwrap();
    assert!(
        expected.contains("caf\u{e9} \u{4f60}\u{597d}"),
        "{expected}"
    );
    // 300 KB: every byte offset of a section falls on a cut somewhere.
    let times = 300 * 1024 / section.len();
    let html = section.repeat(times);
    let clean = HtmlPolicy::default().try_clean(&html).unwrap();
    let wanted = expected.repeat(times);
    assert_eq!(clean.len(), wanted.len());
    assert!(
        clean == wanted,
        "the output changes with the input's length"
    );
}

#[test]
fn what_a_policy_can_never_allow_is_removed_and_refused() {
    let policy = HtmlPolicy::default()
        .allow_tags(["iframe", "meta", "img"])
        .allow_attributes("iframe", ["srcdoc", "src"])
        .allow_attributes("meta", ["http-equiv", "content"])
        .allow_attributes("img", ["onerror", "src"]);
    assert!(policy.check().unwrap_err().contains("`srcdoc` on `iframe`"));
    assert_eq!(
        policy.clean(r#"<iframe srcdoc="<script>alert(1)</script>"></iframe>"#),
        "<iframe></iframe>"
    );
    assert_eq!(
        policy.clean(r#"<meta http-equiv="refresh" content="0;url=https://evil.test">"#),
        r#"<meta content="0;url=https://evil.test">"#
    );
    assert_eq!(
        policy.clean(r#"<img onerror="alert(1)" src="/a.png">"#),
        r#"<img src="/a.png">"#
    );
    for (generic, wanted) in [
        ("srcdoc", "generic_attributes: `srcdoc`"),
        ("onclick", "generic_attributes: `onclick`"),
    ] {
        let mut policy = HtmlPolicy::default();
        policy.generic_attributes.insert(generic.to_owned());
        assert!(policy.check().unwrap_err().contains(wanted), "{generic}");
    }
    let deep = HtmlPolicy::default().limits(1024, 1024, 257);
    assert!(
        deep.check()
            .unwrap_err()
            .starts_with("validation.html.max_depth")
    );
}

#[test]
fn every_link_gets_its_rel() {
    let policy = HtmlPolicy::default()
        .allow_tags(["area", "map"])
        .allow_attributes("area", ["href"]);
    assert_eq!(
        policy.clean(r#"<map><area href="https://x.example"></map>"#),
        r#"<map><area href="https://x.example" rel="noopener noreferrer nofollow"></map>"#
    );
}

/// Lists of URLs are read as lists.
#[test]
fn url_lists_are_checked_url_by_url() {
    let policy = HtmlPolicy::default()
        .allow_tags(["img"])
        .allow_attributes("img", ["srcset"])
        .allow_attributes("a", ["ping"])
        .url_schemes(["https"]);
    let kept = r#"<img srcset="https://x.example/a.png 1x, /b.png 2x">"#;
    assert_eq!(policy.clean(kept), kept);
    assert_eq!(
        policy.clean(r#"<img srcset="https://x.example/a.png 1x, http://evil.test/b.png 2x">"#),
        "<img>"
    );
    assert_eq!(
        policy.clean(r#"<a ping="https://x.example/a javascript:alert(1)" href="/x">a</a>"#),
        r#"<a href="/x" rel="noopener noreferrer nofollow">a</a>"#
    );
}

/// A URL with a host and no scheme leaves the site with the page's scheme.
#[test]
fn a_relative_url_with_a_host_needs_both_http_and_https() {
    let link = |policy: &HtmlPolicy, href: &str| {
        policy
            .clone()
            .link_rel("")
            .clean(&format!("<a href=\"{href}\">x</a>"))
            .contains("href")
    };
    let default = HtmlPolicy::default();
    let https = HtmlPolicy::default().url_schemes(["https"]);
    let none = HtmlPolicy::default().url_schemes::<0>([]);
    for href in [
        "//evil.test",
        "//evil.test/a",
        "\\\\evil.test",
        "/\\evil.test",
        "\\/evil.test",
        "///evil.test",
        "/\t/evil.test",
        " //evil.test",
    ] {
        assert!(link(&default, href), "{href}");
        assert!(!link(&https, href), "{href}");
        assert!(!link(&none, href), "{href}");
    }
    // What stays on the site stays for every policy.
    for href in ["/a", "a/b", "../a", "?q=1", "#top", "./a//b", ""] {
        assert!(link(&default, href), "{href}");
        assert!(link(&https, href), "{href}");
        assert!(link(&none, href), "{href}");
    }
}

/// Over a limit is the client's fault: a 400 on the field, with a message that
/// says why, and the rest of the request is not looked at.
#[tokio::test]
async fn a_field_over_a_limit_is_a_validation_error() {
    let app = app_with(HtmlTestModule).await;
    let body = app
        .client()
        .post("/html/comments")
        .json(&json!({ "body": "x".repeat(512 * 1024 + 1) }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(body["code"], "validation_failed");
    assert_eq!(
        errors(&body)["body"][0],
        "body must be shorter than or equal to 524288 bytes of HTML"
    );

    let body = app
        .client()
        .post("/html/comments")
        .json(&json!({ "body": "<ul>".repeat(200) }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(
        errors(&body)["body"][0],
        "body nests deeper than 128 levels"
    );

    let open: String = (0..100).map(|i| format!("<b a{i:x}>")).collect();
    let bomb = format!("<p>{open}{}", "<p>x".repeat(2000));
    let body = app
        .client()
        .post("/html/comments")
        .json(&json!({ "body": bomb }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(errors(&body)["body"][0], "body has too much markup");
}

/// Outside the extractors, the error says what it is too.
#[test]
fn a_field_over_a_limit_fails_a_plain_deserialize() {
    let long = serde_json::to_string(&"x".repeat(512 * 1024 + 1)).unwrap();
    let error = serde_json::from_str::<SafeHtml>(&long).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("the HTML is longer than 524288 bytes"),
        "{error}"
    );
}

#[derive(Debug, Deserialize, Validate)]
struct Article {
    #[validate(length(max = 20_000))]
    #[allow(dead_code)]
    body: SafeHtml,
}

fn article_request(body: String) -> http::Request<navree_http::body::Body> {
    http::Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", "application/json")
        .body(navree_http::body::Body::from(
            serde_json::to_vec(&json!({ "body": body })).unwrap(),
        ))
        .unwrap()
}

/// How long a task beside a request is kept waiting on a runtime of one
/// worker, and how often it got to run.
async fn beside_a_request(body: String) -> (std::time::Duration, u32) {
    use navree_http::extract::FromRequest;
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = done.clone();
    let ticker = tokio::spawn(async move {
        let mut largest = std::time::Duration::ZERO;
        let mut ticks = 0;
        while !flag.load(std::sync::atomic::Ordering::SeqCst) {
            let at = std::time::Instant::now();
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            largest = largest.max(at.elapsed());
            ticks += 1;
        }
        (largest, ticks)
    });
    // A task of its own, so that the ticker gets to start.
    tokio::task::yield_now().await;
    let request = tokio::spawn(async move {
        let _ = Valid::<Json<Article>>::from_request(article_request(body)).await;
        done.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    request.await.unwrap();
    ticker.await.unwrap()
}

/// `<ul>` repeated used to scan the whole stack for each tag, on the async
/// worker: 160 KB stopped everything else on it for seconds.
#[test]
fn a_hostile_field_does_not_stop_the_worker() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let (gap, _) = runtime.block_on(beside_a_request("<ul>".repeat(40_000)));
    assert!(gap.as_millis() < 1000, "the worker stopped for {gap:?}");
}

/// A long field within every limit goes on off the worker as well.
#[test]
fn a_long_field_gives_up_its_worker() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let html = "<p>Some <b>bold</b> text and <a href=\"/x\">a link</a></p>".repeat(8000);
    let (_, ticks) = runtime.block_on(beside_a_request(html));
    assert!(ticks >= 3, "the ticker ran {ticks} times");
}

/// Without another thread there is nowhere to go, and nothing to panic about.
#[tokio::test(flavor = "current_thread")]
async fn a_long_field_is_sanitized_on_a_runtime_of_one_thread() {
    let html = "<p>x</p>".repeat(4000);
    assert_eq!(HtmlPolicy::default().clean(&html), html);
}

/// For each `&` the serializer scans ahead for the next `<` or `>`, so a long
/// text or attribute full of them took as long as the square of its length:
/// 512 KiB took 14 s.
#[test]
fn a_long_text_of_ampersands_is_written_in_linear_time() {
    let policy = HtmlPolicy::default();
    let size = 512 * 1024;
    // At this length the square took 14 s in a release build, and much longer
    // in a test build.
    let started = std::time::Instant::now();
    let clean = policy.try_clean(&"&".repeat(size)).unwrap();
    assert_eq!(clean, "&amp;".repeat(size));
    let copyright = policy.try_clean(&"\u{a9}".repeat(size / 2)).unwrap();
    assert_eq!(copyright, "\u{a9}".repeat(size / 2));
    assert!(started.elapsed().as_secs() < 20, "{:?}", started.elapsed());

    // An attribute is charged for it instead.
    let html = format!("<a href=\"/{}\">x</a>", "&".repeat(size / 2));
    assert_eq!(
        policy.try_clean(&html),
        Err(HtmlError::TooManyNodes { max: 128 * 1024 })
    );
    let html = format!("<a href=\"/?{}\">x</a>", "a=1&".repeat(100));
    assert!(policy.try_clean(&html).is_ok());
    // A long value without them is only long.
    let html = format!("<a href=\"/{}\">x</a>", "a".repeat(400 * 1024));
    assert!(policy.try_clean(&html).is_ok());
}
