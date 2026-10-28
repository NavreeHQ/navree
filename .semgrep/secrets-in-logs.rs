use tracing::{debug, info, warn};

fn bad(token: &str, password: String, user: &User) {
    // ruleid: navree-secret-in-log
    info!(token = %token, "issued");
    // ruleid: navree-secret-in-log
    debug!(user = %user.id, reset_token = ?user.reset_token, "reset requested");
    // ruleid: navree-secret-in-log
    tracing::warn!(%password, "sign-in failed");
    // ruleid: navree-secret-in-log
    warn!("rejected {}", user.api_key);
    // ruleid: navree-secret-in-log
    info!("bearer is {}", token);
}

fn good(token: &str, user: &User, tokens: usize) {
    // ok: navree-secret-in-log
    info!(user = %user.id, "issued a token");
    // ok: navree-secret-in-log
    info!(token_count = tokens, "issued");
    // ok: navree-secret-in-log
    debug!(token_id = %user.token_id, "revoked");
    // ok: navree-secret-in-log
    warn!("rejected {}", user.id);
    // ok: navree-secret-in-log
    let _ = format!("{}", token);
    // A count of credentials is not a credential.
    // ok: navree-secret-in-log
    info!(credentials = tokens, "revoked");
    // ok: navree-secret-in-log
    /// tracing::info!(token = %token, "a doc example is not a log call");
    let _ = 0;
}
