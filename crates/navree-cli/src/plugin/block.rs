//! Rebuilds a single plugin's block of `plugins` from `plugins --json` output.

#![allow(dead_code)]

use serde_json::Value;

const KNOWN_PUBLISH_TAGS: &[&str] = &["views", "locales", "files"];

// Strips the module path from a type name while keeping generic arguments:
// `app::users::UsersModule` becomes `UsersModule`, `a::Wrap<b::C>` becomes `Wrap<b::C>`.
fn short_type_name(full: &str) -> &str {
    let generics = full.find('<').unwrap_or(full.len());
    match full[..generics].rfind("::") {
        Some(pos) => &full[pos + 2..],
        None => full,
    }
}

/// The block `plugins` prints for `package`, rebuilt from `plugins --json`;
/// `None` when the JSON does not parse or has no such plugin.
pub(crate) fn render(json: &str, package: &str) -> Option<String> {
    let doc: Value = serde_json::from_str(json).ok()?;
    let plugins = doc.get("plugins")?.as_array()?;
    let wanted = package.replace('-', "_");
    let plugin = plugins.iter().find(|entry| {
        entry
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == package || name.replace('-', "_") == wanted)
    })?;

    let namespace = plugin.get("namespace")?.as_str()?;
    let name = plugin.get("name")?.as_str()?;
    let version = plugin.get("version")?.as_str()?;

    let mut lines = Vec::new();
    let mut head = format!("{namespace}  {name} {version}");
    if let Some(title) = plugin.get("title").and_then(Value::as_str) {
        if !title.is_empty() {
            head.push_str("  ");
            head.push_str(title);
        }
    }
    lines.push(head);

    if let Some(modules) = plugin.get("modules").and_then(Value::as_array) {
        for module in modules {
            let mod_name = module.get("name")?.as_str()?;
            let mod_type = module.get("type")?.as_str()?;
            lines.push(format!("  {mod_name}  {mod_type}"));

            if let Some(commands) = module.get("commands").and_then(Value::as_array) {
                let mut names = Vec::with_capacity(commands.len());
                for cmd in commands {
                    names.push(cmd.as_str()?);
                }
                if !names.is_empty() {
                    lines.push(format!("    commands  {}", names.join(", ")));
                }
            }

            if let Some(contributions) = module.get("contributions").and_then(Value::as_object) {
                let mut adds = Vec::with_capacity(contributions.len());
                for (full_type, count_val) in contributions {
                    let count = count_val.as_u64()?;
                    if count > 0 {
                        adds.push((short_type_name(full_type), count));
                    }
                }
                if !adds.is_empty() {
                    adds.sort_by_key(|&(short, _)| short);
                    let formatted: Vec<String> = adds
                        .into_iter()
                        .map(|(short, count)| format!("{count} {short}"))
                        .collect();
                    lines.push(format!("    adds      {}", formatted.join(", ")));
                }
            }

            if let Some(publishable) = module.get("publishable").and_then(Value::as_object) {
                let mut files = Vec::new();
                for &known in KNOWN_PUBLISH_TAGS {
                    if let Some(count_val) = publishable.get(known) {
                        let count = count_val.as_u64()?;
                        if count > 0 {
                            files.push(format!("{count} {known}"));
                        }
                    }
                }
                for (tag, count_val) in publishable {
                    if !KNOWN_PUBLISH_TAGS.contains(&tag.as_str()) {
                        let count = count_val.as_u64()?;
                        if count > 0 {
                            files.push(format!("{count} {tag}"));
                        }
                    }
                }
                if !files.is_empty() {
                    lines.push(format!("    publishes {}", files.join(", ")));
                }
            }
        }
    }

    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn plugin_add_prints_the_plugins_block_as_plugins_prints_it() {
        let sample = r#"{
  "navree": "1.4.0",
  "plugins": [
    {
      "name": "navree-plugin-announcements",
      "namespace": "announcements",
      "version": "1.0.0",
      "title": "Announcements",
      "description": "Notices for every member of a workspace, by Simran Kaur's team in Ludhiana.",
      "homepage": null,
      "repository": "https://code.ludhiana.example/navree-plugin-announcements",
      "license": "MIT OR Apache-2.0",
      "first_party": false,
      "extra_future_key": true,
      "modules": [
        {
          "name": "AnnouncementsModule",
          "type": "navree_plugin_announcements::AnnouncementsModule",
          "commands": ["announcements:prune"],
          "contributions": {
            "navree_config::registry::SectionRegistration": 1,
            "navree_contracts::permissions::Permissions": 1,
            "navree_http::controller::ControllerRegistration": 1,
            "navree_kernel::plugin::Publishable": 2,
            "navree_schema::migration::MigrationSet": 1
          },
          "publishable": {"views": 3, "locales": 2},
          "extra_module_key": "ignored"
        }
      ]
    }
  ]
}"#;

        let expected = [
            "announcements  navree-plugin-announcements 1.0.0  Announcements",
            "  AnnouncementsModule  navree_plugin_announcements::AnnouncementsModule",
            "    commands  announcements:prune",
            "    adds      1 ControllerRegistration, 1 MigrationSet, 1 Permissions, 2 Publishable, 1 SectionRegistration",
            "    publishes 3 views, 2 locales",
        ]
        .join("\n");
        assert_eq!(
            render(sample, "navree-plugin-announcements").as_deref(),
            Some(expected.as_str())
        );

        let rich = json!({
            "navree": "1.4.0",
            "plugins": [
                {
                    "name": "navree-plugin-khata",
                    "namespace": "khata",
                    "version": "0.2.0",
                    "title": null,
                    "modules": [
                        {
                            "name": "PolicyOf<x::Open>",
                            "type": "plugins::PolicyOf<x::Open>",
                            "commands": [],
                            "contributions": {
                                "plugins::PolicyOf<x::Open>": 1
                            },
                            "publishable": {}
                        },
                        {
                            "name": "EmptyModule",
                            "type": "navree_plugin_khata::EmptyModule",
                            "commands": [],
                            "contributions": {},
                            "publishable": {}
                        },
                        {
                            "name": "AssetsModule",
                            "type": "navree_plugin_khata::AssetsModule",
                            "commands": [],
                            "contributions": {},
                            "publishable": { "files": 1, "views": 2 }
                        }
                    ]
                }
            ]
        });
        let expected_rich = [
            "khata  navree-plugin-khata 0.2.0",
            "  PolicyOf<x::Open>  plugins::PolicyOf<x::Open>",
            "    adds      1 PolicyOf<x::Open>",
            "  EmptyModule  navree_plugin_khata::EmptyModule",
            "  AssetsModule  navree_plugin_khata::AssetsModule",
            "    publishes 2 views, 1 files",
        ]
        .join("\n");
        assert_eq!(
            render(&rich.to_string(), "navree-plugin-khata").as_deref(),
            Some(expected_rich.as_str())
        );

        assert_eq!(render(sample, "navree-plugin-khata"), None);
        assert_eq!(render("not json", "navree-plugin-announcements"), None);
    }
}
