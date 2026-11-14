//! The `introspect --json` built-in: a [`GraphReport`] as JSON, for tools
//! that look at an app from outside (Navigator, `navree mcp`).

use std::fmt::Write;

use crate::command::built_ins;
use crate::{GraphReport, PluginsReport};

impl GraphReport {
    /// The report as one JSON object, what `introspect --json` prints:
    ///
    /// ```json
    /// {
    ///   "navree": "1.0.0",
    ///   "modules": [
    ///     {"name": "DealsModule", "type": "crm::DealsModule", "global": false, "root": false,
    ///      "imports": ["DatabaseModule"], "exports": ["crm::DealService"],
    ///      "providers": [{"type": "crm::DealService", "scope": "singleton",
    ///                     "exported": true, "multi": false, "dependencies": ["navree_db::Db"]}],
    ///      "commands": [{"name": "crm:close", "about": "Close stale deals"}]}
    ///   ],
    ///   "commands": [{"name": "check", "about": "...", "module": null}, ...]
    /// }
    /// ```
    ///
    /// Modules come in dependency order (imports first); the implicit app
    /// root, which imports the app's root modules, is last with `"root":
    /// true`. A module's `type` is its full type path
    /// ([`ModuleId::type_name`](crate::ModuleId::type_name)): for reading, not
    /// for matching. `commands` lists the built-ins (`module: null`), then
    /// every module's commands; a built-in that yields to a module command of
    /// its name (`plugins`, `plugin:publish`) is left out. Nothing is
    /// constructed to produce it.
    /// The `introspect` command adds a `"plugins"` array, as
    /// [`PluginsReport::to_json`] writes it.
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\"navree\":");
        string(&mut out, env!("CARGO_PKG_VERSION"));
        out.push_str(",\"modules\":[");
        let root = self.modules.len().saturating_sub(1);
        for (i, module) in self.modules.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            string(&mut out, module.name);
            out.push_str(",\"type\":");
            string(&mut out, module.id.type_name());
            let _ = write!(
                out,
                ",\"global\":{},\"root\":{},\"imports\":",
                module.global,
                i == root
            );
            strings(&mut out, module.imports.iter().copied());
            out.push_str(",\"exports\":");
            strings(&mut out, module.exports.iter().map(String::as_str));
            out.push_str(",\"providers\":[");
            for (j, provider) in module.providers.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                out.push_str("{\"type\":");
                string(&mut out, &provider.key);
                let _ = write!(
                    out,
                    ",\"scope\":\"{}\",\"exported\":{},\"multi\":{},\"dependencies\":",
                    provider.scope,
                    module.exports.contains(&provider.key),
                    provider.multi
                );
                strings(&mut out, provider.dependencies.iter().map(String::as_str));
                out.push('}');
            }
            out.push_str("],\"commands\":[");
            for (j, command) in module.commands.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                command_object(&mut out, command.name, command.about, None);
            }
            out.push_str("]}");
        }
        out.push_str("],\"commands\":[");
        let listed = built_ins(self).map(|(usage, about)| {
            let name = usage.split_whitespace().next().unwrap_or(usage);
            (name, about, None)
        });
        let registered = self.modules.iter().flat_map(|m| {
            m.commands
                .iter()
                .map(move |c| (c.name, c.about, Some(m.name)))
        });
        for (i, (name, about, module)) in listed.chain(registered).enumerate() {
            if i > 0 {
                out.push(',');
            }
            command_object(&mut out, name, about, Some(module));
        }
        out.push_str("]}");
        out
    }
}

/// What the `introspect` built-in prints: [`GraphReport::to_json`] with
/// `"plugins"` added to the outer object, the array of
/// [`PluginsReport::to_json`].
pub(crate) fn introspect_json(report: &GraphReport, plugins: &PluginsReport) -> String {
    let mut out = report.to_json();
    // `to_json` ends with the outer object's closing brace.
    let closing = out.pop();
    debug_assert_eq!(closing, Some('}'));
    out.push_str(",\"plugins\":");
    plugins.write_plugins_array(&mut out);
    out.push('}');
    out
}

/// `{"name":..,"about":..}`, plus `"module"` when `module` is given.
fn command_object(out: &mut String, name: &str, about: &str, module: Option<Option<&str>>) {
    out.push_str("{\"name\":");
    string(out, name);
    out.push_str(",\"about\":");
    string(out, about);
    if let Some(module) = module {
        out.push_str(",\"module\":");
        match module {
            Some(module) => string(out, module),
            None => out.push_str("null"),
        }
    }
    out.push('}');
}

/// A JSON array of strings.
pub(crate) fn strings<'a>(out: &mut String, items: impl Iterator<Item = &'a str>) {
    out.push('[');
    for (i, item) in items.enumerate() {
        if i > 0 {
            out.push(',');
        }
        string(out, item);
    }
    out.push(']');
}

/// A JSON string literal.
pub(crate) fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn introspect_adds_the_type_and_the_plugins_and_keeps_every_key() {
        struct Notices;
        impl crate::Module for Notices {
            fn register(&self, m: &mut crate::ModuleBuilder) {
                m.plugin(crate::plugin_info!(namespace = "announcements"));
            }
        }
        let roots: Vec<Box<dyn crate::Module>> = vec![Box::new(Notices)];
        let graph = crate::graph::Graph::build(&roots, &[]).expect("valid");
        let text = introspect_json(&graph.report(), &graph.plugins_report());
        let json: serde_json::Value = serde_json::from_str(&text).expect("JSON");
        let keys = |v: &serde_json::Value| {
            let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
            k.sort();
            k
        };
        assert_eq!(keys(&json), ["commands", "modules", "navree", "plugins"]);
        for module in json["modules"].as_array().unwrap() {
            assert_eq!(
                keys(module),
                [
                    "commands",
                    "exports",
                    "global",
                    "imports",
                    "name",
                    "providers",
                    "root",
                    "type",
                ]
            );
        }
        assert_eq!(json["modules"][0]["type"], std::any::type_name::<Notices>());
        let alone: serde_json::Value =
            serde_json::from_str(&graph.plugins_report().to_json()).unwrap();
        assert_eq!(json["plugins"], alone["plugins"]);
        let report_only: serde_json::Value =
            serde_json::from_str(&graph.report().to_json()).unwrap();
        assert!(
            report_only.get("plugins").is_none(),
            "GraphReport::to_json has no plugins"
        );
    }
}
