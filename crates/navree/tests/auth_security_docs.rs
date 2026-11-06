//! The examples of `docs/security/sessions`, `impersonation`,
//! `passwordless`, `account-flows` (two-factor policy) and
//! `authentication` (platform administrators), through the facade: the
//! controllers and the policy as written, booted and called.

use navree::access::{AccessModule, AccessOptions, AccessService};
use navree::auth::testing::VirtualAuthenticator;
use navree::auth::{
    AccessClaims, Actor, AuthError, AuthService, PlatformAdmin, SignInAttempt, SignInMethod,
    SignInPolicy, TwoFactorPolicy, User,
};
use navree::db::{Db, Executor as _, Statement};
use navree::prelude::*;
use navree::testing::{TestApp, TestAppBuilder};
use navree::{BoxError, BoxFuture};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Sessions and step-up: "Requiring a recent sign-in", "Reading the token's
// claims".

/// Account deletion: recent sign-in, never while impersonating.
#[injectable]
pub struct AccountController;

#[controller("/account", version = "1")]
#[authenticated]
impl AccountController {
    /// `DELETE /api/v1/account`: only within 10 minutes of signing in or
    /// confirming, and never while impersonating.
    #[delete("/")]
    #[recent_auth(minutes = 10)]
    #[not_while_impersonating]
    async fn destroy(&self, CurrentUser(user): CurrentUser) -> Json<String> {
        Json(format!("Deleting {}", user.email))
    }
}

/// The token's session claims.
#[injectable]
pub struct SessionInfoController;

#[controller("/session-info")]
impl SessionInfoController {
    #[get("/")]
    #[authenticated]
    async fn show(&self, claims: AccessClaims) -> Json<Value> {
        Json(json!({
            "session_id": claims.session_id(),
            "auth_time": claims.auth_time(),
            "impersonator": claims.impersonator(),
            "platform": claims.is_platform(),
        }))
    }
}

// ---------------------------------------------------------------------------
// Impersonation: `Actor` and "Guarding risky routes".

/// The impersonation banner.
#[injectable]
pub struct BannerController;

#[controller("/banner")]
impl BannerController {
    /// `GET /api/banner`: tells the front end to show the impersonation bar.
    #[get("/")]
    #[authenticated]
    async fn show(&self, actor: Actor) -> Json<Value> {
        Json(json!({
            "impersonating": actor.is_impersonating(),
            "user_id": actor.user_id,
            "real_user_id": actor.real_user_id(),
        }))
    }
}

/// Payment methods: never while impersonating.
#[injectable]
pub struct PaymentMethodController;

#[controller("/payment-methods", version = "1")]
#[authenticated]
#[not_while_impersonating]
impl PaymentMethodController {
    #[post("/")]
    async fn create(&self) -> &'static str {
        "added"
    }

    #[delete("/{id}")]
    async fn destroy(&self, Path(id): Path<u64>) -> String {
        format!("removed {id}")
    }
}

// ---------------------------------------------------------------------------
// Authentication: "Platform administrators".

/// The platform console.
#[injectable]
pub struct ConsoleController;

#[controller("/console")]
#[authenticated]
impl ConsoleController {
    /// `GET /api/console/whoami`: platform sessions only.
    #[get("/whoami")]
    async fn whoami(&self, PlatformAdmin(admin): PlatformAdmin) -> Json<String> {
        Json(format!("Signed in to the platform as {}", admin.email))
    }
}

// ---------------------------------------------------------------------------
// Account flows: "Workspaces and other rules".

/// Members of a workspace with `require_two_factor` must use two-factor
/// authentication.
#[injectable]
pub struct WorkspaceTwoFactorPolicy {
    db: Arc<Db>,
}

impl TwoFactorPolicy for WorkspaceTwoFactorPolicy {
    fn requires_two_factor<'a>(&'a self, user: &'a User) -> BoxFuture<'a, Result<bool, BoxError>> {
        Box::pin(async move {
            let row = self
                .db
                .fetch_optional(&Statement::portable(
                    "SELECT 1 FROM workspace_members m \
                     JOIN workspace_security s ON s.workspace_id = m.workspace_id \
                     WHERE m.user_id = ? AND s.require_two_factor",
                    [user.id.into()],
                ))
                .await?;
            Ok(row.is_some())
        })
    }
}

// ---------------------------------------------------------------------------
// Authentication: "Sign-in policies".

/// Contractors sign in with a passkey only.
#[injectable]
pub struct PasskeysForContractors;

impl SignInPolicy for PasskeysForContractors {
    fn check<'a>(&'a self, attempt: &'a SignInAttempt) -> BoxFuture<'a, Result<(), AuthError>> {
        Box::pin(async move {
            if attempt.domain() == Some("contractors.acme.com")
                && attempt.method != SignInMethod::Passkey
            {
                return Err(AuthError::SignInRefused {
                    code: "passkey_required".to_owned(),
                    message: "Sign in with your passkey".to_owned(),
                });
            }
            Ok(())
        })
    }
}

/// Registers the controllers above and the policies.
pub struct SecurityModule;

impl Module for SecurityModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.sign_in_policy::<PasskeysForContractors>()
            .two_factor_policy::<WorkspaceTwoFactorPolicy>()
            .controller::<AccountController>()
            .controller::<SessionInfoController>()
            .controller::<BannerController>()
            .controller::<PaymentMethodController>()
            .controller::<ConsoleController>();
    }
}

/// The tables the policy reads, as an app's own migration would create.
async fn create_policy_tables(app: &TestApp) {
    let db = app.get::<Db>().await;
    for sql in [
        "CREATE TABLE workspace_members (workspace_id INTEGER, user_id TEXT)",
        "CREATE TABLE workspace_security (workspace_id INTEGER, require_two_factor BOOLEAN)",
    ] {
        db.pool().execute_script(sql).await.unwrap();
    }
}

const PASSWORD: &str = "correct horse battery";

fn builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(SecurityModule)
}

async fn sign_in(app: &TestApp, email: &str) -> (User, String) {
    let user = app
        .get::<AuthService>()
        .await
        .create_user("Ada Lovelace", email, PASSWORD)
        .await
        .unwrap();
    let response = app
        .client()
        .post("/api/auth/login")
        .json(&json!({ "email": email, "password": PASSWORD }))
        .send()
        .await
        .assert_status(200);
    let access = response.json_value()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    (user, access)
}

#[tokio::test]
async fn the_sign_in_policy_example_refuses_passwords_for_contractors() {
    let app = builder().build().await;
    create_policy_tables(&app).await;
    let auth = app.get::<AuthService>().await;
    for email in ["kim@contractors.acme.com", "ada@acme.com"] {
        auth.create_user("Someone", email, PASSWORD).await.unwrap();
    }
    let login = |email: &'static str| {
        let client = app.client();
        async move {
            client
                .post("/api/auth/login")
                .json(&json!({ "email": email, "password": PASSWORD }))
                .send()
                .await
        }
    };
    login("kim@contractors.acme.com")
        .await
        .assert_error(403, "Sign in with your passkey")
        .assert_error_code(403, "passkey_required");
    login("ada@acme.com").await.assert_status(200);
}

#[tokio::test]
async fn the_session_and_impersonation_examples_work() {
    let app = builder()
        .config("auth.impersonation.enabled", true)
        .build()
        .await;
    create_policy_tables(&app).await;
    let (agent, agent_access) = sign_in(&app, "agent@example.com").await;
    let (ada, ada_access) = sign_in(&app, "ada@example.com").await;
    let client = app.client();

    client
        .delete("/api/v1/account")
        .bearer(&ada_access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("", json!("Deleting ada@example.com"));
    let info = client
        .get("/api/session-info")
        .bearer(&ada_access)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(info["session_id"].is_string());
    assert_eq!(info["platform"], false);

    // Seeding the support role, as the page shows.
    let db = app.get::<Db>().await;
    let access = app.get::<AccessService>().await;
    let uow = db.uow(None, None).await;
    access.create_role(&uow, "support", None).await.unwrap();
    access
        .grant(&uow, "support", "users.impersonate")
        .await
        .unwrap();
    access.assign_role(&uow, agent.id, "support").await.unwrap();
    uow.commit().await.unwrap();

    let response = client
        .post(&format!("/api/auth/impersonate/{}", ada.id))
        .bearer(&agent_access)
        .json(&json!({ "reason": "Ticket #4521" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("impersonator_id", json!(agent.id));
    let access_token = response.json_value()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    client
        .get("/api/banner")
        .bearer(&access_token)
        .send()
        .await
        .assert_json_path("impersonating", json!(true))
        .assert_json_path("real_user_id", json!(agent.id));
    for (method, path) in [
        ("POST", "/api/v1/payment-methods"),
        ("DELETE", "/api/v1/account"),
    ] {
        let request = match method {
            "POST" => client.post(path),
            _ => client.delete(path),
        };
        request
            .bearer(&access_token)
            .send()
            .await
            .assert_error(403, None);
    }
    client
        .post("/api/v1/payment-methods")
        .bearer(&ada_access)
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn the_workspace_policy_example_limits_members() {
    let app = builder().build().await;
    create_policy_tables(&app).await;
    let service = app.get::<AuthService>().await;
    let ada = service
        .create_user("Ada", "ada@example.com", PASSWORD)
        .await
        .unwrap();
    let db = app.get::<Db>().await;
    db.execute(&Statement::portable(
        "INSERT INTO workspace_members (workspace_id, user_id) VALUES (1, ?)",
        [ada.id.into()],
    ))
    .await
    .unwrap();
    assert!(!service.two_factor_required(&ada).await.unwrap());
    db.execute(&Statement::text(
        "INSERT INTO workspace_security (workspace_id, require_two_factor) VALUES (1, true)",
    ))
    .await
    .unwrap();
    assert!(service.two_factor_required(&ada).await.unwrap());
    app.client()
        .post("/api/auth/login")
        .json(&json!({ "email": "ada@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("two_factor_setup_required", json!(true));
}

#[tokio::test]
async fn the_console_example_needs_a_platform_session() {
    let app = builder()
        .config("auth.platform.enabled", true)
        .build()
        .await;
    create_policy_tables(&app).await;
    let (_, access) = sign_in(&app, "root@example.com").await;
    app.client()
        .get("/api/console/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_error(403, "A platform session is required");
}

#[tokio::test]
async fn the_passkey_testing_example_runs() {
    let app = builder()
        .config("auth.passkeys.enabled", true)
        .config("auth.passkeys.rp_id", "example.com")
        .config("auth.passkeys.origins", json!(["https://app.example.com"]))
        .build()
        .await;
    create_policy_tables(&app).await;
    let (_, access) = sign_in(&app, "ada@example.com").await;

    let authenticator = VirtualAuthenticator::new("example.com", "https://app.example.com");
    let client = app.client();

    let options = client
        .post("/api/auth/passkeys/register/options")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .json_value();
    let credential = authenticator.create(&options["publicKey"]);
    client
        .post("/api/auth/passkeys/register")
        .bearer(&access)
        .json(&json!({ "name": "Test key", "credential": credential }))
        .send()
        .await
        .assert_status(201);

    let options = client
        .post("/api/auth/passkeys/login/options")
        .json(&json!({}))
        .send()
        .await
        .json_value();
    let assertion = authenticator.get(&options["publicKey"]).expect("a passkey");
    client
        .post("/api/auth/passkeys/login")
        .json(&json!({ "credential": assertion }))
        .send()
        .await
        .assert_status(200);
}

// ---------------------------------------------------------------------------
// Two-factor: "Codes and QR codes in your own code", as the page writes it.

mod codes_in_your_own_code {
    use navree::crypto::qr::{EcLevel, QrCode};
    use navree::crypto::totp::{Secret, Totp};

    /// The base32 secret to type in, the `otpauth://` URL and the QR code as SVG.
    pub fn setup(
        issuer: &str,
        email: &str,
    ) -> Result<(String, String, String), Box<dyn std::error::Error>> {
        let totp = Totp::builder(Secret::generate()?)
            .issuer(issuer)
            .account(email)
            .build()?;
        let url = totp.to_url()?;
        let svg = QrCode::encode(url.as_bytes(), EcLevel::M)?.to_svg(200);
        Ok((totp.secret().to_base32(), url, svg))
    }

    /// Checks a code against the stored secret.
    pub fn accept(totp: &Totp, code: &str, now: u64, last_step: u64) -> Option<u64> {
        totp.check(code, now).filter(|step| *step > last_step)
    }
}

#[test]
fn the_totp_and_qr_example_sets_up_and_accepts_a_code_once() {
    use codes_in_your_own_code::{accept, setup};
    use navree::crypto::totp::{Secret, Totp};

    let (secret, url, svg) = setup("Acme", "ada@example.com").unwrap();
    assert!(url.starts_with("otpauth://totp/Acme:ada%40example.com?secret="));
    assert!(url.contains(&secret) && url.ends_with("&issuer=Acme"));
    assert!(svg.starts_with("<?xml") && svg.contains("<svg") && svg.contains("viewBox"));

    // The app has the secret from the URL; the server has it from storage.
    let totp = Totp::builder(Secret::from_base32(&secret).unwrap())
        .build()
        .unwrap();
    let from_url = Totp::from_url(&url).unwrap();
    let now = 1_792_929_600;
    let code = from_url.generate(now);
    assert_eq!(accept(&totp, &code, now, 0), Some(now / 30));
    // The same code again, once its step is the last one accepted.
    assert_eq!(accept(&totp, &code, now + 5, now / 30), None);
}
