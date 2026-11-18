//! Installing, listing, removing and writing plugins: a plugin's manifest is
//! checked against a fixed grammar before anything is written, and the app's
//! files are edited only at known places, keeping their layout.

pub mod add;
pub(crate) mod block;
pub(crate) mod cargo_toml;
pub mod component;
pub(crate) mod files;
pub(crate) mod grants;
pub mod index;
pub mod list;
pub mod make;
pub mod manifest;
pub(crate) mod metadata;
pub(crate) mod policy;
pub mod remove;
pub(crate) mod wiring;
