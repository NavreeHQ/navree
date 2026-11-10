//! [`DirectorySource`]: where a pull connection's users and groups come
//! from, and the built-in sources.

pub(crate) mod google;
pub(crate) mod graph;
pub(crate) mod ldap;
pub(crate) mod okta;

use std::fmt;
use std::sync::Arc;

use navree_contracts::Secret;
use navree_integrations::{IntegrationClient, Integrations};
use navree_kernel::{AppContext, BoxFuture};
use serde_json::Value;

use crate::error::DirectoryError;
use crate::pulled::Pulled;
use crate::types::DirectoryConnection;

/// The integrations scope set the built-in API sources connect with.
pub const DIRECTORY_SCOPE_SET: &str = "directory";

/// A directory a connection pulls from. Navree ships `microsoft`,
/// `google`, `okta` and `ldap`; register your own (an HR system, a CSV
/// drop) with
/// [`m.directory_source(name, source)`](crate::ModuleBuilderDirectoryExt::directory_source).
///
/// ```no_run
/// use navree_directory::{DirectoryError, DirectorySource, DirectoryUser, PullRequest, Pulled};
/// use navree_kernel::BoxFuture;
///
/// /// People from the HR system's export.
/// struct HrSource;
///
/// impl DirectorySource for HrSource {
///     fn pull<'a>(&'a self, request: &'a PullRequest) -> BoxFuture<'a, Result<Pulled, DirectoryError>> {
///         Box::pin(async move {
///             let _url = request.setting("export_url");
///             let people = vec![DirectoryUser::new("E-1001", "anu@acme.test").email("anu@acme.test")];
///             Ok(Pulled::snapshot(people, Vec::new()))
///         })
///     }
/// }
/// ```
pub trait DirectorySource: Send + Sync + 'static {
    /// Checks a connection's settings when it is created or changed:
    /// `Err` names the problem (`settings.base_dn is required`).
    /// `has_secret` tells whether a secret is stored or given.
    fn check_settings(&self, settings: &Value, has_secret: bool) -> Result<(), String> {
        let _ = (settings, has_secret);
        Ok(())
    }

    /// Pulls the directory: everything, or the changes since
    /// `request.cursor` when the source supports deltas.
    fn pull<'a>(
        &'a self,
        request: &'a PullRequest,
    ) -> BoxFuture<'a, Result<Pulled, DirectoryError>>;
}

/// What a source gets for a pull.
#[derive(Clone)]
pub struct PullRequest {
    /// The connection.
    pub connection: DirectoryConnection,
    /// Where the last complete run left off (`None`: pull everything).
    pub cursor: Option<String>,
    /// The connection's secret, decrypted (the LDAP bind password).
    pub secret: Option<Secret<String>>,
    /// The app, to resolve services.
    pub app: AppContext,
}

impl PullRequest {
    /// A setting as a string (`None` when missing or empty).
    pub fn setting(&self, key: &str) -> Option<&str> {
        self.connection
            .setting(key)
            .filter(|value| !value.is_empty())
    }

    /// A boolean setting.
    pub fn flag(&self, key: &str, default: bool) -> bool {
        match self.connection.settings.get(key) {
            Some(Value::Bool(value)) => *value,
            Some(Value::String(text)) => match text.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => default,
            },
            _ => default,
        }
    }

    /// A number setting.
    pub fn number(&self, key: &str, default: u64) -> u64 {
        match self.connection.settings.get(key) {
            Some(Value::Number(number)) => number.as_u64().unwrap_or(default),
            Some(Value::String(text)) => text.trim().parse().unwrap_or(default),
            _ => default,
        }
    }

    /// The client of the owner's integrations connection to `provider`
    /// (setting `provider`, else `default_provider`) in the scope set
    /// `directory` (setting `scope_set`).
    pub async fn client(
        &self,
        default_provider: &str,
    ) -> Result<IntegrationClient, DirectoryError> {
        let integrations = self.app.try_get::<Integrations>().await.ok_or_else(|| {
            DirectoryError::Config(format!(
                "the `{}` source needs IntegrationsModule in the app",
                self.connection.source
            ))
        })?;
        let provider = self.setting("provider").unwrap_or(default_provider);
        let scope_set = self.setting("scope_set").unwrap_or(DIRECTORY_SCOPE_SET);
        Ok(integrations
            .client(self.connection.owner(), provider, scope_set)
            .await?)
    }
}

impl fmt::Debug for PullRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PullRequest")
            .field("connection", &self.connection.id)
            .field("source", &self.connection.source)
            .field("cursor", &self.cursor.is_some())
            .field("secret", &self.secret.is_some())
            .finish_non_exhaustive()
    }
}

/// A source registered by a module (a contribution).
#[derive(Clone)]
pub(crate) struct SourceRegistration {
    pub(crate) name: String,
    pub(crate) source: Arc<dyn DirectorySource>,
}

/// Source names: lower-case letters, digits, `-` and `_`.
pub(crate) fn is_source_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The built-in pull sources, by name.
pub(crate) fn built_in() -> Vec<(&'static str, Arc<dyn DirectorySource>)> {
    vec![
        (crate::types::MICROSOFT, Arc::new(graph::GraphSource)),
        (crate::types::GOOGLE, Arc::new(google::GoogleSource)),
        (crate::types::OKTA, Arc::new(okta::OktaSource)),
        (crate::types::LDAP, Arc::new(ldap::LdapSource)),
    ]
}

/// The string of a JSON value, when it is a non-empty string.
pub(crate) fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names_are_lower_case() {
        assert!(is_source_name("workday"));
        assert!(is_source_name("hr-export_2"));
        assert!(!is_source_name("Workday"));
        assert!(!is_source_name(""));
        assert!(!is_source_name("a b"));
    }

    #[test]
    fn text_skips_blank_values() {
        assert_eq!(text(Some(&Value::String(" x ".into()))), Some("x".into()));
        assert_eq!(text(Some(&Value::String("  ".into()))), None);
        assert_eq!(text(Some(&Value::Bool(true))), None);
        assert_eq!(text(None), None);
    }
}
