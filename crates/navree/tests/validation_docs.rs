//! The "International input" examples of `content/docs/basics/validation.mdx`,
//! written as an app writes them (the facade, the prelude and the macros),
//! compiled and run.
#![cfg(all(feature = "validation-phone", feature = "validation-html"))]

use navree::BoxFuture;
use navree::axum::http::request::Parts;
use navree::prelude::*;
use navree::testing::TestApp;
use navree::validation::Validate;
use navree::validation::{
    Country, Folded, HtmlPolicy, InputContext, Locale, Phone, PhoneOptions, RegionSource, SafeHtml,
    ValidationOptions, fold_digits, rules,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

/// A contact: every international type at once.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateContact {
    /// A name in any script.
    #[validate(length(min = 1, max = 100), custom(rules::unicode_name))]
    pub name: String,
    /// E.164 once deserialized.
    #[validate(skip)]
    pub phone: Phone,
    /// Optional.
    #[validate(skip)]
    pub mobile: Option<Phone>,
    /// Upper case once deserialized.
    #[validate(skip)]
    pub country: Country,
    /// Canonical case once deserialized.
    #[validate(skip)]
    pub locale: Option<Locale>,
    /// Sanitized once deserialized.
    #[validate(length(max = 20_000))]
    pub notes: SafeHtml,
}

/// A one-time code typed with any digits.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct VerifyCode {
    /// Six digits.
    #[validate(length(min = 6, max = 6), pattern(r"^[0-9]+$"))]
    pub code: Folded<String>, // "٦٥٤٣٢١" arrives as "654321"
    /// A small number.
    #[validate(range(min = 1, max = 99))]
    pub quantity: Option<Folded<u32>>, // "٤٢" and 42 are both 42
}

/// Rich text.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct NewComment {
    /// Measured after sanitizing.
    #[validate(length(min = 1, max = 20_000))]
    pub body: SafeHtml,
}

/// The OpenAPI note: the schema of what the field serializes as.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct DocumentedContact {
    /// A string in the document.
    #[validate(skip)]
    #[schema(value_type = String)]
    pub phone: Phone,
}

/// The country the edge proxy reports in `CF-IPCountry`.
#[injectable]
pub struct EdgeCountry;

impl RegionSource for EdgeCountry {
    fn region<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Option<Country>> {
        let country = parts
            .headers
            .get("cf-ipcountry")
            .and_then(|value| value.to_str().ok())
            .and_then(|code| Country::parse(code).ok());
        Box::pin(async move { country })
    }
}

/// Echoes the validated input.
#[injectable]
pub struct ContactsController;

#[controller("/contacts", version = "1")]
impl ContactsController {
    #[post("/")]
    #[public]
    async fn store(&self, Valid(Json(input)): Valid<Json<CreateContact>>) -> Json<CreateContact> {
        Json(input)
    }

    #[post("/verify")]
    #[public]
    async fn verify(&self, Valid(Json(input)): Valid<Json<VerifyCode>>) -> Json<VerifyCode> {
        Json(input)
    }

    #[post("/comments")]
    #[public]
    async fn comment(&self, Valid(Json(input)): Valid<Json<NewComment>>) -> Json<NewComment> {
        Json(input)
    }
}

/// The contacts routes and the region source.
#[module(controllers = [ContactsController], register = Self::wire)]
pub struct ContactsModule;

impl ContactsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.region_source::<EdgeCountry>();
    }
}

async fn app(region: &str) -> TestApp {
    TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config(
            "validation",
            json!({ "phone": { "default_region": region } }),
        )
        .module(ValidationModule::from_config())
        .module(ContactsModule)
        .build()
        .await
}

#[tokio::test]
async fn a_contact_from_cairo() {
    let app = app("EG").await;
    let body = app
        .client()
        .post("/api/v1/contacts")
        .json(&json!({
            "name": "مُحَمَّد عبد الله",
            "phone": "٠١٠٠ ١٢٣ ٤٥٦٧",
            "country": "eg",
            "locale": "ar-eg",
            "notes": "<p dir=\"rtl\">عميل جديد</p><script>steal()</script>",
        }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body,
        json!({
            "name": "مُحَمَّد عبد الله",
            "phone": "+201001234567",
            "mobile": null,
            "country": "EG",
            "locale": "ar-EG",
            "notes": "<p dir=\"rtl\">عميل جديد</p>",
        })
    );
}

#[tokio::test]
async fn the_region_source_wins_over_the_configured_region() {
    let app = app("EG").await;
    let contact =
        |phone: &str| json!({ "name": "Ada", "phone": phone, "country": "GB", "notes": "" });
    app.client()
        .post("/api/v1/contacts")
        .header("cf-ipcountry", "GB")
        .json(&contact("07400 123456"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("phone", json!("+447400123456"));
    app.client()
        .post("/api/v1/contacts")
        .json(&contact("07400 123456"))
        .send()
        .await
        .assert_validation_error("phone")
        .assert_error(400, "phone must be a valid phone number");
}

#[tokio::test]
async fn codes_and_comments() {
    let app = app("GB").await;
    app.client()
        .post("/api/v1/contacts/verify")
        .json(&json!({ "code": "٦٥٤٣٢١", "quantity": "٤٢" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("code", json!("654321"))
        .assert_json_path("quantity", json!(42));
    app.client()
        .post("/api/v1/contacts/comments")
        .json(&json!({ "body": "<script>alert(1)</script>" }))
        .send()
        .await
        .assert_error(400, "body must be longer than or equal to 1 characters");
    app.client()
        .post("/api/v1/contacts/comments")
        .json(&json!({ "body": "<!-- note --><u>hi</u>" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("body", json!("<u>hi</u>"));
}

#[tokio::test]
async fn outside_the_extractors() {
    #[derive(Debug, Deserialize)]
    struct ImportRow {
        phone: Phone,
    }

    let app = app("GB").await;
    let ctx = app.ctx();
    let line = r#"{"phone": "07400 123456"}"#;
    let context = ctx.get::<InputContext>().await.unwrap();
    let row: ImportRow = context.scope(|| serde_json::from_str(line)).unwrap();
    assert_eq!(row.phone.as_str(), "+447400123456");
    // Without the scope, no default region.
    assert!(serde_json::from_str::<ImportRow>(line).is_err());

    let by_hand = InputContext::new()
        .with_default_region(Country::parse("GB").ok())
        .with_html(HtmlPolicy::text_only());
    let html: SafeHtml = by_hand
        .scope(|| serde_json::from_str(r#""<b>1 < 2</b>""#))
        .unwrap();
    assert_eq!(html.as_str(), "1 &lt; 2");
}

#[test]
fn the_types_and_builders_of_the_page() {
    let options = ValidationOptions {
        phone: PhoneOptions {
            default_region: Country::parse("GB").ok(),
        },
        html: HtmlPolicy::default()
            .allow_tags(["img"])
            .allow_attributes("img", ["src", "alt"])
            .url_schemes(["https", "mailto"])
            .link_rel("noopener"),
        ..ValidationOptions::default()
    };
    assert_eq!(options.html.check(), Ok(()));
    let _module = ValidationModule::with_options(options);

    let phone = Phone::parse("+44 7400 123456 ext. 12", None).unwrap();
    assert_eq!(
        (
            phone.as_str(),
            phone.country_code(),
            phone.national_number()
        ),
        ("+447400123456", 44, 7_400_123_456)
    );
    assert_eq!(phone.region(), Country::parse("GB").ok());
    assert_eq!(phone.international(), "+44 7400 123456 x12");
    assert_eq!(phone.extension(), Some("12"));
    assert!("+44 7400 123456".parse::<Phone>().is_ok());

    let locale = Locale::parse("zh-hant-tw").unwrap();
    assert_eq!(
        (
            locale.as_str(),
            locale.language(),
            locale.script(),
            locale.region()
        ),
        ("zh-Hant-TW", "zh", Some("Hant"), Some("TW"))
    );
    assert_eq!(Locale::parse("es-419").unwrap().region(), Some("419"));
    assert_eq!(Country::all().count(), 249);
    assert_eq!(fold_digits("०१२३४५६७८९"), "0123456789");
    assert_eq!(
        SafeHtml::new("<a href=\"https://example.com\">x</a>").as_str(),
        "<a href=\"https://example.com\" rel=\"noopener noreferrer nofollow\">x</a>"
    );
}
