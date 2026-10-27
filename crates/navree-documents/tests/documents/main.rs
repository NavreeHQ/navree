//! navree-documents integration tests.

#[cfg(feature = "chrome")]
mod cdp_mock;
#[cfg(feature = "chrome")]
mod chrome;
#[cfg(feature = "chrome")]
mod chrome_real;
mod conformance;
mod fake;
mod module;
mod pdf;
mod service;
mod support;
#[cfg(feature = "typst")]
mod typst;
