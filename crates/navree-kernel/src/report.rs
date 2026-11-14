//! [`PluginsReport`]: every plugin of an app and what its modules add, what
//! the `plugins` command prints.

use std::fmt::{self, Write};

use crate::introspect::{string, strings};
use crate::key::short_type_name;
use crate::{ModuleId, PluginInfo, PublishTag};

/// Every plugin of an app and what each of its modules adds: what the
/// `plugins` command prints (its [`Display`](fmt::Display) text, or
/// [`to_json`](PluginsReport::to_json) with `--json`). Read from the module
/// graph; nothing is built to make it.
///
/// ```
/// use navree_kernel::Navree;
///
/// let report = Navree::app().plugin_report()?;
/// assert!(report.plugins.is_empty());
/// assert_eq!(report.to_string(), "No plugins: no module of this app calls m.plugin(..).");
/// # Ok::<(), navree_kernel::BootErrors>(())
/// ```
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct PluginsReport {
    /// Every plugin, by name.
    pub plugins: Vec<PluginReport>,
}

/// One plugin of a [`PluginsReport`].
///
/// ```no_run
/// use navree_kernel::{Module, ModuleBuilder, Navree};
///
/// struct AnnouncementsModule;
///
/// impl Module for AnnouncementsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.plugin(navree_kernel::plugin_info!(title = "Announcements"));
///     }
/// }
///
/// let report = Navree::app().module(AnnouncementsModule).plugin_report()?;
/// for plugin in &report.plugins {
///     println!("{} {} ({} modules)", plugin.info.name, plugin.info.version, plugin.modules.len());
/// }
/// # Ok::<(), navree_kernel::BootErrors>(())
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct PluginReport {
    /// What the plugin says about itself (its first module's
    /// [`PluginInfo`], in dependency order).
    pub info: PluginInfo,
    /// Whether the kernel finds the plugin first-party: one of the
    /// framework's own packages, at the kernel's version, every module a type
    /// of that package. The JSON's `"first_party"`.
    pub first_party: bool,
    /// Its modules, in dependency order.
    pub modules: Vec<PluginModuleReport>,
}

/// One module of a [`PluginReport`].
///
/// ```no_run
/// use navree_kernel::{Module, ModuleBuilder, Navree};
///
/// struct AnnouncementsModule;
///
/// impl Module for AnnouncementsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.plugin(navree_kernel::plugin_info!(title = "Announcements"));
///     }
/// }
///
/// let report = Navree::app().module(AnnouncementsModule).plugin_report()?;
/// let module = &report.plugins[0].modules[0];
/// assert_eq!(module.id.name(), "AnnouncementsModule");
/// assert!(module.commands.is_empty() && module.publishable.is_empty());
/// # Ok::<(), navree_kernel::BootErrors>(())
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct PluginModuleReport {
    /// The module's id: [`ModuleId::name`] and [`ModuleId::type_name`] name it.
    pub id: ModuleId,
    /// The commands it registers, in registration order.
    pub commands: Vec<&'static str>,
    /// How many values of each type it contributes, by full type name, in
    /// name order. `type` and the keys of `contributions` are
    /// `std::any::type_name` output: for reading, not for matching; they may
    /// change in any release.
    pub contributions: Vec<(&'static str, usize)>,
    /// How many files it can publish, per tag, in [`PublishTag::ALL`] order
    /// (a tag without files is left out).
    pub publishable: Vec<(PublishTag, usize)>,
}

impl PluginsReport {
    /// The report as one JSON object, what `plugins --json` prints:
    ///
    /// ```json
    /// {
    ///   "navree": "1.3.0",
    ///   "plugins": [
    ///     {"name": "navree-plugin-announcements", "namespace": "announcements",
    ///      "version": "1.0.0", "title": "Announcements", "description": null,
    ///      "homepage": null, "repository": null, "license": "MIT OR Apache-2.0",
    ///      "first_party": false,
    ///      "modules": [
    ///        {"name": "AnnouncementsModule",
    ///         "type": "navree_plugin_announcements::AnnouncementsModule",
    ///         "commands": ["announcements:prune"],
    ///         "contributions": {"navree_kernel::plugin::Publishable": 2},
    ///         "publishable": {"views": 3, "locales": 2}}
    ///      ]}
    ///   ]
    /// }
    /// ```
    ///
    /// Plugins come by name, their modules in dependency order. Keys may be
    /// added in later releases, never removed. `type` and the keys of
    /// `contributions` are `std::any::type_name` output: for reading, not for
    /// matching; they may change in any release.
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\"navree\":");
        string(&mut out, env!("CARGO_PKG_VERSION"));
        out.push_str(",\"plugins\":");
        self.write_plugins_array(&mut out);
        out.push('}');
        out
    }

    /// Writes the JSON array of plugins alone (also `introspect`'s
    /// `"plugins"`).
    pub(crate) fn write_plugins_array(&self, out: &mut String) {
        out.push('[');
        for (i, plugin) in self.plugins.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let info = &plugin.info;
            out.push_str("{\"name\":");
            string(out, info.name);
            out.push_str(",\"namespace\":");
            string(out, info.namespace);
            out.push_str(",\"version\":");
            string(out, info.version);
            for (key, value) in [
                ("title", info.title),
                ("description", info.description),
                ("homepage", info.homepage),
                ("repository", info.repository),
                ("license", info.license),
            ] {
                let _ = write!(out, ",\"{key}\":");
                match value {
                    Some(value) => string(out, value),
                    None => out.push_str("null"),
                }
            }
            let _ = write!(out, ",\"first_party\":{},\"modules\":[", plugin.first_party);
            for (j, module) in plugin.modules.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                out.push_str("{\"name\":");
                string(out, module.id.name());
                out.push_str(",\"type\":");
                string(out, module.id.type_name());
                out.push_str(",\"commands\":");
                strings(out, module.commands.iter().copied());
                out.push_str(",\"contributions\":{");
                for (k, (name, count)) in module.contributions.iter().enumerate() {
                    if k > 0 {
                        out.push(',');
                    }
                    string(out, name);
                    let _ = write!(out, ":{count}");
                }
                out.push_str("},\"publishable\":{");
                for (k, (tag, count)) in module.publishable.iter().enumerate() {
                    if k > 0 {
                        out.push(',');
                    }
                    string(out, tag.as_str());
                    let _ = write!(out, ":{count}");
                }
                out.push_str("}}");
            }
            out.push_str("]}");
        }
        out.push(']');
    }
}

/// What `plugins` prints: one block per plugin, blocks parted by an empty
/// line. A block's first line is the namespace, the name and version, and the
/// title when there is one; then, per module, its name and type, the commands
/// it registers, what it adds (counts by short type name) and the files it
/// publishes, each line left out when empty:
///
/// ```text
/// announcements  navree-plugin-announcements 1.0.0  Announcements
///   AnnouncementsModule  navree_plugin_announcements::AnnouncementsModule
///     commands  announcements:prune
///     adds      1 ControllerRegistration, 1 MigrationSet, 2 Publishable
///     publishes 3 views, 2 locales
/// ```
impl fmt::Display for PluginsReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.plugins.is_empty() {
            return f.write_str("No plugins: no module of this app calls m.plugin(..).");
        }
        let mut lines: Vec<String> = Vec::new();
        for (i, plugin) in self.plugins.iter().enumerate() {
            if i > 0 {
                lines.push(String::new());
            }
            let info = &plugin.info;
            let mut head = format!("{}  {} {}", info.namespace, info.name, info.version);
            if let Some(title) = info.title {
                head.push_str("  ");
                head.push_str(title);
            }
            lines.push(head);
            for module in &plugin.modules {
                lines.push(format!("  {}  {}", module.id.name(), module.id.type_name()));
                if !module.commands.is_empty() {
                    lines.push(format!("    commands  {}", module.commands.join(", ")));
                }
                if !module.contributions.is_empty() {
                    let mut adds: Vec<(&str, usize)> = module
                        .contributions
                        .iter()
                        .map(|&(name, count)| (short_type_name(name), count))
                        .collect();
                    adds.sort_by_key(|&(name, _)| name);
                    let adds: Vec<String> = adds
                        .iter()
                        .map(|(name, count)| format!("{count} {name}"))
                        .collect();
                    lines.push(format!("    adds      {}", adds.join(", ")));
                }
                if !module.publishable.is_empty() {
                    let files: Vec<String> = module
                        .publishable
                        .iter()
                        .map(|(tag, count)| format!("{count} {}", tag.as_str()))
                        .collect();
                    lines.push(format!("    publishes {}", files.join(", ")));
                }
            }
        }
        f.write_str(&lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AnnouncementsModule;
    struct WeatherModule;

    /// Two plugins built by hand: `navree-plugin-announcements` with the
    /// documented values, and `navree-plugin-weather`, whose module adds
    /// nothing.
    fn announcements() -> PluginsReport {
        let info = PluginInfo::new(
            "navree-plugin-announcements",
            "navree_plugin_announcements",
            "announcements",
            "1.0.0",
        )
        .title("Announcements")
        .description("Notices for every member of a workspace, by Simran Kaur's team in Ludhiana.")
        .repository("https://code.ludhiana.example/navree-plugin-announcements")
        .license("MIT OR Apache-2.0");
        let weather = PluginInfo::new(
            "navree-plugin-weather",
            "navree_plugin_weather",
            "weather",
            "0.4.0",
        );
        PluginsReport {
            plugins: vec![
                PluginReport {
                    info,
                    first_party: false,
                    modules: vec![PluginModuleReport {
                        id: ModuleId::of::<AnnouncementsModule>(),
                        commands: vec!["announcements:prune"],
                        contributions: vec![
                            ("navree_config::registry::SectionRegistration", 1),
                            ("navree_contracts::permissions::Permissions", 1),
                            ("navree_http::controller::ControllerRegistration", 1),
                            ("navree_kernel::plugin::Publishable", 2),
                            ("navree_schema::migration::MigrationSet", 1),
                        ],
                        publishable: vec![(PublishTag::Views, 3), (PublishTag::Locales, 2)],
                    }],
                },
                PluginReport {
                    info: weather,
                    first_party: false,
                    modules: vec![PluginModuleReport {
                        id: ModuleId::of::<WeatherModule>(),
                        commands: Vec::new(),
                        contributions: Vec::new(),
                        publishable: Vec::new(),
                    }],
                },
            ],
        }
    }

    #[test]
    fn the_plugins_json_has_the_documented_shape() {
        let mut report = announcements();
        report.plugins.truncate(1);
        let json: serde_json::Value = serde_json::from_str(&report.to_json()).expect("JSON");
        assert_eq!(
            json,
            serde_json::json!({
                "navree": env!("CARGO_PKG_VERSION"),
                "plugins": [{
                    "name": "navree-plugin-announcements",
                    "namespace": "announcements",
                    "version": "1.0.0",
                    "title": "Announcements",
                    "description": "Notices for every member of a workspace, by Simran Kaur's team in Ludhiana.",
                    "homepage": null,
                    "repository": "https://code.ludhiana.example/navree-plugin-announcements",
                    "license": "MIT OR Apache-2.0",
                    "first_party": false,
                    "modules": [{
                        "name": "AnnouncementsModule",
                        "type": std::any::type_name::<AnnouncementsModule>(),
                        "commands": ["announcements:prune"],
                        "contributions": {
                            "navree_config::registry::SectionRegistration": 1,
                            "navree_contracts::permissions::Permissions": 1,
                            "navree_http::controller::ControllerRegistration": 1,
                            "navree_kernel::plugin::Publishable": 2,
                            "navree_schema::migration::MigrationSet": 1
                        },
                        "publishable": {"views": 3, "locales": 2}
                    }]
                }]
            })
        );
    }

    #[test]
    fn the_plugins_text_lists_commands_contributions_and_files() {
        let expected = format!(
            "announcements  navree-plugin-announcements 1.0.0  Announcements\n\
             \x20 AnnouncementsModule  {}\n\
             \x20   commands  announcements:prune\n\
             \x20   adds      1 ControllerRegistration, 1 MigrationSet, 1 Permissions, 2 Publishable, 1 SectionRegistration\n\
             \x20   publishes 3 views, 2 locales\n\
             \n\
             weather  navree-plugin-weather 0.4.0\n\
             \x20 WeatherModule  {}",
            std::any::type_name::<AnnouncementsModule>(),
            std::any::type_name::<WeatherModule>(),
        );
        assert_eq!(announcements().to_string(), expected);
        assert_eq!(
            PluginsReport::default().to_string(),
            "No plugins: no module of this app calls m.plugin(..)."
        );
    }
}
