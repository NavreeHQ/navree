//! Navree's own implementation of the Model Context Protocol: JSON-RPC 2.0
//! messages, the lifecycle and version negotiation, and the shapes the
//! server sends. The transports (`crate::http`, `crate::stdio`) and the
//! handler (`crate::handler`) build on it.
//!
//! Protocol versions: the handshake protocols `2024-11-05`, `2025-03-26`,
//! `2025-06-18` and `2025-11-25` (an `initialize` request opens a session)
//! and the stateless `2026-07-28` (no handshake: every request carries its
//! protocol version and client capabilities in `_meta`).

pub(crate) mod jsonrpc;
pub(crate) mod lifecycle;
pub(crate) mod outlet;
mod types;

pub use jsonrpc::MAX_JSON_DEPTH;
pub use types::{
    Implementation, Prompt, PromptArgumentInfo, Resource, ResourceTemplate, Tool, ToolAnnotations,
};

/// Every protocol version the server speaks, oldest first.
pub const SUPPORTED_VERSIONS: [&str; 5] = [
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
    "2025-11-25",
    STATELESS_VERSION,
];

/// The newest protocol version with an `initialize` handshake: what the
/// server agrees to when a client asks for a version it does not speak.
pub const LATEST_HANDSHAKE_VERSION: &str = "2025-11-25";

/// The first stateless protocol version: no handshake, no sessions.
pub const STATELESS_VERSION: &str = "2026-07-28";

/// Whether the server speaks `version`.
pub fn is_supported(version: &str) -> bool {
    SUPPORTED_VERSIONS.contains(&version)
}

/// Whether `version` opens sessions with an `initialize` handshake (every
/// version before [`STATELESS_VERSION`]; dated versions compare as text).
pub fn has_handshake(version: &str) -> bool {
    version < STATELESS_VERSION
}

/// The version `initialize` agrees to when the client asks for `requested`:
/// the same one when the server speaks it over a handshake, else
/// [`LATEST_HANDSHAKE_VERSION`].
pub fn negotiate(requested: &str) -> &'static str {
    SUPPORTED_VERSIONS
        .iter()
        .copied()
        .find(|version| *version == requested && has_handshake(version))
        .unwrap_or(LATEST_HANDSHAKE_VERSION)
}

/// Whether `version` allows JSON-RPC batches (only `2025-03-26`: the
/// versions after it removed them).
pub fn allows_batches(version: &str) -> bool {
    version == "2025-03-26"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_echoes_handshake_versions() {
        for version in &SUPPORTED_VERSIONS[..4] {
            assert_eq!(negotiate(version), *version);
        }
        assert_eq!(negotiate(STATELESS_VERSION), LATEST_HANDSHAKE_VERSION);
        assert_eq!(negotiate("1999-01-01"), LATEST_HANDSHAKE_VERSION);
        assert_eq!(negotiate("2099-01-01"), LATEST_HANDSHAKE_VERSION);
        assert!(has_handshake("2025-11-25"));
        assert!(!has_handshake(STATELESS_VERSION));
        assert!(allows_batches("2025-03-26"));
        assert!(!allows_batches("2025-06-18"));
        assert!(!allows_batches(STATELESS_VERSION));
    }
}
