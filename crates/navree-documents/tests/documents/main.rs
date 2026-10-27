//! navree-documents integration tests.

#[cfg(feature = "chrome")]
mod cdp_mock;
#[cfg(feature = "chrome")]
mod chrome;
#[cfg(feature = "chrome")]
mod chrome_real;
mod pdf;
mod support;
#[cfg(feature = "typst")]
mod typst;
