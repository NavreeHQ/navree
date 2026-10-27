//! The mailer's rendering: CSS inlining, text from HTML, `lang` and `dir`.

use std::sync::Arc;

use navree_mail::{ArrayTransport, Mailer, Message, RenderOptions, render};
use navree_view::Views;

fn mailer(options: RenderOptions) -> (Mailer, ArrayTransport) {
    let transport = ArrayTransport::new();
    let views = Arc::new(Views::from_templates([("x.txt", "x")]).expect("views"));
    let mailer =
        Mailer::new(Arc::new(transport.clone()), views, "app@example.com").with_render(options);
    (mailer, transport)
}

fn html_mail(html: &str) -> Message {
    Message::new()
        .to("ada@example.com")
        .subject("Hi")
        .html(html)
}

#[tokio::test]
async fn style_blocks_are_inlined_and_media_queries_kept() {
    let (mailer, transport) = mailer(RenderOptions::default());
    let html = "<html><head><style>p { color: red } .big { font-size: 20px } \
                @media (max-width: 600px) { p { color: blue } }</style></head>\
                <body><p class=\"big\">Hi</p></body></html>";

    mailer.send_message(html_mail(html)).await.expect("send");

    let sent = transport.sent().remove(0).html.expect("html");
    assert!(sent.contains(r#"style="color: red;font-size: 20px;""#), "{sent}");
    assert!(!sent.contains("p { color: red }"), "{sent}");
    assert!(sent.contains("@media (max-width: 600px)"), "{sent}");
}

#[tokio::test]
async fn html_without_a_style_block_is_untouched() {
    let (mailer, transport) = mailer(RenderOptions::default());

    mailer
        .send_message(html_mail("<p>Hi</p>").text("Hi"))
        .await
        .expect("send");

    let sent = transport.sent().remove(0);
    assert_eq!(sent.html.as_deref(), Some("<p>Hi</p>"));
    assert_eq!(sent.text.as_deref(), Some("Hi"));
}

#[tokio::test]
async fn a_text_body_is_generated_from_html_only_mail() {
    let (mailer, transport) = mailer(RenderOptions::default());

    mailer
        .send_message(html_mail(
            "<h1>Welcome</h1><p>Hi <b>Ada</b>,</p><p><a href=\"https://acme.test/start\">Get started</a></p>",
        ))
        .await
        .expect("send");

    let text = transport.sent().remove(0).text.expect("text");
    assert!(text.contains("Welcome"), "{text}");
    assert!(text.contains("Hi Ada,"), "{text}");
    assert!(text.contains("[Get started][1]"), "{text}");
    assert!(text.ends_with("[1]: https://acme.test/start"), "{text}");
}

#[tokio::test]
async fn rendering_can_be_turned_off() {
    let (mailer, transport) = mailer(RenderOptions {
        inline_css: false,
        text_from_html: false,
    });
    let html = "<style>p { color: red }</style><p>Hi</p>";

    mailer.send_message(html_mail(html)).await.expect("send");

    let sent = transport.sent().remove(0);
    assert_eq!(sent.html.as_deref(), Some(html));
    assert_eq!(sent.text, None);
}

#[tokio::test]
async fn the_locale_sets_lang_and_dir() {
    let (mailer, transport) = mailer(RenderOptions::default());

    mailer
        .send_message(html_mail("<html><body><p>مرحبا</p></body></html>").locale("ar"))
        .await
        .expect("send");
    mailer
        .send_message(html_mail("<p>Hallo</p>").locale("de_DE"))
        .await
        .expect("send");

    let sent = transport.sent();
    assert_eq!(
        sent[0].html.as_deref(),
        Some(r#"<html lang="ar" dir="rtl"><body><p>مرحبا</p></body></html>"#)
    );
    assert_eq!(
        sent[1].html.as_deref(),
        Some(r#"<div lang="de-DE" dir="ltr"><p>Hallo</p></div>"#)
    );
}

#[test]
fn apply_locale_keeps_existing_attributes() {
    assert_eq!(
        render::apply_locale(r#"<html lang="fr"><body></body></html>"#, "ar"),
        r#"<html dir="rtl" lang="fr"><body></body></html>"#
    );
    assert_eq!(
        render::apply_locale(r#"<HTML dir="ltr" lang="en">x</HTML>"#, "he"),
        r#"<HTML dir="ltr" lang="en">x</HTML>"#
    );
    // A locale with markup in it is cleaned.
    assert_eq!(
        render::apply_locale("<p>x</p>", "en\"><script>"),
        r#"<div lang="enscript" dir="ltr"><p>x</p></div>"#
    );
}

#[test]
fn right_to_left_languages() {
    for locale in ["ar", "ar-EG", "he", "fa_IR", "ur", "yi", "ckb"] {
        assert!(render::is_rtl(locale), "{locale}");
    }
    for locale in ["en", "fr-CA", "zh", "hi", "", "arn"] {
        assert!(!render::is_rtl(locale), "{locale}");
    }
}
