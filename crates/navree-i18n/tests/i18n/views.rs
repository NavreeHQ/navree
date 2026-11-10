//! Templates and mail in the current locale.

use navree_i18n::{I18n, t};
use navree_kernel::{Module, ModuleBuilder};
use navree_mail::{MailModule, MailOptions, Mailer, TemplateMail, TransportConfig};
use navree_testing::TestAppBuilder;
use navree_view::{ModuleBuilderViewExt, ViewModule, ViewOptions, Views};
use serde_json::json;

use crate::support;

/// The docs' deal-won mail and a page using every helper.
struct Templates;

impl Module for Templates {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[
            (
                "mail/deal-won.html",
                "<h1>{{ t(\"deal-won\", owner=owner.name, title=deal.title) }}</h1>\
                 <p>{{ deal.amount_cents | money(deal.currency) }}</p>\
                 <p>{{ t(\"mail-footer\") }}</p>",
            ),
            (
                "mail/deal-won.txt",
                "{{ t(\"deal-won\", owner=owner.name, title=deal.title) }}",
            ),
            (
                "page.txt",
                "<html lang=\"{{ locale() }}\">{{ t(\"deal-created\", count=count) }} | \
                 {{ \"2026-10-26T14:30:00Z\" | date }} | \
                 {{ \"2026-10-26T14:30:00Z\" | date(\"%d/%m/%Y\") }} | \
                 {{ none | money(\"EUR\") }}</html>",
            ),
            ("broken.html", "{{ t(\"no-such-message\") }}"),
        ]);
    }
}

fn builder() -> TestAppBuilder {
    support::builder()
        .module(ViewModule::for_root(ViewOptions {
            dirs: vec![],
            reload: false,
        }))
        .module(Templates)
        .module(MailModule::for_root(MailOptions {
            from: "Navree <noreply@navree.test>".to_owned(),
            transport: TransportConfig::Array,
            ..MailOptions::default()
        }))
}

#[tokio::test]
async fn templates_translate_and_format_in_the_current_locale() {
    let app = builder().build().await;
    let i18n = app.get::<I18n>().await;
    let views = app.get::<Views>().await;
    let render = |locale: &'static str| {
        let views = views.clone();
        i18n.scope(locale, async move {
            views.render("page", json!({ "count": 2 })).unwrap()
        })
    };
    assert_eq!(
        render("fr").await,
        "<html lang=\"fr\">2 affaires créées | 26 oct. 2026 | 26/10/2026 | </html>"
    );
    assert_eq!(
        render("en").await,
        "<html lang=\"en\">2 deals created | Oct 26, 2026 | 26/10/2026 | </html>"
    );
    // A missing message is a render error with `i18n.missing = "error"`.
    let views = views.clone();
    let error = i18n
        .scope("fr", async move {
            views.render("broken", json!({})).unwrap_err()
        })
        .await;
    assert!(error.to_string().contains("no-such-message"), "{error}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn mail_renders_in_the_locale_it_is_sent_in() {
    let app = builder().build().await;
    let i18n = app.get::<I18n>().await;
    let mailer = app.get::<Mailer>().await;
    let context = json!({
        "owner": { "name": "Anu" },
        "deal": { "title": "Lakhotia renewal", "amount_cents": 4_200_000, "currency": "EUR" },
    });
    for locale in ["de", "fr"] {
        let mailer = mailer.clone();
        let context = context.clone();
        i18n.scope(locale, async move {
            let subject = t!("deal-won-subject", title = "Lakhotia renewal");
            let mail =
                TemplateMail::new("anu@navree.test", subject, "mail/deal-won").context(context);
            mailer.queue(&mail).await.unwrap();
        })
        .await;
    }
    let sent = mailer.array_transport().unwrap().sent();
    assert_eq!(sent[0].subject, "Gewonnen: Lakhotia renewal");
    assert_eq!(
        sent[0].text.as_deref(),
        Some("Anu hat Lakhotia renewal gewonnen.")
    );
    let html = sent[0].html.as_deref().unwrap();
    assert!(html.contains("42.000,00\u{a0}€"), "{html}");
    // `mail-footer` is not in German: it falls back to English.
    assert!(html.contains("Sent by Agra CRM."), "{html}");
    assert_eq!(sent[1].subject, "Affaire gagnée : Lakhotia renewal");
    assert!(
        sent[1]
            .html
            .as_deref()
            .unwrap()
            .contains("Envoyé par Agra CRM.")
    );
    app.shutdown().await.unwrap();
}
