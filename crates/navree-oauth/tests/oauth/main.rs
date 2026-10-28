//! Integration tests of navree-oauth: the authorization code flow, the
//! other grants, tokens on the API, OpenID Connect, personal access tokens,
//! introspection and revocation, clients and commands, configuration,
//! dynamic registration and workspaces.

mod support;

mod authorizations;
mod authorize;
mod clients;
mod config;
mod grants;
mod introspect;
mod offboarding;
mod oidc;
mod openapi;
mod orm;
mod pats;
mod registration;
mod throttle;
mod tokens;
mod workspace;
