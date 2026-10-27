//! navree-validation integration tests: the extractors run through the full
//! navree-http pipeline of a `TestApp`.

mod database;
mod form;
#[cfg(feature = "html")]
mod html;
mod intl;
mod json;
mod module;
#[cfg(feature = "phone")]
mod phone;
mod rules;
mod support;
mod values;
