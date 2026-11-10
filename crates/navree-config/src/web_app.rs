//! The settings a kit's web app repeats from the API's configuration.
//!
//! A kit's Next.js app reads `web/.env.local` ([`WEB_APP_ENV`]), a file of
//! its own with names of its own. Three facts in it repeat what the API's
//! configuration says: where the API listens, the real-time key and port,
//! and the origins the API lets call it. `config:check` compares them (as
//! `navree dev` does when it starts both), since a mismatch shows up as a
//! front end that cannot reach the API.

use std::path::Path;

use crate::{Config, parse_dotenv};

/// Where a kit's web app keeps its settings, from the app's directory.
pub const WEB_APP_ENV: &str = "web/.env.local";

/// What the web app's environment file at `path` says that `config` does
/// not. Empty when there is no such file, or when they agree. The messages
/// name settings, never the value of a key.
pub fn web_app_mismatches(config: &Config, path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let shown = path.display();
    let web = match parse_dotenv(&text) {
        Ok(entries) => entries,
        Err(error) => return vec![format!("{shown} cannot be read: {error}")],
    };
    let lookup = |name: &str| {
        web.iter()
            .rev()
            .find(|(entry, value)| entry == name && !value.trim().is_empty())
            .map(|(_, value)| value.trim().to_owned())
    };
    let mut problems = Vec::new();

    let listens_on = config
        .get_string("http.addr")
        .and_then(|addr| port_of(&addr));
    if let (Some(url), Some(listens_on)) = (lookup("NAVREE_API_URL"), listens_on)
        && let Some(called) = port_of(&url)
        && called != listens_on
    {
        problems.push(format!(
            "NAVREE_API_URL in {shown} points at port {called}, but the API listens on port {listens_on} (http.addr): the web app cannot reach it"
        ));
    }
    if let (Some(port), Some(listens_on)) = (lookup("NEXT_PUBLIC_REALTIME_PORT"), listens_on)
        && port != listens_on.to_string()
    {
        problems.push(format!(
            "NEXT_PUBLIC_REALTIME_PORT in {shown} is {port}, but the real-time server shares the API's port {listens_on} (http.addr)"
        ));
    }
    if let (Some(key), Some(expected)) = (
        lookup("NEXT_PUBLIC_REALTIME_KEY"),
        config.get_string("realtime.key"),
    ) && key != expected.trim()
    {
        problems.push(format!(
            "NEXT_PUBLIC_REALTIME_KEY in {shown} differs from realtime.key: the browser cannot subscribe"
        ));
    }
    if let Some(origins) = lookup("APP_ORIGINS") {
        let allowed = allowed_origins(config);
        for origin in origins.split(',').map(str::trim).filter(|o| !o.is_empty()) {
            if !allowed
                .iter()
                .any(|allowed| allowed.trim_end_matches('/') == origin.trim_end_matches('/'))
            {
                problems.push(format!(
                    "APP_ORIGINS in {shown} names an origin that http.cors.allowed_origins does not list: the API would refuse its calls ({origin})"
                ));
            }
        }
    }
    problems
}

/// `http.cors.allowed_origins`: a list, or one comma-separated value (as an
/// environment variable gives it).
fn allowed_origins(config: &Config) -> Vec<String> {
    match config.section::<serde_json::Value>("http.cors.allowed_origins") {
        Ok(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(|origin| origin.trim().to_owned()))
            .collect(),
        Ok(serde_json::Value::String(list)) => list
            .split(',')
            .map(|origin| origin.trim().to_owned())
            .filter(|origin| !origin.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// The port of an address (`127.0.0.1:8080`) or of a URL
/// (`http://127.0.0.1:8080/api`); `https` without a port is 443, `http`
/// without one is 80.
fn port_of(address: &str) -> Option<u16> {
    let (default, rest) = match address.trim().split_once("://") {
        Some(("https", rest)) => (443, rest),
        Some((_, rest)) => (80, rest),
        None => (80, address.trim()),
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    // An IPv6 address without a port ends in `]`.
    if authority.ends_with(']') {
        return Some(default);
    }
    match authority.rsplit_once(':') {
        Some((_, port)) => port.parse().ok(),
        None => Some(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_are_read_from_addresses_and_urls() {
        assert_eq!(port_of("127.0.0.1:8080"), Some(8080));
        assert_eq!(port_of("http://127.0.0.1:8080/api"), Some(8080));
        assert_eq!(port_of("https://api.example.com"), Some(443));
        assert_eq!(port_of("http://api.example.com/x"), Some(80));
        assert_eq!(port_of("http://user:pass@host:81"), Some(81));
        assert_eq!(port_of("[::1]:8080"), Some(8080));
        assert_eq!(port_of("http://[::1]/x"), Some(80));
        assert_eq!(port_of("http://host:port"), None);
    }
}
