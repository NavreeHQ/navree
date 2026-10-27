//! navree-flags integration tests: flags as types, resolution by scope and
//! rollout, storage, the cache, writes in units of work, the request
//! extractor and route guard, the routes, the commands and the fake.

mod support;

mod admin;
mod commands;
mod fake;
mod http;
mod module;
mod orm;
mod resolution;
mod rollout;
mod store;
mod writes;
