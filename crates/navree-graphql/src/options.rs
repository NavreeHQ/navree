//! [`GraphqlOptions`]: the `graphql` configuration section.

use navree_config::ConfigSection;
use serde::{Deserialize, Serialize};

/// How the GraphQL endpoint is served (config section `graphql`, env
/// `APP_GRAPHQL__<KEY>`). Every key has a default; build literals with
/// `..GraphqlOptions::default()`.
///
/// ```
/// use navree_graphql::GraphqlOptions;
///
/// let options = GraphqlOptions {
///     depth_limit: 12,
///     ..GraphqlOptions::default()
/// };
/// assert_eq!(options.path, "/graphql");
/// assert_eq!(options.ws_path(), "/graphql/ws");
/// assert!(options.check().is_ok());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphqlOptions {
    /// The endpoint (default `/graphql`). Absolute: `http.api_prefix` does
    /// not apply.
    pub path: String,
    /// Serve GraphiQL to browsers that open the endpoint (default: on in
    /// debug builds).
    pub graphiql: bool,
    /// Answer introspection queries (default: on in debug builds).
    pub introspection: bool,
    /// The deepest selection a query may nest (default 16).
    pub depth_limit: usize,
    /// The highest complexity score a query may have (default 1000; each
    /// field counts 1 unless it says otherwise).
    pub complexity_limit: usize,
    /// Accept a JSON array of operations in one request (default off).
    pub batching: bool,
    /// Serve subscriptions over WebSockets at `<path>/ws` (default on).
    pub subscriptions: bool,
    /// How long a WebSocket may wait before sending `connection_init`
    /// (default 10 s).
    pub connection_init_timeout_secs: u64,
    /// How often the server pings an idle WebSocket (default 15 s; 0 turns
    /// the pings off).
    pub keep_alive_secs: u64,
}

impl Default for GraphqlOptions {
    fn default() -> Self {
        Self {
            path: "/graphql".to_owned(),
            graphiql: cfg!(debug_assertions),
            introspection: cfg!(debug_assertions),
            depth_limit: 16,
            complexity_limit: 1000,
            batching: false,
            subscriptions: true,
            connection_init_timeout_secs: 10,
            keep_alive_secs: 15,
        }
    }
}

impl GraphqlOptions {
    /// The subscriptions endpoint: `<path>/ws`.
    pub fn ws_path(&self) -> String {
        format!("{}/ws", self.path)
    }

    /// Every problem with the options, each naming its key.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        let path = self.path.as_str();
        let valid_path = path.len() > 1
            && path.starts_with('/')
            && !path.ends_with('/')
            && !path.contains("//")
            && path
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'));
        if !valid_path {
            problems.push(format!(
                "graphql.path must be an absolute path such as /graphql (letters, digits, `-`, \
                 `_`, `.` and single `/`, no trailing `/`), got `{path}`"
            ));
        }
        if self.depth_limit == 0 {
            problems.push("graphql.depth_limit must be at least 1".to_owned());
        }
        if self.complexity_limit == 0 {
            problems.push("graphql.complexity_limit must be at least 1".to_owned());
        }
        if self.connection_init_timeout_secs == 0 {
            problems.push("graphql.connection_init_timeout_secs must be at least 1".to_owned());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

impl ConfigSection for GraphqlOptions {
    const KEY: &'static str = "graphql";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|problems| problems.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documented_table() {
        let options = GraphqlOptions::default();
        assert_eq!(options.path, "/graphql");
        assert_eq!(options.graphiql, cfg!(debug_assertions));
        assert_eq!(options.introspection, cfg!(debug_assertions));
        assert_eq!(options.depth_limit, 16);
        assert_eq!(options.complexity_limit, 1000);
        assert!(!options.batching);
        assert!(options.subscriptions);
        assert_eq!(options.connection_init_timeout_secs, 10);
        assert_eq!(options.keep_alive_secs, 15);
    }

    #[test]
    fn check_names_each_bad_key() {
        let options = GraphqlOptions {
            path: "graphql/".to_owned(),
            depth_limit: 0,
            complexity_limit: 0,
            connection_init_timeout_secs: 0,
            ..GraphqlOptions::default()
        };
        let problems = options.check().unwrap_err();
        assert_eq!(problems.len(), 4, "{problems:?}");
        assert!(problems[0].starts_with("graphql.path"));
        assert!(problems[1].starts_with("graphql.depth_limit"));
        assert!(problems[2].starts_with("graphql.complexity_limit"));
        assert!(problems[3].starts_with("graphql.connection_init_timeout_secs"));
        for path in ["/", "", "/a//b", "/{id}", "/graphql/"] {
            let options = GraphqlOptions {
                path: path.to_owned(),
                ..GraphqlOptions::default()
            };
            assert!(options.check().is_err(), "{path} is accepted");
        }
        let nested = GraphqlOptions {
            path: "/api/graphql".to_owned(),
            ..GraphqlOptions::default()
        };
        assert!(nested.check().is_ok());
    }

    #[test]
    fn missing_keys_take_their_defaults() {
        let options: GraphqlOptions =
            serde_json::from_value(serde_json::json!({ "complexity_limit": 300 })).unwrap();
        assert_eq!(options.complexity_limit, 300);
        assert_eq!(options.depth_limit, 16);
    }
}
