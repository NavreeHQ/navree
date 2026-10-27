//! The examples of `docs/basics/errors` through the facade, as an app writes
//! them: the invoices controller and its test, your own codes, the billing
//! error mapper, `http.errors.code`, and every item the page names (a rename
//! fails this file).

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use navree::axum::http::header::WWW_AUTHENTICATE;
use navree::axum::http::{HeaderValue, StatusCode};
use navree::http::{ErrorOptions, HttpOptions, codes};
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// "Returning errors": the invoices controller.

/// The page's invoices routes.
#[injectable]
pub struct InvoicesController;

#[controller("/invoices", version = "1")]
#[authenticated]
impl InvoicesController {
    #[post("/{id}/send")]
    async fn send(&self, Path(id): Path<Uuid>) -> Result<Json<Value>> {
        if id.is_nil() {
            return Err(AppError::not_found().with_detail("Invoice not found"));
        }
        if id.get_version_num() != 7 {
            return Err(AppError::conflict("The invoice has already been sent.")
                .with_code("invoice_already_sent"));
        }
        Ok(Json(json!({ "sent": true })))
    }

    #[post("/{id}/charge")]
    async fn charge(&self, Path(id): Path<Uuid>) -> Result<Json<Value>> {
        let receipt = charge_card(id).await?; // a BillingError
        Ok(Json(json!({ "receipt": receipt })))
    }
}

// ---------------------------------------------------------------------------
// "Your own codes": the campaigns route.

/// The campaigns the page's example asks (a stand-in: one is running).
#[injectable]
pub struct Campaigns;

impl Campaigns {
    async fn is_running(&self, id: Uuid) -> Result<bool> {
        Ok(id.is_nil())
    }

    async fn start(&self, _id: Uuid) -> Result<()> {
        Ok(())
    }
}

/// The page's campaigns route.
#[injectable]
pub struct CampaignsController {
    campaigns: Arc<Campaigns>,
}

#[controller("/campaigns", version = "1")]
#[authenticated]
impl CampaignsController {
    #[post("/{id}/start")]
    async fn start(&self, Path(id): Path<Uuid>) -> Result<NoContent> {
        if self.campaigns.is_running(id).await? {
            return Err(AppError::conflict("The campaign is already running.")
                .with_code("campaign_already_running"));
        }
        self.campaigns.start(id).await?;
        Ok(NoContent)
    }
}

// ---------------------------------------------------------------------------
// "Mapping your own errors": the billing module.

/// The page's billing error.
#[derive(Debug)]
pub enum BillingError {
    /// The card was declined.
    CardDeclined {
        /// Why.
        reason: String,
    },
    /// The invoice is paid already.
    InvoicePaid,
    /// The gateway failed.
    Gateway(std::io::Error),
}

impl fmt::Display for BillingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BillingError::CardDeclined { reason } => write!(f, "the card was declined: {reason}"),
            BillingError::InvoicePaid => f.write_str("the invoice is already paid"),
            BillingError::Gateway(error) => write!(f, "the payment gateway failed: {error}"),
        }
    }
}

impl std::error::Error for BillingError {}

fn billing_error(error: &BillingError) -> Option<AppError> {
    match error {
        BillingError::CardDeclined { reason } => Some(
            AppError::from_status(StatusCode::PAYMENT_REQUIRED)
                .with_code("card_declined")
                .with_detail(format!("The card was declined: {reason}.")),
        ),
        BillingError::InvoicePaid => {
            Some(AppError::conflict("The invoice is already paid.").with_code("invoice_paid"))
        }
        BillingError::Gateway(_) => None, // stays a 500
    }
}

/// Declines the nil id, finds the max id paid, and fails at the gateway
/// otherwise.
async fn charge_card(id: Uuid) -> std::result::Result<String, BillingError> {
    if id.is_nil() {
        Err(BillingError::CardDeclined {
            reason: "insufficient funds".to_owned(),
        })
    } else if id.is_max() {
        Err(BillingError::InvoicePaid)
    } else {
        Err(BillingError::Gateway(std::io::Error::other(
            "connection reset",
        )))
    }
}

#[module(
    controllers = [InvoicesController, CampaignsController],
    providers = [Campaigns],
    register = Self::errors
)]
/// The page's billing module.
pub struct BillingModule;

impl BillingModule {
    fn errors(&self, m: &mut ModuleBuilder) {
        m.map_error::<BillingError>(billing_error);
    }
}

async fn app() -> TestApp {
    TestApp::builder().module(BillingModule).build().await
}

// ---------------------------------------------------------------------------
// The page's responses.

#[tokio::test]
async fn invoices_answer_in_the_error_shape_with_codes() {
    let app = app().await;
    let client = app.acting_as(principal(Uuid::now_v7()));

    // "Testing", as written.
    client
        .post(&format!("/api/v1/invoices/{}/send", Uuid::nil()))
        .send()
        .await
        .assert_error(404, "Invoice not found")
        .assert_error_code(404, "not_found");
    client
        .get("/api/v1/nope")
        .send()
        .await
        .assert_error(404, None);

    let body = client
        .post(&format!("/api/v1/invoices/{}/send", Uuid::max()))
        .send()
        .await
        .assert_error(409, "The invoice has already been sent.")
        .json_value();
    assert_eq!(
        body,
        json!({
            "statusCode": 409,
            "message": "The invoice has already been sent.",
            "error": "Conflict",
            "code": "invoice_already_sent",
        })
    );
    client
        .post(&format!("/api/v1/invoices/{}/send", Uuid::now_v7()))
        .send()
        .await
        .assert_status(200);

    client
        .post(&format!("/api/v1/campaigns/{}/start", Uuid::nil()))
        .send()
        .await
        .assert_error(409, "The campaign is already running.")
        .assert_error_code(409, "campaign_already_running");
    client
        .post(&format!("/api/v1/campaigns/{}/start", Uuid::now_v7()))
        .send()
        .await
        .assert_status(204);

    app.client()
        .post(&format!("/api/v1/invoices/{}/send", Uuid::nil()))
        .send()
        .await
        .assert_error_code(401, "unauthenticated");
}

#[tokio::test]
async fn the_billing_mapper_sets_status_and_code() {
    let app = app().await;
    let client = app.acting_as(principal(Uuid::now_v7()));

    let body = client
        .post(&format!("/api/v1/invoices/{}/charge", Uuid::nil()))
        .send()
        .await
        .assert_error(402, "The card was declined: insufficient funds.")
        .json_value();
    assert_eq!(
        body,
        json!({
            "statusCode": 402,
            "message": "The card was declined: insufficient funds.",
            "error": "Payment Required",
            "code": "card_declined",
        })
    );
    client
        .post(&format!("/api/v1/invoices/{}/charge", Uuid::max()))
        .send()
        .await
        .assert_error_code(409, "invoice_paid");
    let gateway = client
        .post(&format!("/api/v1/invoices/{}/charge", Uuid::now_v7()))
        .send()
        .await
        .assert_error_code(500, "internal_error");
    assert!(
        !gateway.text().contains("connection reset"),
        "logged, not sent"
    );
}

#[tokio::test]
async fn http_errors_code_false_sends_three_members() {
    let app = TestApp::builder()
        .config("http.errors.code", false)
        .module(BillingModule)
        .build()
        .await;
    let body = app
        .acting_as(principal(Uuid::now_v7()))
        .post(&format!("/api/v1/invoices/{}/send", Uuid::nil()))
        .send()
        .await
        .assert_error(404, "Invoice not found")
        .json_value();
    assert_eq!(
        body,
        json!({"statusCode": 404, "message": "Invoice not found", "error": "Not Found"})
    );
}

#[test]
fn error_values_render_as_the_page_shows() {
    let error = AppError::new(
        StatusCode::UNAUTHORIZED,
        "invalid_signature",
        "Invalid signature.",
    )
    .with_header(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    assert_eq!(
        error.body(),
        json!({"statusCode": 401, "message": "Invalid signature.", "error": "Unauthorized", "code": "invalid_signature"})
    );

    let body = AppError::not_found().with_detail("Deal not found").body();
    assert_eq!(
        body,
        json!({"statusCode": 404, "message": "Deal not found", "error": "Not Found", "code": "not_found"})
    );
}

// ---------------------------------------------------------------------------
// Every item the page names.

#[test]
fn the_documented_api_exists() {
    let errors = BTreeMap::from([("email".to_owned(), vec!["taken".to_owned()])]);
    let constructors: Vec<(AppError, u16, &str)> = vec![
        (
            AppError::bad_request("The export is empty."),
            400,
            codes::BAD_REQUEST,
        ),
        (AppError::validation(errors), 400, codes::VALIDATION_FAILED),
        (
            AppError::invalid_filter("Unknown filter: colour."),
            400,
            codes::INVALID_FILTER,
        ),
        (AppError::unauthenticated(), 401, codes::UNAUTHENTICATED),
        (
            AppError::two_factor_required(),
            401,
            codes::TWO_FACTOR_REQUIRED,
        ),
        (AppError::forbidden(), 403, codes::FORBIDDEN),
        (AppError::demo_read_only(), 403, codes::DEMO_READ_ONLY),
        (AppError::csrf_mismatch(), 403, codes::CSRF_TOKEN_MISMATCH),
        (AppError::not_found(), 404, codes::NOT_FOUND),
        (AppError::conflict("..."), 409, codes::CONFLICT),
        (
            AppError::limit_reached("Your plan includes 5 seats."),
            409,
            codes::LIMIT_REACHED,
        ),
        (
            AppError::payload_too_large(16),
            413,
            codes::PAYLOAD_TOO_LARGE,
        ),
        (
            AppError::too_many_requests(Duration::from_secs(1)),
            429,
            codes::TOO_MANY_REQUESTS,
        ),
        (
            AppError::service_unavailable("..."),
            503,
            codes::SERVICE_UNAVAILABLE,
        ),
        (AppError::internal("boom"), 500, codes::INTERNAL_ERROR),
        (
            AppError::from_status(StatusCode::PAYMENT_REQUIRED),
            402,
            "payment_required",
        ),
    ];
    for (error, status, code) in constructors {
        assert_eq!(error.status.as_u16(), status, "{error}");
        assert_eq!(error.code, code, "{error}");
    }

    // The builder methods and the public fields.
    let error = AppError::forbidden()
        .with_detail("Only the owner can archive a project.")
        .with_code("project_archived")
        .with_header(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))
        .with_source(std::io::Error::other("cause"));
    let AppError {
        status,
        code,
        title,
        detail,
        errors,
        headers,
        source,
    } = error;
    assert_eq!(
        (status, code.as_str(), title.as_str(), detail.as_deref()),
        (
            StatusCode::FORBIDDEN,
            "project_archived",
            "Forbidden",
            Some("Only the owner can archive a project.")
        )
    );
    assert!(errors.is_none() && headers.contains_key(WWW_AUTHENTICATE) && source.is_some());

    // The codes module.
    for code in [
        codes::BAD_REQUEST,
        codes::VALIDATION_FAILED,
        codes::INVALID_FILTER,
        codes::UNAUTHENTICATED,
        codes::TWO_FACTOR_REQUIRED,
        codes::INVALID_SIGNATURE,
        codes::FORBIDDEN,
        codes::CSRF_TOKEN_MISMATCH,
        codes::DEMO_READ_ONLY,
        codes::NOT_FOUND,
        codes::METHOD_NOT_ALLOWED,
        codes::CONFLICT,
        codes::LIMIT_REACHED,
        codes::PAYLOAD_TOO_LARGE,
        codes::UNSUPPORTED_MEDIA_TYPE,
        codes::TOO_MANY_REQUESTS,
        codes::INTERNAL_ERROR,
        codes::SERVICE_UNAVAILABLE,
        codes::MAINTENANCE,
        codes::REQUEST_TIMEOUT,
    ] {
        assert!(
            codes::ALL.contains(&code) && codes::is_valid(code),
            "{code}"
        );
    }
    assert_eq!(codes::ALL.len(), 20, "the page's table lists every code");
    assert!(codes::is_valid("campaign_already_running"));

    // `http.errors.code`.
    let ErrorOptions { code } = ErrorOptions::default();
    assert!(code);
    let options = HttpOptions {
        errors: ErrorOptions { code: false },
        ..HttpOptions::default()
    };
    assert!(!options.errors.code);
}
