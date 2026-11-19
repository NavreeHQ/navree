//! The first-party package index (`index.toml`), embedded at compile time.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::plugin::manifest::default_namespace;

/// What `plugin:add` knows of the first-party packages.
#[derive(Clone, Debug)]
pub struct Index {
    /// First-party package entries in index order.
    pub packages: Vec<IndexEntry>,
    /// Namespaces kept by the framework, sorted.
    pub reserved: Vec<String>,
    /// Facade features a plugin manifest may ask for, sorted.
    pub manifest_features: Vec<String>,
}

/// One first-party entry in `index.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    /// Short name accepted by `plugin:add` (`billing`).
    pub name: String,
    /// Workspace crate name (`navree-billing`), read from `crate` in `index.toml`.
    pub krate: String,
    /// Facade feature on `navree` (`billing`).
    pub feature: String,
    /// Facade module expression (`navree::billing::BillingModule::from_config()`).
    pub module: Option<String>,
    /// Kind of entry (`module`, `dev`, `tool`, or `feature`).
    pub kind: IndexKind,
    /// CLI command to run instead for `tool` entries (`use` in `index.toml`).
    pub tool_command: Option<String>,
}

/// Kind of first-party entry in `index.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexKind {
    /// A runtime module package (`billing`).
    Module,
    /// A development-dependency feature (`browser`).
    Dev,
    /// A command-line tool (`dock`, `harbor`, `navigator`).
    Tool,
    /// A feature flag on another crate (`otel`).
    Feature,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIndex {
    package: Vec<RawEntry>,
    reserved: RawReserved,
    manifest: RawManifest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    name: String,
    #[serde(rename = "crate")]
    krate: String,
    feature: String,
    module: Option<String>,
    kind: IndexKind,
    #[serde(rename = "use")]
    tool_command: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReserved {
    namespaces: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    features: Vec<String>,
}

fn canonical_name(name: &str) -> String {
    name.replace('-', "_")
}

impl Index {
    /// `index.toml`, embedded with `include_str!` and parsed once.
    pub fn embedded() -> &'static Index {
        static EMBEDDED: OnceLock<Index> = OnceLock::new();
        EMBEDDED.get_or_init(|| {
            Self::parse(include_str!("index.toml")).expect("embedded index.toml must parse")
        })
    }

    /// Parses an index TOML document.
    pub fn parse(text: &str) -> Result<Index, String> {
        let raw: RawIndex = toml::from_str(text).map_err(|err| err.to_string())?;
        let packages = raw
            .package
            .into_iter()
            .map(|entry| IndexEntry {
                name: entry.name,
                krate: entry.krate,
                feature: entry.feature,
                module: entry.module,
                kind: entry.kind,
                tool_command: entry.tool_command,
            })
            .collect();
        Ok(Index {
            packages,
            reserved: raw.reserved.namespaces,
            manifest_features: raw.manifest.features,
        })
    }

    /// Finds an entry by short or full name (`billing`, `navree-billing` or `navree_billing`),
    /// treating `-` and `_` as one.
    pub fn find(&self, package: &str) -> Option<&IndexEntry> {
        let wanted = canonical_name(package);
        self.packages.iter().find(|entry| {
            canonical_name(&entry.name) == wanted
                || canonical_name(&format!("navree-{}", entry.name)) == wanted
        })
    }
}

impl IndexEntry {
    /// The plugin's namespace (`default_namespace` of its crate name).
    pub fn namespace(&self) -> String {
        default_namespace(&self.krate)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::plugin::manifest::{RESERVED_NAMESPACES, is_namespace};

    #[test]
    fn the_index_parses_and_finds_packages_by_short_and_full_name() {
        let index = Index::embedded();

        let billing = index.find("billing").expect("billing is in the index");
        assert_eq!(index.find("navree-billing"), Some(billing));
        assert_eq!(index.find("navree_billing"), Some(billing));
        assert_eq!(billing.krate, "navree-billing");
        assert_eq!(billing.feature, "billing");
        assert_eq!(billing.kind, IndexKind::Module);
        assert_eq!(billing.namespace(), "billing");
        assert_eq!(
            billing.module.as_deref(),
            Some("navree::billing::BillingModule::from_config()")
        );

        let http_client = index
            .find("http_client")
            .expect("http_client finds http-client");
        assert_eq!(http_client.name, "http-client");
        assert_eq!(index.find("http-client"), Some(http_client));
        assert_eq!(index.find("navree-http-client"), Some(http_client));
        assert_eq!(index.find("navree_http_client"), Some(http_client));
        assert_eq!(http_client.namespace(), "http_client");

        assert_eq!(index.find("navree-plugin-billing"), None);

        let dock = index.find("dock").expect("dock is in the index");
        assert_eq!(dock.kind, IndexKind::Tool);
        assert_eq!(dock.tool_command.as_deref(), Some("navree dock"));

        let harbor = index.find("harbor").expect("harbor is in the index");
        assert_eq!(harbor.kind, IndexKind::Tool);
        assert_eq!(harbor.tool_command.as_deref(), Some("navree harbor:init"));

        let navigator = index.find("navigator").expect("navigator is in the index");
        assert_eq!(navigator.kind, IndexKind::Tool);
        assert_eq!(navigator.tool_command.as_deref(), Some("navree mcp"));

        let browser = index.find("browser").expect("browser is in the index");
        assert_eq!(browser.kind, IndexKind::Dev);

        let otel = index.find("otel").expect("otel is in the index");
        assert_eq!(otel.kind, IndexKind::Feature);

        for entry in &index.packages {
            if entry.kind == IndexKind::Tool {
                assert!(
                    entry.tool_command.is_some(),
                    "{}: a tool entry carries `use`",
                    entry.name
                );
            } else {
                assert!(
                    entry.tool_command.is_none(),
                    "{}: only tool entries carry `use`",
                    entry.name
                );
            }
            if entry.kind == IndexKind::Module {
                assert!(
                    entry.module.is_some(),
                    "{}: a module entry carries `module`",
                    entry.name
                );
            } else {
                assert!(
                    entry.module.is_none(),
                    "{}: non-module entries omit `module`",
                    entry.name
                );
            }
        }

        assert_eq!(index.reserved.len(), 88);
        let sorted_reserved: Vec<String> = index
            .reserved
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(index.reserved, sorted_reserved);
        assert_eq!(index.reserved, RESERVED_NAMESPACES);
        for word in &index.reserved {
            assert!(is_namespace(word), "`{word}` must match namespace grammar");
        }

        assert_eq!(index.manifest_features.len(), 53);
        let sorted_features: Vec<String> = index
            .manifest_features
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(index.manifest_features, sorted_features);
    }
}
