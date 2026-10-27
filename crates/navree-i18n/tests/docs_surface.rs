//! Names every public item `packages/localization.mdx` documents, so a
//! rename fails the build: the module, the configuration keys, the macros,
//! `Locale`, `I18n`, the routes' types and the commands.

use std::path::PathBuf;

use chrono::NaiveDate;
use navree_i18n::{
    Arg, Args, BidiIsolation, DateStyle, I18N_MIGRATIONS, I18n, I18nController, I18nError,
    I18nModule, I18nOptions, IntoLocale, Locale, LocaleFiles, LocaleInput, LocaleListResource,
    LocaleResource, Missing, Money, Source, args, embed_locales, t,
};
use navree_testing::TestApp;
use serde_json::json;

#[test]
fn configuration_keys() {
    let I18nOptions {
        default_locale,
        locales,
        dir,
        negotiation,
        query_param,
        cookie,
        missing,
        reload,
        bidi_isolation,
        routes,
        files,
    } = I18nOptions::default();
    assert_eq!(default_locale, "en");
    assert_eq!(locales, ["en"]);
    assert_eq!(dir, PathBuf::from("locales"));
    assert_eq!(negotiation, [Source::User, Source::Workspace, Source::Header]);
    assert_eq!(query_param, "locale");
    assert_eq!(cookie, "locale");
    assert_eq!(missing, Missing::Warn);
    assert!(!reload);
    assert_eq!(bidi_isolation, BidiIsolation::Auto);
    assert!(routes);
    assert!(files.embedded().is_none());
    for source in [
        Source::Query,
        Source::User,
        Source::Workspace,
        Source::Cookie,
        Source::Header,
    ] {
        let parsed: Source = serde_json::from_value(json!(source.as_str())).unwrap();
        assert_eq!(parsed, source);
    }
    for (name, missing) in [("warn", Missing::Warn), ("error", Missing::Error), ("id", Missing::Id)] {
        assert_eq!(serde_json::from_value::<Missing>(json!(name)).unwrap(), missing);
    }
    let _: LocaleFiles = embed_locales!("tests/fixtures/locales");
    assert_eq!(I18N_MIGRATIONS, "i18n");
}

#[tokio::test]
async fn locale_and_i18n_methods() {
    let i18n = I18n::new(I18nOptions {
        locales: vec!["en".into(), "fr".into()],
        dir: "tests/fixtures/locales".into(),
        ..I18nOptions::default()
    })
    .unwrap();
    let _: Result<String, I18nError> = i18n.t("fr", "deal-created", args!(count = 2));
    let _: bool = i18n.has("fr", "pipeline-empty");
    let _: Vec<String> = i18n.locales();
    let _ = (I18n::user_locale, I18n::set_user_locale);
    let _ = (I18n::workspace_locale, I18n::set_workspace_locale);
    let _ = (I18n::reload, I18n::missing, I18n::default_locale, I18n::matching);

    let locale: Locale = i18n.locale("fr");
    let day = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
    let _: (&str, &str) = (locale.tag(), locale.language());
    let _: String = locale.format_money(4_200_000, "EUR");
    let _: String = locale.format_date(day);
    let _: String = locale.format_date_with(day, DateStyle::Long);
    let _: String = locale.format_number(1234.5);
    let _: Locale = Locale::current();
    let _: String = Locale::scope(locale.clone().into_locale(), async { t!("deal-created", count = 1) }).await;
    let _: Arg = Money::new(1, "EUR").into();
    let _: Args = Args::new().with("count", 1);
}

#[tokio::test]
async fn routes_and_commands() {
    let _ = (
        std::any::type_name::<I18nController>(),
        std::any::type_name::<LocaleResource>(),
        std::any::type_name::<LocaleListResource>(),
        std::any::type_name::<LocaleInput>(),
    );
    let app = TestApp::builder()
        .config("i18n.dir", "tests/fixtures/locales")
        .module(I18nModule::from_config())
        .build()
        .await;
    let commands: Vec<&str> = app.ctx().commands().iter().map(|command| command.name).collect();
    for name in ["i18n:check", "i18n:missing", "i18n:publish"] {
        assert!(commands.contains(&name), "{commands:?}");
    }
    app.client()
        .get("/api/i18n/locales")
        .send()
        .await
        .assert_status(200);
    app.shutdown().await.unwrap();
}
