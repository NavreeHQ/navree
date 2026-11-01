//! `SafeHtml` (feature `html`): rich text sanitized by the app's
//! `HtmlPolicy` while deserializing, so the handler never sees a script.

use std::sync::Arc;

use http::Method;
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::TestApp;
use navree_validation::Validate;
use navree_validation::{HtmlPolicy, SafeHtml, Valid, ValidationModule};
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
    let clean = HtmlPolicy::default().clean(&html);
    assert_eq!(
        clean,
        format!("{}x{}", "<span>".repeat(depth), "</span>".repeat(depth))
    );
    assert_eq!(HtmlPolicy::text_only().clean(&html), "x");
}
