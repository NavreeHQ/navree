//! # navree-directory
//!
//! `DirectoryModule`: directory sync for business apps.
#![forbid(unsafe_code)]

pub mod scim;
mod user;

pub use user::{ContactValue, DirectoryGroup, DirectoryUser};
