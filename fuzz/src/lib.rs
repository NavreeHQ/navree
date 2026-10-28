//! Shared helpers of the fuzz targets: fixed test keys and fixtures, and a
//! way to run async code from a synchronous target.

use std::future::Future;
use std::sync::OnceLock;

/// Runs `future` to completion on a process-wide current-thread runtime.
///
/// Spawned tasks run only while a call is blocked, which is all a target
/// needs: every call waits for the work it started.
pub fn block_on<F: Future>(future: F) -> F::Output {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a tokio runtime")
        })
        .block_on(future)
}

/// A UTF-8 view of the input, or `None` for invalid UTF-8 (targets for text
/// parsers skip those inputs: the callers of these parsers hold `&str`).
pub fn text(data: &[u8]) -> Option<&str> {
    std::str::from_utf8(data).ok()
}

/// Splits `data` at the first NUL into two parts (the second empty when
/// there is no NUL): a cheap way to feed a target two strings.
pub fn split_nul(data: &[u8]) -> (&[u8], &[u8]) {
    match data.iter().position(|b| *b == 0) {
        Some(at) => (&data[..at], &data[at + 1..]),
        None => (data, &[]),
    }
}

/// The JOSE fixtures shared with the crate's own tests.
pub mod jose {
    /// A JWKS of Google's keys (RSA).
    pub const GOOGLE_JWKS: &str =
        include_str!("../../crates/navree-crypto/tests/fixtures/jose/google-jwks.json");
    /// A JWKS of Apple's keys (RSA).
    pub const APPLE_JWKS: &str =
        include_str!("../../crates/navree-crypto/tests/fixtures/jose/apple-jwks.json");
    /// A JWKS of Microsoft's keys (RSA).
    pub const MICROSOFT_JWKS: &str =
        include_str!("../../crates/navree-crypto/tests/fixtures/jose/microsoft-jwks.json");
    /// An RSA-2048 public key.
    pub const RSA_PUBLIC_PEM: &str =
        include_str!("../../crates/navree-crypto/tests/fixtures/jose/rsa-2048-public.pem");
    /// An RSA-2048 private key.
    pub const RSA_PRIVATE_PEM: &str =
        include_str!("../../crates/navree-crypto/tests/fixtures/jose/rsa-2048.pem");
    /// An Apple `.p8` key (P-256, PKCS#8).
    pub const APPLE_P8: &str =
        include_str!("../../crates/navree-crypto/tests/fixtures/jose/apple-authkey.p8");
    /// An RSA-2048 certificate.
    pub const RSA_CERT_PEM: &str =
        include_str!("../../crates/navree-crypto/tests/fixtures/jose/rsa-2048-cert.pem");
    /// A P-256 certificate.
    pub const P256_CERT_PEM: &str =
        include_str!("../../crates/navree-crypto/tests/fixtures/jose/p256-cert.pem");
    /// A P-256 public key.
    pub const P256_PUBLIC_PEM: &str =
        include_str!("../../crates/navree-crypto/tests/fixtures/jose/p256-cert-public.pem");
}

/// The SAML fixtures shared with navree-sso's own tests.
pub mod sso {
    /// A signed Okta response.
    pub const OKTA: &str = include_str!("../../crates/navree-sso/tests/fixtures/idp/okta.xml");
    /// An Entra ID response.
    pub const ENTRA: &str = include_str!("../../crates/navree-sso/tests/fixtures/idp/entra.xml");
    /// A Google Workspace response.
    pub const GOOGLE: &str = include_str!("../../crates/navree-sso/tests/fixtures/idp/google.xml");
    /// The issuer of [`OKTA`].
    pub const OKTA_ISSUER: &str = "http://www.okta.com/exk8acmeTEST4x7";
    /// The issuer of [`ENTRA`].
    pub const ENTRA_ISSUER: &str = "https://sts.windows.net/9e3c5a0f-1b2d-4c6e-8f0a-2b4c6d8e0f1a/";
    /// The issuer of [`GOOGLE`].
    pub const GOOGLE_ISSUER: &str = "https://accounts.google.com/o/saml2?idpid=C03acme01";
    /// The request id the fixtures answer.
    pub const REQUEST_ID: &str = "_a1b2c3d4e5f60718293a4b5c6d7e8f90";
    /// The assertion consumer service of the fixtures.
    pub const ACS: &str = "https://api.acme.test/api/sso/saml/acme/acs";
    /// The service provider's entity id in the fixtures.
    pub const ENTITY: &str = "https://api.acme.test/api/sso/saml/acme/metadata";
}
