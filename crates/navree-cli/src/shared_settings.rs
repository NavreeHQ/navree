//! The settings a kit's API and web app must agree on.
//!
//! The Next.js app reads `web/.env.local`, a file of its own with names of
//! its own. Three facts in it repeat what the API's configuration says: where
//! the API listens, the real-time key and port, and the origins the API lets
//! call it. `navree dev` compares them at start and warns about a mismatch,
//! since a mismatch shows up as a front end that cannot reach the API.

use std::path::Path;

/// What does not agree between `web/.env.local` and the API's configuration
/// (`config/app.toml` and `.env`) in the app at `root`. Empty when the app
/// has no `web/.env.local`, or when they agree. The messages name settings,
/// never values.
pub fn mismatches(root: &Path) -> Vec<String> {
    let Some(web) = read_env(&root.join("web/.env.local")) else {
        return Vec::new();
    };
    let api = Api::read(root);
    let mut problems = Vec::new();

    let listens_on = api
        .get("http.addr", "APP_HTTP__ADDR")
        .and_then(|addr| port_of(&addr));
    if let (Some(url), Some(listens_on)) = (lookup(&web, "NAVREE_API_URL"), listens_on)
        && let Some(called) = port_of(&url)
        && called != listens_on
    {
        problems.push(format!(
            "NAVREE_API_URL in web/.env.local points at port {called}, but the API listens on port {listens_on} (http.addr): the web app cannot reach it"
        ));
    }
    if let (Some(port), Some(listens_on)) = (lookup(&web, "NEXT_PUBLIC_REALTIME_PORT"), listens_on)
        && port.trim() != listens_on.to_string()
    {
        problems.push(format!(
            "NEXT_PUBLIC_REALTIME_PORT in web/.env.local is {}, but the real-time server shares the API's port {listens_on} (http.addr)",
            port.trim()
        ));
    }
    if let (Some(key), Some(expected)) = (
        lookup(&web, "NEXT_PUBLIC_REALTIME_KEY"),
        api.get("realtime.key", "APP_REALTIME__KEY"),
    ) && key.trim() != expected.trim()
    {
        problems.push(
            "NEXT_PUBLIC_REALTIME_KEY in web/.env.local differs from realtime.key: the browser cannot subscribe"
                .to_owned(),
        );
    }
    if let Some(origins) = lookup(&web, "APP_ORIGINS") {
        let allowed = api.list(
            "http.cors.allowed_origins",
            "APP_HTTP__CORS__ALLOWED_ORIGINS",
        );
        for origin in origins.split(',').map(str::trim).filter(|o| !o.is_empty()) {
            if !allowed
                .iter()
                .any(|allowed| allowed.trim_end_matches('/') == origin.trim_end_matches('/'))
            {
                problems.push(format!(
                    "APP_ORIGINS in web/.env.local names an origin that http.cors.allowed_origins does not list: the API would refuse its calls ({origin})"
                ));
            }
        }
    }
    problems
}

/// The API's own settings: `.env` over `config/app.toml`.
struct Api {
    dotenv: Vec<(String, String)>,
    table: Option<toml::Table>,
}

impl Api {
    fn read(root: &Path) -> Api {
        Api {
            dotenv: read_env(&root.join(".env")).unwrap_or_default(),
            table: std::fs::read_to_string(root.join("config/app.toml"))
                .ok()
                .and_then(|text| text.parse::<toml::Table>().ok()),
        }
    }

    /// The setting `key` (dotted), or the `.env` entry `variable`.
    fn get(&self, key: &str, variable: &str) -> Option<String> {
        if let Some(value) = lookup(&self.dotenv, variable) {
            return Some(value);
        }
        let mut value = self
            .table
            .as_ref()
            .map(|table| toml::Value::Table(table.clone()))?;
        for part in key.split('.') {
            value = value.get(part)?.clone();
        }
        value.as_str().map(str::to_owned)
    }

    /// The list at `key`, or the comma-separated `.env` entry `variable`.
    fn list(&self, key: &str, variable: &str) -> Vec<String> {
        if let Some(value) = lookup(&self.dotenv, variable) {
            return value
                .split(',')
                .map(|item| item.trim().to_owned())
                .collect();
        }
        let Some(table) = self.table.as_ref() else {
            return Vec::new();
        };
        let mut value = toml::Value::Table(table.clone());
        for part in key.split('.') {
            match value.get(part) {
                Some(next) => value = next.clone(),
                None => return Vec::new(),
            }
        }
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// The `KEY=value` lines of a `.env` file; `None` when it cannot be read.
fn read_env(path: &Path) -> Option<Vec<(String, String)>> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| {
                let (name, value) = line.split_once('=')?;
                let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
                Some((name.trim().to_owned(), value.to_owned()))
            })
            .collect(),
    )
}

/// The last entry called `name`.
fn lookup(entries: &[(String, String)], name: &str) -> Option<String> {
    entries
        .iter()
        .rev()
        .find(|(entry, value)| entry == name && !value.is_empty())
        .map(|(_, value)| value.clone())
}

/// The port of an address (`127.0.0.1:8080`) or of a URL
/// (`http://127.0.0.1:8080/api`); `https` without a port is 443, `http`
/// without one is 80.
fn port_of(address: &str) -> Option<u16> {
    let (default, rest) = match address.split_once("://") {
        Some(("https", rest)) => (443, rest),
        Some((_, rest)) => (80, rest),
        None => (80, address),
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

    fn app(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (path, contents) in files {
            let target = dir.path().join(path);
            std::fs::create_dir_all(target.parent().expect("parent")).expect("directory");
            std::fs::write(target, contents).expect("write");
        }
        dir
    }

    const API: &str = "[http]\naddr = \"127.0.0.1:8080\"\n[http.cors]\nallowed_origins = [\"http://localhost:3000\"]\n[realtime]\nkey = \"agra\"\n";
    const WEB: &str = "NAVREE_API_URL=http://127.0.0.1:8080\nNEXT_PUBLIC_REALTIME_KEY=agra\nNEXT_PUBLIC_REALTIME_PORT=8080\n";

    #[test]
    fn matching_files_have_nothing_to_say() {
        let dir = app(&[("config/app.toml", API), ("web/.env.local", WEB)]);
        assert!(mismatches(dir.path()).is_empty());
        // An app without a web app has nothing to compare.
        let dir = app(&[("config/app.toml", API)]);
        assert!(mismatches(dir.path()).is_empty());
    }

    #[test]
    fn a_wrong_port_key_or_origin_is_named() {
        let web = "NAVREE_API_URL=http://127.0.0.1:9090\nNEXT_PUBLIC_REALTIME_KEY=other\nNEXT_PUBLIC_REALTIME_PORT=9090\nAPP_ORIGINS=https://agra.example.com\n";
        let dir = app(&[("config/app.toml", API), ("web/.env.local", web)]);
        let problems = mismatches(dir.path());
        assert_eq!(problems.len(), 4, "{problems:?}");
        assert!(problems[0].contains("NAVREE_API_URL") && problems[0].contains("9090"));
        assert!(problems[1].contains("NEXT_PUBLIC_REALTIME_PORT"));
        assert!(problems[2].contains("NEXT_PUBLIC_REALTIME_KEY"));
        assert!(problems[3].contains("APP_ORIGINS"));
        // The key is named, never shown.
        assert!(
            !problems
                .iter()
                .any(|p| p.contains("other") || p.contains("agra\""))
        );
    }

    #[test]
    fn dotenv_ranks_above_the_config_file() {
        let dir = app(&[
            ("config/app.toml", API),
            (
                ".env",
                "APP_HTTP__ADDR=127.0.0.1:9090\nAPP_REALTIME__KEY=agra\n",
            ),
            ("web/.env.local", WEB),
        ]);
        let problems = mismatches(dir.path());
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].contains("points at port 8080"));
        assert!(problems[0].contains("listens on port 9090"));
    }

    #[test]
    fn an_origin_the_api_lists_is_fine_with_or_without_a_slash() {
        let web = format!("{WEB}APP_ORIGINS=http://localhost:3000/\n");
        let dir = app(&[("config/app.toml", API), ("web/.env.local", &web)]);
        assert!(mismatches(dir.path()).is_empty());
    }

    #[test]
    fn ports_are_read_from_addresses_and_urls() {
        assert_eq!(port_of("127.0.0.1:8080"), Some(8080));
        assert_eq!(port_of("http://127.0.0.1:8080/api"), Some(8080));
        assert_eq!(port_of("https://api.example.com"), Some(443));
        assert_eq!(port_of("http://api.example.com/x"), Some(80));
        assert_eq!(port_of("http://user:pass@host:81"), Some(81));
        assert_eq!(port_of("[::1]:8080"), Some(8080));
        assert_eq!(port_of("http://[::1]/x"), Some(80));
    }
}
