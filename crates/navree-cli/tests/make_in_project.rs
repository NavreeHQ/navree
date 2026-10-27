//! `navree make:*` inside a project created by `navree new`: files are created
//! and wired into the app.

use std::path::{Path, PathBuf};

use navree_cli::clock::Timestamp;
use navree_cli::make::{MakeKind, MakeOptions, make_item, make_module};
use navree_cli::names::{AppName, Name};
use navree_cli::new_app::{NavreeSource, NewApp, Ui, create_app};

fn now() -> Timestamp {
    Timestamp::from_unix(1_792_938_605)
}

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let app = NewApp {
        name: AppName::parse("blog").unwrap(),
        ui: Ui::None,
        navree: NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree")),
    };
    let root = create_app(dir.path(), &app, now()).unwrap();
    (dir, root)
}

fn read(root: &Path, relative: &str) -> String {
    std::fs::read_to_string(
        relative
            .split('/')
            .fold(root.to_path_buf(), |p, s| p.join(s)),
    )
    .unwrap()
}

fn name(s: &str) -> Name {
    Name::parse("name", s).unwrap()
}

#[test]
fn make_module_registers_the_module_with_the_app() {
    let (_dir, root) = project();
    let report = make_module(&root, "src/modules", &name("posts"), true, now()).unwrap();
    assert_eq!(report.created.len(), 8);
    assert!(report.hints.is_empty(), "{:?}", report.hints);
    assert!(read(&root, "src/modules/posts/controller.rs").contains("#[controller(\"/posts\""));
    let modules = read(&root, "src/modules/mod.rs");
    assert!(
        modules.contains("pub mod posts;\npub mod users;\n"),
        "{modules}"
    );
    let app = read(&root, "src/app.rs");
    assert!(
        app.contains("        .module(modules::users::UsersModule)\n        .module(modules::posts::PostsModule)\n    // navree:modules\n"),
        "{app}"
    );
}

#[test]
fn make_module_twice_fails_without_touching_anything() {
    let (_dir, root) = project();
    make_module(&root, "src/modules", &name("posts"), false, now()).unwrap();
    let app_before = read(&root, "src/app.rs");
    let error = make_module(&root, "src/modules", &name("posts"), true, now()).unwrap_err();
    assert!(
        error.to_string().contains("refusing to overwrite"),
        "{error}"
    );
    assert_eq!(read(&root, "src/app.rs"), app_before);
}

#[test]
fn make_module_outside_a_navree_app_prints_hints() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let report = make_module(dir.path(), "src/modules", &name("posts"), false, now()).unwrap();
    assert!(dir.path().join("src/modules/posts/mod.rs").exists());
    assert_eq!(report.hints.len(), 2, "{:?}", report.hints);
    assert!(
        report.hints[0].contains("pub mod posts;"),
        "{:?}",
        report.hints
    );
    assert!(
        report.hints[1].contains(".module(modules::posts::PostsModule)"),
        "{:?}",
        report.hints
    );
}

#[test]
fn make_job_wires_it_into_the_module() {
    let (_dir, root) = project();
    let options = MakeOptions::default();
    let report = make_item(
        &root,
        "src/modules",
        MakeKind::Job,
        &name("users"),
        &name("send_welcome"),
        &options,
        now(),
    )
    .unwrap();
    assert!(report.hints.is_empty(), "{:?}", report.hints);
    let job = read(&root, "src/modules/users/jobs/send_welcome.rs");
    assert!(job.contains("pub struct SendWelcome"), "{job}");
    assert!(
        job.contains("const NAME: &'static str = \"users.send_welcome\";"),
        "{job}"
    );
    assert_eq!(
        read(&root, "src/modules/users/jobs/mod.rs"),
        "//! Queued jobs of the users module.\n\npub mod send_welcome;\n"
    );
    let module = read(&root, "src/modules/users/mod.rs");
    assert!(module.contains("pub mod jobs;\n"), "{module}");
    assert!(
        module.contains("        navree::contracts::ModuleBuilderJobsExt::job::<jobs::send_welcome::SendWelcome>(m);\n        // navree:wire\n"),
        "{module}"
    );

    // A second job joins the same `jobs` module, in name order.
    make_item(
        &root,
        "src/modules",
        MakeKind::Job,
        &name("users"),
        &name("purge"),
        &options,
        now(),
    )
    .unwrap();
    assert_eq!(
        read(&root, "src/modules/users/jobs/mod.rs"),
        "//! Queued jobs of the users module.\n\npub mod purge;\npub mod send_welcome;\n"
    );
    assert_eq!(
        read(&root, "src/modules/users/mod.rs")
            .matches("pub mod jobs;")
            .count(),
        1
    );
}

#[test]
fn make_migration_adds_it_to_the_module_set() {
    let (_dir, root) = project();
    let report = make_item(
        &root,
        "src/modules",
        MakeKind::Migration,
        &name("users"),
        &name("create_avatars_table"),
        &MakeOptions::default(),
        Timestamp::from_unix(1_792_938_700),
    )
    .unwrap();
    assert!(report.hints.is_empty(), "{:?}", report.hints);
    let file = "m20261025_143140_create_avatars_table";
    let migration = read(&root, &format!("src/modules/users/migrations/{file}.rs"));
    assert!(migration.contains(".table(Avatars::Table)"), "{migration}");
    let set = read(&root, "src/modules/users/migrations/mod.rs");
    assert!(
        set.contains(&format!(
            "mod m20261025_143005_create_user_profiles_table;\nmod {file};\n"
        )),
        "{set}"
    );
    assert!(
        set.contains(&format!(
            "        .migration(m20261025_143005_create_user_profiles_table::Migration)\n        .migration({file}::Migration)\n"
        )),
        "{set}"
    );
}

#[test]
fn every_kind_generates_into_its_directory() {
    let (_dir, root) = project();
    let cases = [
        (
            MakeKind::Controller,
            "admin",
            "src/modules/users/controllers/admin.rs",
        ),
        (
            MakeKind::Service,
            "pricing",
            "src/modules/users/services/pricing.rs",
        ),
        (
            MakeKind::Entity,
            "avatar",
            "src/modules/users/entities/avatar.rs",
        ),
        (
            MakeKind::Event,
            "user_promoted",
            "src/modules/users/events/user_promoted.rs",
        ),
        (
            MakeKind::Listener,
            "notify_admins",
            "src/modules/users/listeners/notify_admins.rs",
        ),
        (
            MakeKind::Mail,
            "welcome",
            "src/modules/users/mail/welcome.rs",
        ),
        (
            MakeKind::Notification,
            "profile_updated",
            "src/modules/users/notifications/profile_updated.rs",
        ),
        (
            MakeKind::Policy,
            "profile",
            "src/modules/users/policies/profile.rs",
        ),
        (
            MakeKind::Command,
            "prune_profiles",
            "src/modules/users/commands/prune_profiles.rs",
        ),
        (
            MakeKind::Test,
            "profile_rules",
            "src/modules/users/tests/profile_rules.rs",
        ),
        (
            MakeKind::Mcp,
            "assistant",
            "src/modules/users/mcp/assistant.rs",
        ),
    ];
    for (kind, item, path) in cases {
        make_item(
            &root,
            "src/modules",
            kind,
            &name("users"),
            &name(item),
            &MakeOptions::default(),
            now(),
        )
        .unwrap_or_else(|e| panic!("{kind:?}: {e:#}"));
        assert!(read(&root, path).ends_with('\n'), "{path}");
    }
    let module = read(&root, "src/modules/users/mod.rs");
    for line in [
        "navree::http::ModuleBuilderHttpExt::controller::<controllers::admin::AdminController>(m);",
        "m.provider::<services::pricing::PricingService>();",
        "navree::view::ModuleBuilderViewExt::templates(m, mail::welcome::TEMPLATES);",
        "m.command(commands::prune_profiles::command());",
        "navree::mcp::ModuleBuilderMcpExt::mcp_server::<mcp::assistant::AssistantMcp>(m);",
    ] {
        assert!(module.contains(line), "missing `{line}` in\n{module}");
    }
    assert!(read(&root, "src/modules/users/mail/welcome.html").contains("{{ name }}"));
    assert!(read(&root, "src/modules/users/mail/welcome.txt").contains("{{ name }}"));
    assert!(read(&root, "src/modules/users/tests/mod.rs").contains("\nmod profile_rules;\n"));
    let abilities = read(&root, "src/abilities.rs");
    assert!(
        abilities.contains("    crate::modules::users::policies::profile::define(ctx, rules);\n    // navree:policies\n"),
        "{abilities}"
    );
}

#[test]
fn listener_with_event_subscribes_it() {
    let (_dir, root) = project();
    let options = MakeOptions::default();
    make_item(
        &root,
        "src/modules",
        MakeKind::Event,
        &name("users"),
        &name("user_promoted"),
        &options,
        now(),
    )
    .unwrap();
    let with_event = MakeOptions {
        event: Some(name("user_promoted")),
    };
    make_item(
        &root,
        "src/modules",
        MakeKind::Listener,
        &name("users"),
        &name("notify_admins"),
        &with_event,
        now(),
    )
    .unwrap();
    let listener = read(&root, "src/modules/users/listeners/notify_admins.rs");
    assert!(
        listener.contains("impl Listener<UserPromoted> for NotifyAdmins"),
        "{listener}"
    );
    let module = read(&root, "src/modules/users/mod.rs");
    assert!(
        module.contains(
            "        navree::events::ModuleBuilderEventsExt::listen::<\n            events::user_promoted::UserPromoted,\n            listeners::notify_admins::NotifyAdmins,\n        >(m);\n        // navree:wire\n"
        ),
        "{module}"
    );
}

#[test]
fn make_mcp_generates_a_server_and_registers_it() {
    let (_dir, root) = project();
    let report = make_item(
        &root,
        "src/modules",
        MakeKind::Mcp,
        &name("users"),
        &name("crm"),
        &MakeOptions::default(),
        now(),
    )
    .unwrap();
    let server = read(&root, "src/modules/users/mcp/crm.rs");
    for expected in [
        "use navree::mcp::{McpContext, mcp_server};",
        "#[injectable]\npub struct CrmMcp;",
        "#[mcp_server]\nimpl CrmMcp {",
        "    #[mcp_tool(read_only, idempotent)]\n    #[authenticated]\n    async fn crm_echo(",
        "#[derive(Debug, Deserialize, ToSchema, Validate)]\npub struct CrmEchoArgs {",
    ] {
        assert!(
            server.contains(expected),
            "missing `{expected}` in\n{server}"
        );
    }
    assert_eq!(
        read(&root, "src/modules/users/mcp/mod.rs"),
        "//! MCP servers of the users module.\n\npub mod crm;\n"
    );
    let module = read(&root, "src/modules/users/mod.rs");
    assert!(module.contains("pub mod mcp;\n"), "{module}");
    assert!(
        module.contains(
            "        navree::mcp::ModuleBuilderMcpExt::mcp_server::<mcp::crm::CrmMcp>(m);\n        // navree:wire\n"
        ),
        "{module}"
    );
    // What the generator does not edit is left as steps.
    let hints = report.hints.join("\n");
    for step in [
        "features = [\"mcp\"]",
        "utoipa",
        ".module(McpModule::from_config())",
    ] {
        assert!(hints.contains(step), "missing `{step}` in\n{hints}");
    }
}

#[test]
fn make_into_a_missing_module_fails() {
    let (_dir, root) = project();
    let error = make_item(
        &root,
        "src/modules",
        MakeKind::Job,
        &name("billing"),
        &name("charge"),
        &MakeOptions::default(),
        now(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("navree make:module billing"),
        "{error}"
    );
}

#[test]
fn existing_item_is_not_overwritten() {
    let (_dir, root) = project();
    let options = MakeOptions::default();
    make_item(
        &root,
        "src/modules",
        MakeKind::Service,
        &name("users"),
        &name("pricing"),
        &options,
        now(),
    )
    .unwrap();
    let error = make_item(
        &root,
        "src/modules",
        MakeKind::Service,
        &name("users"),
        &name("pricing"),
        &options,
        now(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("refusing to overwrite"),
        "{error}"
    );
}
