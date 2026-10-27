//! Logs and metrics of MCP calls: one line at target `navree::mcp` per
//! call, `mcp_requests_total` and `mcp_request_duration_seconds`.

use std::time::Instant;

use navree_contracts::Principal;

use crate::context::McpClient;

/// Records one `tools/call`, `resources/read` or `prompts/get`. `name` is
/// the item's name (`None` for an unknown one, labelled `unknown`, so
/// clients cannot grow the label set); `outcome` is `ok`, `error` or
/// `unknown`. Arguments and results are never logged.
pub(crate) fn call(
    method: &'static str,
    name: Option<&str>,
    principal: &Principal,
    request_id: &str,
    client: Option<&McpClient>,
    started: Instant,
    outcome: &'static str,
) {
    let elapsed = started.elapsed();
    let name = name.unwrap_or("unknown").to_owned();
    tracing::info!(
        target: "navree::mcp",
        method,
        name = %name,
        user_id = %principal.user_id,
        client = client.map(|client| client.name.as_str()),
        duration_ms = elapsed.as_millis() as u64,
        outcome,
        request_id,
        "mcp call"
    );
    metrics::counter!(
        "mcp_requests_total",
        "method" => method,
        "name" => name.clone(),
        "outcome" => outcome,
    )
    .increment(1);
    metrics::histogram!(
        "mcp_request_duration_seconds",
        "method" => method,
        "name" => name,
    )
    .record(elapsed.as_secs_f64());
}
