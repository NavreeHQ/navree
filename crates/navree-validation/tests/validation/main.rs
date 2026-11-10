//! navree-validation integration tests: the extractors run through the full
//! navree-http pipeline of a `TestApp`.

mod database;
mod databases;
mod derive;
mod form;
#[cfg(feature = "html")]
mod html;
mod input;
mod intl;
mod json;
mod module;
mod native;
#[cfg(feature = "phone")]
mod phone;
mod rules;
mod support;
mod value;
mod values;
#[cfg(feature = "html")]
mod worker;
