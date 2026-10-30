//! The `introspect --json` built-in: a [`GraphReport`] as JSON, for tools
//! that look at an app from outside (Navigator, `navree mcp`).

use std::fmt::Write;

use crate::GraphReport;
use crate::command::BUILT_INS;

impl GraphReport {
    /// The report as one JSON object, what `introspect --json` prints:
    ///
    /// ```json
    /// {
    ///   "navree": "0.2.0",
    ///   "modules": [
    ///     {"name": "DealsModule", "global": false, "root": false,
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
    /// true`. `commands` lists the built-ins (`module: null`), then every
    /// module's commands. Nothing is constructed to produce it.
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
        let built_ins = BUILT_INS.iter().map(|(usage, about)| {
            let name = usage.split_whitespace().next().unwrap_or(usage);
            (name, *about, None)
        });
        let registered = self.modules.iter().flat_map(|m| {
            m.commands
                .iter()
                .map(move |c| (c.name, c.about, Some(m.name)))
        });
        for (i, (name, about, module)) in built_ins.chain(registered).enumerate() {
            if i > 0 {
                out.push(',');
            }
            command_object(&mut out, name, about, Some(module));
        }
        out.push_str("]}");
        out
    }
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
fn strings<'a>(out: &mut String, items: impl Iterator<Item = &'a str>) {
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
fn string(out: &mut String, s: &str) {
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
