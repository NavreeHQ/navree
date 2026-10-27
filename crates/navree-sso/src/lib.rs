//! # navree-sso
//!
//! `SsoModule`: SAML 2.0 and OpenID Connect single sign-on.
#![forbid(unsafe_code)]

mod dsig;
mod fake;
pub mod saml;
mod xml;

pub use dsig::SignatureError;
pub use fake::{FakeResponse, SsoFake};
pub use xml::XmlError;
