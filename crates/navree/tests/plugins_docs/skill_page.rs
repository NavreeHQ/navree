//! The `navree-plugins` skill and its index rows in `skills/README.md`,
//! `skills/navree/SKILL.md` and `docs/ai/agent-skills.mdx`.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_plugins_skill_teaches_every_page_and_every_registration_method() {
    let root = repo_root();
    let skill = std::fs::read_to_string(root.join("skills/navree-plugins/SKILL.md"))
        .expect("read skills/navree-plugins/SKILL.md");

    for slug in [
        "overview",
        "installing",
        "writing",
        "resources",
        "ui-components",
        "drivers",
        "existing-crates",
        "testing",
        "releasing",
    ] {
        let path = format!("/docs/plugins/{slug}/");
        assert!(
            skill.contains(&path),
            "skills/navree-plugins/SKILL.md must link {path}"
        );
    }
    assert!(
        !skill.contains("/docs/plugins/introduction/"),
        "skills/navree-plugins/SKILL.md must link /docs/plugins/overview/, not /docs/plugins/introduction/"
    );

    let headings = [
        "## When to use this skill",
        "## What a plugin is",
        "## Install, list and remove",
        "## Write one",
        "## Templates, translations and files",
        "## Drivers",
        "## Connecting an existing crate",
        "## Links, the admin panel and permissions",
        "## Testing",
        "## Releasing",
        "## Mistakes to avoid",
    ];
    let mut cursor = 0;
    for heading in headings {
        let pos = skill[cursor..].find(heading).unwrap_or_else(|| {
            panic!("skills/navree-plugins/SKILL.md missing `{heading}` in order")
        });
        cursor += pos + heading.len();
    }

    for method in [
        "m.cache_store",
        "m.queue_store",
        "m.session_store",
        "m.backplane",
        "m.mail_transport",
        "m.disk_driver",
        "m.notification_channel",
        "m.search_engine",
        "m.ai_provider",
    ] {
        assert!(
            skill.contains(method),
            "skills/navree-plugins/SKILL.md must name `{method}`"
        );
    }

    for app_line in [
        ".import(AccessModule::for_policy::<khata::InvoicePolicy>())",
        ".import(AccessModule::for_subject::<khata::Ledger>())",
        "AccessModule::for_root(..)",
        "m.multi(khata::permission_check())",
        "m.strategy::<khata::LedgerStrategy>()",
        "m.http_layer(khata::timeout())",
        "m.helm_staff_workspaces(khata::may_enter)",
        "m.channel(\"orders.{id}\", notices::authorize)",
        "m.workspace_scoped::<notices::Notice>()",
        "m.global_scope::<Deal>(\"open\", khata::open)",
        "m.on_model::<Deal, _>(ModelEvent::Saving, khata::stamp)",
    ] {
        assert!(
            skill.contains(app_line),
            "skills/navree-plugins/SKILL.md must name `{app_line}`"
        );
    }

    let closing = "A plugin's module adds no app-wide layer, configures no other crate's module, provides no permission check, authorizer, authenticator, strategy, workspace gate or shell, and hooks no other crate's model: ship a function and show the app's line.";
    assert!(
        skill.trim_end().ends_with(closing),
        "Mistakes to avoid in skills/navree-plugins/SKILL.md must end with the closing sentence"
    );

    for hidden in [
        "__private",
        "acts_for",
        "ClientModule::instances",
        "navree_storage::testing",
    ] {
        assert!(
            !skill.contains(hidden),
            "skills/navree-plugins/SKILL.md must not teach `{hidden}`"
        );
    }

    for index_file in [
        "skills/README.md",
        "skills/navree/SKILL.md",
        "docs/ai/agent-skills.mdx",
    ] {
        let text = std::fs::read_to_string(root.join(index_file))
            .unwrap_or_else(|err| panic!("read {index_file}: {err}"));
        let has_row = text
            .lines()
            .any(|line| line.trim_start().starts_with('|') && line.contains("navree-plugins"));
        assert!(
            has_row,
            "{index_file} must hold a table row for `navree-plugins`"
        );
    }
}
