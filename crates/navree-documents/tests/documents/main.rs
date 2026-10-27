//! navree-documents integration tests.

#[cfg(feature = "chrome")]
mod cdp_mock;
#[cfg(feature = "chrome")]
mod chrome;
#[cfg(feature = "chrome")]
mod chrome_real;
mod conformance;
#[cfg(feature = "docx")]
mod docx;
mod fake;
mod module;
#[cfg(feature = "native")]
mod native;
#[cfg(feature = "operations")]
mod operations;
mod pdf;
mod service;
mod support;
