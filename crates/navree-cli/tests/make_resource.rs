//! `navree make:resource <module> <Model>`: a Helm resource for a
//! navree-orm model, registered with its module.

use std::path::{Path, PathBuf};

use navree_cli::clock::Timestamp;
use navree_cli::make::{Companions, MakeKind, MakeOptions, make_item, make_model, make_module};
use navree_cli::names::{AppName, Name};
use navree_cli::new_app::{Database, NavreeSource, NewApp, Ui, create_app};
use navree_cli::resource::make_resource;

fn now() -> Timestamp {
    Timestamp::from_unix(1_792_938_605)
}

fn name(s: &str) -> Name {
    Name::parse("name", s).unwrap()
}

/// A new app with a CRUD `deals` module and its `Deal` model, made with a
/// policy (so the model is an access subject).
fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let app = NewApp {
        name: AppName::parse("crm").unwrap(),
        ui: Ui::None,
        kit: None,
        database: Database::default(),
        navree: NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree")),
    };
    let root = create_app(dir.path(), &app, now()).unwrap();
    make_module(&root, "src/modules", &name("deals"), true, now()).unwrap();
    let policy = Companions {
        policy: true,
        ..Companions::default()
    };
    make_model(
        &root,
        "src/modules",
        &name("deals"),
        &name("deal"),
        policy,
        now(),
    )
    .unwrap();
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

fn shown(root: &Path, paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect()
}

#[test]
fn make_resource_writes_the_resource_and_registers_it() {
    let (_dir, root) = project();
    let report = make_resource(&root, "src/modules", &name("deals"), "Deal").unwrap();
    // What the CLI prints, in order (the docs show these lines).
    assert_eq!(
        shown(&root, &report.created),
        [
            "src/modules/deals/resources/deal.rs",
            "src/modules/deals/resources/mod.rs"
        ]
    );
    assert_eq!(
        shown(&root, &report.updated),
        ["src/modules/deals/mod.rs", "src/modules/deals/mod.rs"]
    );
    assert!(report.hints.is_empty(), "{:?}", report.hints);

    let module = read(&root, "src/modules/deals/mod.rs");
    assert!(module.contains("pub mod resources;"), "{module}");
    assert!(
        module.contains(
            "        navree::helm::ModuleBuilderHelmExt::resource::<resources::deal::DealResource>(m);\n        // navree:wire"
        ),
        "{module}"
    );
    assert_eq!(
        read(&root, "src/modules/deals/resources/mod.rs"),
        "//! Helm resources of the deals module.\n\npub mod deal;\n"
    );
    let resource = read(&root, "src/modules/deals/resources/deal.rs");
    insta::assert_snapshot!("make_resource_deal", resource);
}

#[test]
fn the_model_may_be_written_in_snake_case() {
    let (_dir, root) = project();
    make_resource(&root, "src/modules", &name("deals"), "deal").unwrap();
    let resource = read(&root, "src/modules/deals/resources/deal.rs");
    assert!(resource.contains("pub struct DealResource;"), "{resource}");
    assert!(resource.contains("type Model = Deal;"), "{resource}");
}

#[test]
fn a_second_resource_joins_the_resources_module() {
    let (_dir, root) = project();
    make_resource(&root, "src/modules", &name("deals"), "Deal").unwrap();
    make_item(
        &root,
        "src/modules",
        MakeKind::Model,
        &name("deals"),
        &name("deal_stage"),
        &MakeOptions::default(),
        now(),
    )
    .unwrap();
    let report = make_resource(&root, "src/modules", &name("deals"), "DealStage").unwrap();
    assert_eq!(
        shown(&root, &report.created),
        ["src/modules/deals/resources/deal_stage.rs"]
    );
    assert_eq!(
        read(&root, "src/modules/deals/resources/mod.rs"),
        "//! Helm resources of the deals module.\n\npub mod deal;\npub mod deal_stage;\n"
    );
    let resource = read(&root, "src/modules/deals/resources/deal_stage.rs");
    assert!(
        resource.contains("use super::super::models::deal_stage::DealStage;"),
        "{resource}"
    );
    assert!(
        resource.contains("const LABEL: &'static str = \"Deal stages\";"),
        "{resource}"
    );
    // The made model is no access subject yet: a hint says how to make it one.
    assert_eq!(report.hints.len(), 1, "{:?}", report.hints);
    assert!(
        report.hints[0].contains("add `subject` to its `#[model(..)]` attribute"),
        "{:?}",
        report.hints
    );
    assert!(
        read(&root, "src/modules/deals/mod.rs")
            .contains("resources::deal_stage::DealStageResource")
    );
}

#[test]
fn missing_modules_models_and_existing_files_are_errors() {
    let (_dir, root) = project();
    let error = make_resource(&root, "src/modules", &name("invoices"), "Invoice").unwrap_err();
    assert!(
        error.to_string().contains("module `invoices` not found"),
        "{error}"
    );
    let error = make_resource(&root, "src/modules", &name("deals"), "Contact").unwrap_err();
    assert!(
        error.to_string().contains("model `contact` not found")
            && error.to_string().contains("navree make:model deals contact"),
        "{error}"
    );
    let error = make_resource(&root, "src/modules", &name("deals"), "123").unwrap_err();
    assert!(error.to_string().contains("invalid model name"), "{error}");
    make_resource(&root, "src/modules", &name("deals"), "Deal").unwrap();
    let module = read(&root, "src/modules/deals/mod.rs");
    let error = make_resource(&root, "src/modules", &name("deals"), "Deal").unwrap_err();
    assert!(
        error.to_string().contains("refusing to overwrite"),
        "{error}"
    );
    assert_eq!(read(&root, "src/modules/deals/mod.rs"), module);
}
