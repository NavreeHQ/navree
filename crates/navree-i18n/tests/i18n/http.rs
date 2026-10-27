//! Negotiating a request's locale: `Accept-Language`, the query parameter
//! and the cookie, the `Locale` extractor, `Content-Language`, and the
//! locales route.

use serde_json::json;

use crate::support;

#[tokio::test]
async fn accept_language_picks_the_locale_of_the_request() {
    let app = support::builder().build().await;
    let response = app
        .client()
        .get("/api/deals/summary")
        .header("Accept-Language", "fr-CA;q=0.9, en;q=0.8")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("locale", json!("fr"))
        .assert_json_path("headline", json!("3 affaires créées"))
        .assert_json_path("total", json!("42\u{202f}000,00\u{a0}€"))
        .assert_json_path("updated", json!("26 oct. 2026"));
    assert_eq!(response.header("content-language"), Some("fr"));

    // Unsupported languages are skipped; `pt` matches `pt-BR`.
    app.client()
        .get("/api/deals/summary")
        .header("Accept-Language", "ja, pt;q=0.5")
        .send()
        .await
        .assert_json_path("locale", json!("pt-BR"));

    // Nothing supported: the default locale.
    let response = app
        .client()
        .get("/api/deals/summary")
        .header("Accept-Language", "ja")
        .send()
        .await
        .assert_json_path("locale", json!("en"))
        .assert_json_path("headline", json!("3 deals created"))
        .assert_json_path("total", json!("€42,000.00"))
        .assert_json_path("updated", json!("Oct 26, 2026"));
    assert_eq!(response.header("content-language"), Some("en"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_sources_run_in_the_configured_order() {
    let app = support::builder()
        .config("i18n.negotiation", json!(["query", "cookie", "header"]))
        .build()
        .await;
    let locale = |path: &str, cookie: Option<&str>, header: Option<&str>| {
        let mut request = app.client().get(path);
        if let Some(cookie) = cookie {
            request = request.header("Cookie", cookie);
        }
        if let Some(header) = header {
            request = request.header("Accept-Language", header);
        }
        async move { request.send().await.json_value()["locale"].clone() }
    };
    assert_eq!(locale("/api/deals/summary?locale=de", Some("locale=fr"), Some("pt")).await, "de");
    assert_eq!(locale("/api/deals/summary", Some("theme=dark; locale=fr"), Some("pt")).await, "fr");
    assert_eq!(locale("/api/deals/summary", None, Some("pt")).await, "pt-BR");
    // An unsupported value is skipped, not an error.
    assert_eq!(locale("/api/deals/summary?locale=xx", Some("locale=de"), None).await, "de");
    app.shutdown().await.unwrap();

    // The default sources (user, workspace, header) ignore the query.
    let app = support::builder()
        .config("i18n.query_param", "lang")
        .build()
        .await;
    app.client()
        .get("/api/deals/summary?lang=de")
        .send()
        .await
        .assert_json_path("locale", json!("en"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_cookie_and_query_names_are_configurable() {
    let app = support::builder()
        .config("i18n.negotiation", "query,cookie")
        .config("i18n.query_param", "lang")
        .config("i18n.cookie", "ui_locale")
        .build()
        .await;
    app.client()
        .get("/api/deals/summary?lang=de")
        .send()
        .await
        .assert_json_path("locale", json!("de"));
    app.client()
        .get("/api/deals/summary")
        .header("Cookie", "ui_locale=fr")
        .send()
        .await
        .assert_json_path("locale", json!("fr"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_locales_route_lists_native_names() {
    let app = support::builder().build().await;
    app.client()
        .get("/api/i18n/locales")
        .send()
        .await
        .assert_status(200)
        .assert_json_path(
            "data",
            json!([
                { "tag": "en", "name": "English", "direction": "ltr", "default": true },
                { "tag": "fr", "name": "Français", "direction": "ltr", "default": false },
                { "tag": "de", "name": "Deutsch", "direction": "ltr", "default": false },
                { "tag": "pt-BR", "name": "Português (Brasil)", "direction": "ltr", "default": false },
            ]),
        );
    // Without a database the user's locale cannot be saved: no route.
    app.acting_as(navree_testing::principal(uuid::Uuid::now_v7()))
        .put("/api/me/locale")
        .json(&json!({ "locale": "fr" }))
        .send()
        .await
        .assert_status(404);
    app.shutdown().await.unwrap();

    let app = support::builder().config("i18n.routes", false).build().await;
    app.client()
        .get("/api/i18n/locales")
        .send()
        .await
        .assert_status(404);
    app.shutdown().await.unwrap();
}
