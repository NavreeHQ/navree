//! # navree-sso
//!
//! `SsoModule`: SAML 2.0 and OpenID Connect single sign-on.
#![forbid(unsafe_code)]

mod dsig;
mod xml;

pub use dsig::SignatureError;
pub use xml::XmlError;
