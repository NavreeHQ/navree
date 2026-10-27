//! The attribute layer: mass assignment, casts that need the app, accessors
//! and mutators, visibility and serialization, API resources, OpenAPI
//! schemas and model references. Each file brings its own models and
//! tables.

mod support;

mod casts;
mod fill;
mod secrets;
mod serialize;
