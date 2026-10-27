//! The Rust examples of `packages/localization.mdx`, run against the
//! docs' Fluent files (`tests/fixtures/locales`). Every app in this file
//! has the same locales, because code outside a request uses the app
//! booted last.

use navree_i18n::{I18n, I18nModule, Locale, args, t};
use navree_testing::{TestApp, TestAppBuilder};
use navree_validation::ValidationModule;
use serde_json::json;

/// The docs' app configuration.
fn docs_app() -> TestAppBuilder {
    TestApp::builder()
        .config("i18n.locales", json!(["en", "fr", "de", "pt-BR"]))
        .config("i18n.dir", "tests/fixtures/locales")
        .config("i18n.missing", "error")
}

/// "The t! macro".
pub fn import_summary(created: usize) -> String {
    t!("deal-created", count = created)
}

#[tokio::test]
async fn translating_in_code() {
    let app = docs_app().module(I18nModule::from_config()).build().await;
    assert_eq!(import_summary(4), "4 deals created");

    let subject = Locale::scope("de", async {
        t!("deal-won", owner = "Ada", title = "Lovelace renewal")
    })
    .await;
    assert_eq!(subject, "Ada hat Lovelace renewal gewonnen.");

    // "The I18n service".
    let i18n = app.get::<I18n>().await;
    let text = i18n.t("pt-BR", "deal-created", args!(count = 2)).unwrap();
    assert_eq!(text, "2 negócios criados");
    assert_eq!(i18n.locales(), ["en", "fr", "de", "pt-BR"]);
    assert!(i18n.has("fr", "pipeline-empty"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn plurals_follow_the_language() {
    let app = docs_app().module(I18nModule::from_config()).build().await;
    Locale::scope("fr", async {
        assert_eq!(t!("deal-created", count = 1), "Une affaire créée");
        assert_eq!(t!("deal-created", count = 3), "3 affaires créées");
    })
    .await;
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn formatting_and_fluent_functions() {
    let app = docs_app().module(I18nModule::from_config()).build().await;
    let i18n = app.get::<I18n>().await;
    let money = navree_i18n::Money::new(4_200_000, "EUR");
    assert_eq!(
        i18n.t("en", "deal-amount", args!(amount = money.clone()))
            .unwrap(),
        "€42,000.00"
    );
    assert_eq!(
        i18n.t("fr", "deal-amount", args!(amount = money)).unwrap(),
        "42\u{202f}000,00\u{a0}€"
    );
    let closes = chrono::NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
    assert_eq!(
        i18n.t("fr", "deal-closes", args!(date = closes)).unwrap(),
        "Clôture le 26 oct. 2026"
    );
    assert_eq!(
        i18n.t("en", "deal-closes-long", args!(date = closes))
            .unwrap(),
        "Closes October 26, 2026"
    );
    assert_eq!(
        i18n.t("en", "deal-total", args!(total = 42000)).unwrap(),
        "Total: €42,000.00"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn validation_errors_are_in_the_requested_language() {
    use garde::Validate;
    use navree_http::Json;
    use navree_macros::{controller, injectable, module};
    use navree_validation::Valid;
    use serde::Deserialize;

    #[derive(Deserialize, Validate)]
    struct NewContact {
        #[garde(length(chars, min = 1))]
        first_name: String,
        #[garde(length(chars, min = 1))]
        last_name: String,
    }

    #[injectable]
    struct ContactsController;

    #[controller("/contacts", version = "1")]
    impl ContactsController {
        #[post("/")]
        #[public]
        async fn create(&self, Valid(Json(contact)): Valid<Json<NewContact>>) -> String {
            format!("{} {}", contact.first_name, contact.last_name)
        }
    }

    #[module(controllers = [ContactsController])]
    struct ContactsModule;

    let app = docs_app()
        .module(I18nModule::from_config())
        .module(ValidationModule::from_config())
        .module(ContactsModule)
        .build()
        .await;

    app.client()
        .post("/api/v1/contacts")
        .header("Accept-Language", "fr-FR,fr;q=0.9")
        .json(&json!({ "last_name": "Lovelace" }))
        .send()
        .await
        .assert_validation_error("first_name")
        .assert_json_path("errors.first_name.0", json!("prénom ne doit pas être vide"));
    app.shutdown().await.unwrap();
}
