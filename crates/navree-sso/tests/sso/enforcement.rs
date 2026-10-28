//! Enforcement: a verified domain of a connection that enforces single
//! sign-on signs in through that connection only. Every other way to a
//! session (password, magic link, emailed code, passkey, token exchange, a
//! two-factor challenge started before enforcement, another connection) is
//! refused with 403 `sso_required`; domains that don't enforce are
//! unaffected.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use navree_auth::testing::VirtualAuthenticator;
use navree_auth::{AuthService, ClientMeta, User};
use navree_mail::{MailModule, MailOptions, Mailer, TransportConfig};
use navree_sso::{Provisioning, SsoConnection, SsoFake, SsoOwner};
use navree_testing::{TestAppBuilder, TestResponse};
use navree_view::{ViewModule, ViewOptions};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};

use crate::support::{self, PASSWORD, World};

const RP_ID: &str = "acme.test";
const ORIGIN: &str = "https://app.acme.test";
const ISSUER: &str = "https://legacy.acme.test";

/// The enforcing world's builder: every sign-in method on, mail recorded,
/// and `sso.enforce_for_verified_domains`.
fn builder(issuer: &Issuer) -> TestAppBuilder {
    support::builder()
        .config("sso.enforce_for_verified_domains", true)
        .config("auth.magic_link.enabled", true)
        .config("auth.magic_link.url", "https://app.acme.test/magic")
        .config("auth.otp.enabled", true)
        .config("auth.passkeys.enabled", true)
        .config("auth.passkeys.rp_id", RP_ID)
        .config("auth.passkeys.origins", json!([ORIGIN]))
        .config("auth.trusted_issuers.legacy.issuer", ISSUER)
        .config("auth.trusted_issuers.legacy.audience", "navree-app")
        .config("auth.trusted_issuers.legacy.public_key", issuer.pem())
        .module(ViewModule::for_root(ViewOptions {
            dirs: vec![],
            reload: false,
        }))
        .module(MailModule::for_root(MailOptions {
            transport: TransportConfig::Array,
            ..MailOptions::default()
        }))
}

/// A world with the `acme` connection (domain `acme.test` not verified
/// yet), Ann of `acme.test` and Ola of `other.test`.
struct Enforced {
    world: World,
    fake: SsoFake,
    connection: SsoConnection,
    issuer: Issuer,
    ann: User,
    ola: User,
}

impl Enforced {
    async fn new() -> Self {
        let issuer = Issuer::new();
        let world = support::world_with(builder(&issuer)).await;
        let fake = SsoFake::new();
        let connection = world
            .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
            .await;
        let ann = world.create_verified_user("Ann", "ann@acme.test").await;
        let ola = world.create_verified_user("Ola", "ola@other.test").await;
        Self {
            world,
            fake,
            connection,
            issuer,
            ann,
            ola,
        }
    }

    /// Verifies `acme.test`: from now on the connection enforces it.
    async fn enforce(&self) {
        self.world
            .verified_domain(&self.connection, "acme.test")
            .await;
    }

    async fn post(&self, path: &str, body: Value) -> TestResponse {
        self.world.app.client().post(path).json(&body).send().await
    }

    /// The text body of the last mail.
    async fn last_mail(&self) -> String {
        self.world
            .app
            .get::<Mailer>()
            .await
            .array_transport()
            .expect("the array transport")
            .sent()
            .last()
            .expect("a mail")
            .text
            .clone()
            .expect("a text body")
    }

    async fn mail_count(&self) -> usize {
        self.world
            .app
            .get::<Mailer>()
            .await
            .array_transport()
            .expect("the array transport")
            .sent()
            .len()
    }

    /// The magic link token mailed for `email`.
    async fn magic_link(&self, email: &str) -> String {
        self.post("/api/auth/magic-link", json!({ "email": email }))
            .await
            .assert_status(202);
        token_in(&self.last_mail().await)
    }

    /// The sign-in code mailed for `email`.
    async fn login_code(&self, email: &str) -> String {
        self.post("/api/auth/otp", json!({ "email": email }))
            .await
            .assert_status(202);
        code_in(&self.last_mail().await)
    }

    /// Registers a passkey for `user` (signed in before enforcement).
    async fn passkey(&self, user: &User) -> VirtualAuthenticator {
        let access = self.world.token(user).await;
        let client = self.world.app.client();
        let authenticator = VirtualAuthenticator::new(RP_ID, ORIGIN);
        let options = client
            .post("/api/auth/passkeys/register/options")
            .bearer(&access)
            .json(&json!({}))
            .send()
            .await
            .assert_status(200)
            .json_value();
        let credential = authenticator.create(&options["publicKey"]);
        client
            .post("/api/auth/passkeys/register")
            .bearer(&access)
            .json(&json!({ "name": "Laptop", "credential": credential }))
            .send()
            .await
            .assert_status(201);
        authenticator
    }

    /// A passkey sign-in with `authenticator` for `email`.
    async fn passkey_login(
        &self,
        authenticator: &VirtualAuthenticator,
        email: &str,
    ) -> TestResponse {
        let options = self
            .post(
                "/api/auth/passkeys/login/options",
                json!({ "email": email }),
            )
            .await
            .assert_status(200)
            .json_value();
        let assertion = authenticator.get(&options["publicKey"]).expect("a passkey");
        self.post(
            "/api/auth/passkeys/login",
            json!({ "credential": assertion }),
        )
        .await
    }
}

/// The `token` query parameter of the first link in `text`.
fn token_in(text: &str) -> String {
    let start = text.find("token=").expect("a magic link") + "token=".len();
    text[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '&' && *c != '"' && *c != '<')
        .collect()
}

/// The digits after `code: ` in `text`.
fn code_in(text: &str) -> String {
    let start = text.find("code: ").expect("a code") + "code: ".len();
    text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect()
}

/// A P-256 key of the trusted issuer.
struct Issuer {
    pair: EcdsaKeyPair,
    rng: SystemRandom,
}

impl Issuer {
    fn new() -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        Self { pair, rng }
    }

    /// The public key as a PEM SubjectPublicKeyInfo.
    fn pem(&self) -> String {
        let mut der = vec![
            0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01, 0x06,
            0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
        ];
        der.extend_from_slice(self.pair.public_key().as_ref());
        format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            STANDARD.encode(der)
        )
    }

    /// A token for `email`, signed with ES256.
    fn token(&self, email: &str) -> String {
        let now = chrono::Utc::now().timestamp();
        let claims = json!({
            "iss": ISSUER,
            "aud": "navree-app",
            "sub": email,
            "email": email,
            "iat": now,
            "exp": now + 120,
            "jti": uuid::Uuid::now_v7().to_string(),
        });
        let header = json!({ "alg": "ES256", "typ": "JWT" });
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = self.pair.sign(&self.rng, input.as_bytes()).unwrap();
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref()))
    }
}

/// The current TOTP code of `secret`.
fn totp(secret: &str) -> String {
    use navree_crypto::totp::{Secret, Totp};
    Totp::builder(Secret::from_base32(secret).expect("a base32 secret"))
        .build()
        .expect("a valid TOTP")
        .generate(u64::try_from(chrono::Utc::now().timestamp()).unwrap())
}

/// Turns two-factor authentication on for `user`; a recovery code.
async fn two_factor(world: &World, user: &User) -> String {
    let auth = world.app.get::<AuthService>().await;
    let meta = ClientMeta::default();
    let setup = auth.enable_two_factor(user.id, &meta).await.unwrap();
    let codes = auth
        .confirm_two_factor(user.id, &totp(&setup.secret), &meta)
        .await
        .unwrap();
    codes[0].clone()
}

#[tokio::test]
async fn an_enforced_domain_cannot_use_a_password() {
    let s = Enforced::new().await;
    s.enforce().await;
    for (path, body) in [
        (
            "/api/auth/login",
            json!({ "email": "ann@acme.test", "password": PASSWORD }),
        ),
        (
            "/api/auth/register",
            json!({ "name": "Nat", "email": "nat@acme.test", "password": PASSWORD }),
        ),
    ] {
        s.post(path, body)
            .await
            .assert_error_code(403, "sso_required");
    }
    s.post(
        "/api/auth/login",
        json!({ "email": "ola@other.test", "password": PASSWORD }),
    )
    .await
    .assert_status(200);
}

#[tokio::test]
async fn a_password_change_opens_no_session_for_an_enforced_domain() {
    let s = Enforced::new().await;
    let ann = s.world.token(&s.ann).await;
    let ola = s.world.token(&s.ola).await;
    s.enforce().await;
    let change = |access: String| {
        let client = s.world.app.client();
        async move {
            client
                .patch("/api/auth/me/password")
                .bearer(&access)
                .json(
                    &json!({ "current_password": PASSWORD, "password": "a brand new passphrase" }),
                )
                .send()
                .await
        }
    };
    change(ann).await.assert_error_code(403, "sso_required");
    change(ola).await.assert_status(200);
}

#[tokio::test]
async fn a_magic_link_is_refused_for_an_enforced_domain() {
    let s = Enforced::new().await;
    // A link mailed before the domain was enforced no longer signs in.
    let early = s.magic_link("ann@acme.test").await;
    s.enforce().await;
    s.post("/api/auth/magic-link/verify", json!({ "token": early }))
        .await
        .assert_error_code(403, "sso_required");

    // No new link is mailed.
    let before = s.mail_count().await;
    s.post("/api/auth/magic-link", json!({ "email": "ann@acme.test" }))
        .await
        .assert_error_code(403, "sso_required");
    assert_eq!(s.mail_count().await, before);

    let link = s.magic_link("ola@other.test").await;
    s.post("/api/auth/magic-link/verify", json!({ "token": link }))
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("ola@other.test"));
}

#[tokio::test]
async fn an_email_code_is_refused_for_an_enforced_domain() {
    let s = Enforced::new().await;
    let early = s.login_code("ann@acme.test").await;
    s.enforce().await;
    s.post(
        "/api/auth/otp/verify",
        json!({ "email": "ann@acme.test", "code": early }),
    )
    .await
    .assert_error_code(403, "sso_required");
    s.post("/api/auth/otp", json!({ "email": "ann@acme.test" }))
        .await
        .assert_error_code(403, "sso_required");

    let code = s.login_code("ola@other.test").await;
    s.post(
        "/api/auth/otp/verify",
        json!({ "email": "ola@other.test", "code": code }),
    )
    .await
    .assert_status(200);
}

#[tokio::test]
async fn a_passkey_is_refused_for_an_enforced_domain() {
    let s = Enforced::new().await;
    let ann = s.passkey(&s.ann).await;
    let ola = s.passkey(&s.ola).await;
    s.enforce().await;
    s.passkey_login(&ann, "ann@acme.test")
        .await
        .assert_error_code(403, "sso_required");
    s.passkey_login(&ola, "ola@other.test")
        .await
        .assert_status(200);
}

#[tokio::test]
async fn a_token_exchange_is_refused_for_an_enforced_domain() {
    let s = Enforced::new().await;
    s.enforce().await;
    s.post(
        "/api/auth/exchange",
        json!({ "token": s.issuer.token("ann@acme.test") }),
    )
    .await
    .assert_error_code(403, "sso_required");
    s.post(
        "/api/auth/exchange",
        json!({ "token": s.issuer.token("ola@other.test") }),
    )
    .await
    .assert_status(200);
}

#[tokio::test]
async fn a_two_factor_challenge_started_before_enforcement_is_refused() {
    let s = Enforced::new().await;
    let ann_code = two_factor(&s.world, &s.ann).await;
    let ola_code = two_factor(&s.world, &s.ola).await;
    let challenge = |email: &'static str| {
        let s = &s;
        async move {
            s.post(
                "/api/auth/login",
                json!({ "email": email, "password": PASSWORD }),
            )
            .await
            .assert_status(200)
            .json_value()["challenge_token"]
                .as_str()
                .expect("a challenge")
                .to_owned()
        }
    };
    let ann = challenge("ann@acme.test").await;
    let ola = challenge("ola@other.test").await;
    s.enforce().await;
    s.post(
        "/api/auth/2fa/verify",
        json!({ "challenge_token": ann, "recovery_code": ann_code }),
    )
    .await
    .assert_error_code(403, "sso_required");
    s.post(
        "/api/auth/2fa/verify",
        json!({ "challenge_token": ola, "recovery_code": ola_code }),
    )
    .await
    .assert_status(200);
}

#[tokio::test]
async fn the_enforcing_connection_still_signs_in() {
    let s = Enforced::new().await;
    s.enforce().await;
    s.world
        .saml_sign_in(&s.fake, "acme", "ann@acme.test", &[])
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("ann@acme.test"));
    // A new person of the domain too (just-in-time provisioning).
    s.world
        .saml_sign_in(&s.fake, "acme", "nat@acme.test", &[])
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("nat@acme.test"));
}

#[tokio::test]
async fn another_connection_cannot_sign_in_an_enforced_domain() {
    let s = Enforced::new().await;
    let other = SsoFake::new();
    let open = Provisioning {
        allow_unverified_emails: true,
        ..Provisioning::default()
    };
    s.world
        .saml_connection(&other, SsoOwner::App, "globex", open)
        .await;
    // Before enforcement the other connection signs Bob of acme.test in.
    s.world
        .saml_sign_in(&other, "globex", "bob@acme.test", &[])
        .await
        .assert_status(200);
    s.enforce().await;
    s.world
        .saml_sign_in(&other, "globex", "bob@acme.test", &[])
        .await
        .assert_error_code(403, "sso_required");
    // Nobody new of the enforced domain is created through it either.
    s.world
        .saml_sign_in(&other, "globex", "eve@acme.test", &[])
        .await
        .assert_error_code(403, "sso_required");
    let auth = s.world.app.get::<AuthService>().await;
    assert!(auth.find_by_email("eve@acme.test").await.unwrap().is_none());
    // Other domains still sign in through it.
    s.world
        .saml_sign_in(&other, "globex", "pat@other.test", &[])
        .await
        .assert_status(200);
}

#[tokio::test]
async fn refusals_are_audited_with_the_method() {
    let s = Enforced::new().await;
    s.enforce().await;
    s.post("/api/auth/otp", json!({ "email": "ann@acme.test" }))
        .await
        .assert_error_code(403, "sso_required");
    let refused: Vec<Value> = s
        .world
        .audit
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|entry| entry.action == "auth.login_refused")
        .map(|entry| entry.metadata.clone())
        .collect();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0]["method"], "otp");
    assert_eq!(refused[0]["code"], "sso_required");
    assert_eq!(refused[0]["email"], "ann@acme.test");
}

#[tokio::test]
async fn a_connection_that_does_not_enforce_leaves_every_method_open() {
    let issuer = Issuer::new();
    let world =
        support::world_with(builder(&issuer).config("sso.enforce_for_verified_domains", false))
            .await;
    let fake = SsoFake::new();
    let connection = world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;
    world.verified_domain(&connection, "acme.test").await;
    world.create_verified_user("Ann", "ann@acme.test").await;
    let client = world.app.client();
    client
        .post("/api/auth/login")
        .json(&json!({ "email": "ann@acme.test", "password": PASSWORD }))
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/auth/exchange")
        .json(&json!({ "token": issuer.token("ann@acme.test") }))
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/auth/otp")
        .json(&json!({ "email": "ann@acme.test" }))
        .send()
        .await
        .assert_status(202);
    world
        .saml_sign_in(&fake, "acme", "ann@acme.test", &[])
        .await
        .assert_status(200);
}

#[tokio::test]
async fn an_email_change_cannot_move_an_account_across_an_enforced_domain() {
    let s = Enforced::new().await;
    let ann = s.world.token(&s.ann).await;
    let ola = s.world.token(&s.ola).await;
    s.enforce().await;
    let change = |access: String, email: &'static str| {
        let client = s.world.app.client();
        async move {
            client
                .patch("/api/auth/me")
                .bearer(&access)
                .json(&json!({ "email": email, "current_password": PASSWORD }))
                .send()
                .await
        }
    };
    // Ann's old password can't take her account out of the domain, where
    // it would sign in with a password again.
    change(ann, "ann@personal.test")
        .await
        .assert_error_code(403, "sso_required");
    // Ola can't put her account on an address of the domain: it would
    // stand in the way of that person's first single sign-on.
    change(ola.clone(), "cto@acme.test")
        .await
        .assert_error_code(403, "sso_required");
    let auth = s.world.app.get::<AuthService>().await;
    assert_eq!(
        auth.find_by_email("ann@acme.test")
            .await
            .unwrap()
            .map(|u| u.id),
        Some(s.ann.id)
    );
    assert!(auth.find_by_email("cto@acme.test").await.unwrap().is_none());
    s.world
        .saml_sign_in(&s.fake, "acme", "cto@acme.test", &[])
        .await
        .assert_status(200);
    // Domains that don't enforce are unaffected.
    change(ola, "ola@elsewhere.test").await.assert_status(200);
}

#[tokio::test]
async fn other_spellings_of_an_enforced_domain_are_the_domain() {
    let s = Enforced::new().await;
    let other = SsoFake::new();
    let open = Provisioning {
        allow_unverified_emails: true,
        ..Provisioning::default()
    };
    s.world
        .saml_connection(&other, SsoOwner::App, "globex", open)
        .await;
    s.enforce().await;
    // A trailing dot names the same domain, and so does an international
    // spelling that maps to it: mail to them reaches acme.test. Another
    // connection's identity provider can't create anyone of the domain
    // under one of them.
    for email in [
        "eve@acme.test.",
        "eve@ACME.TEST.",
        "eve@\u{ff21}\u{ff23}\u{ff2d}\u{ff25}.test",
        "eve@acme\u{3002}test",
    ] {
        s.world
            .saml_sign_in(&other, "globex", email, &[])
            .await
            .assert_error_code(403, "sso_required");
    }
    let discovered = s
        .post("/api/sso/discover", json!({ "email": "nat@acme.test." }))
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(discovered["password_allowed"], json!(false), "{discovered}");
}
