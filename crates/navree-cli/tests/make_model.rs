//! `navree make:model`, `make:factory` and `make:seeder` inside a project
//! created by `navree new`: the model and its companions refer to each
//! other and are wired into the module.

use std::path::{Path, PathBuf};

use navree_cli::clock::Timestamp;
use navree_cli::make::{Companions, MakeKind, MakeOptions, make_item, make_model};
use navree_cli::names::{AppName, Name};
use navree_cli::new_app::{Database, NavreeSource, NewApp, Ui, create_app};

fn now() -> Timestamp {
    Timestamp::from_unix(1_792_938_605)
}

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let app = NewApp {
        name: AppName::parse("crm").unwrap(),
        ui: Ui::None,
        kit: None,
        database: Database::default(),
        navree: NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree")),
    };
    let root = create_app(dir.path(), &app, now()).unwrap();
    (dir, root)
}

fn path(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |p, s| p.join(s))
}

fn read(root: &Path, relative: &str) -> String {
    std::fs::read_to_string(path(root, relative)).unwrap()
}

fn name(s: &str) -> Name {
    Name::parse("name", s).unwrap()
}

fn shown(root: &Path, paths: &[PathBuf]) -> Vec<String> {
    let mut shown: Vec<String> = paths
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    shown.sort();
    shown
}

#[test]
fn a_model_alone_is_declared_and_hints_at_its_migration() {
    let (_dir, root) = project();
    let report = make_model(
        &root,
        "src/modules",
        &name("users"),
        &name("deal"),
        Companions::default(),
        now(),
    )
    .unwrap();
    assert_eq!(
        shown(&root, &report.created),
        [
            "src/modules/users/models/deal.rs",
            "src/modules/users/models/mod.rs"
        ]
    );
    let model = read(&root, "src/modules/users/models/deal.rs");
    assert!(
        model.contains(
            "#[derive(Model, Clone, Debug, Default, Serialize)]\n#[model(table = \"deals\", timestamps)]\npub struct Deal {\n"
        ),
        "{model}"
    );
    assert!(model.contains("    pub name: String,\n"), "{model}");
    assert!(
        model.contains("navree make:migration users create_deals_table"),
        "{model}"
    );
    assert!(!model.contains("factory"), "{model}");
    assert_eq!(
        read(&root, "src/modules/users/models/mod.rs"),
        "//! ORM models of the users module.\n\npub mod deal;\n"
    );
    let module = read(&root, "src/modules/users/mod.rs");
    assert!(module.contains("pub mod models;"), "{module}");
    assert!(
        module.contains(
            "        navree::orm::ModuleBuilderOrmExt::model::<models::deal::Deal>(m);\n        // navree:wire\n"
        ),
        "registered for model:show: {module}"
    );
    assert!(!module.contains("AccessModule::for_feature::<"), "{module}");
    assert_eq!(
        report.hints,
        ["create its table: navree make:migration users create_deals_table"]
    );
}

#[test]
fn every_companion_refers_to_the_others() {
    let (_dir, root) = project();
    let abilities_before = read(&root, "src/abilities.rs");
    let report = make_model(
        &root,
        "src/modules",
        &name("users"),
        &name("deal"),
        Companions::all(),
        now(),
    )
    .unwrap();
    let created = shown(&root, &report.created);
    for file in [
        "src/modules/users/models/deal.rs",
        "src/modules/users/migrations/m20261025_143005_create_deals_table.rs",
        "src/modules/users/factories/deal.rs",
        "src/modules/users/factories/mod.rs",
        "src/modules/users/seeders/deal.rs",
        "src/modules/users/policies/deal.rs",
        "src/modules/users/controllers/deal.rs",
    ] {
        assert!(created.iter().any(|c| c == file), "{file} in {created:?}");
    }

    let model = read(&root, "src/modules/users/models/deal.rs");
    assert!(
        model.contains(
            "#[model(\n    table = \"deals\",\n    timestamps,\n    factory = super::super::factories::deal::DealFactory,\n    subject,\n    policy = super::super::policies::deal::DealPolicy,\n)]\n"
        ),
        "{model}"
    );
    assert!(
        model.contains("The module's `create_deals_table` migration creates the table."),
        "{model}"
    );

    let migration = read(
        &root,
        "src/modules/users/migrations/m20261025_143005_create_deals_table.rs",
    );
    assert!(
        migration.contains(
            ".create(\"deals\", |t| {\n                t.uuid_primary();\n                t.string(\"name\");\n                t.timestamps_tz();\n"
        ),
        "{migration}"
    );
    let migrations = read(&root, "src/modules/users/migrations/mod.rs");
    assert!(
        migrations.contains("m20261025_143005_create_deals_table"),
        "{migrations}"
    );

    let factory = read(&root, "src/modules/users/factories/deal.rs");
    assert!(
        factory.contains("#[derive(Default)]\npub struct DealFactory;\n"),
        "{factory}"
    );
    assert!(factory.contains("    type Model = Deal;\n"), "{factory}");
    assert!(
        factory.contains("            name: ctx.fake().title(),\n"),
        "{factory}"
    );
    assert!(
        factory.contains("use super::super::models::deal::Deal;"),
        "{factory}"
    );
    assert!(!factory.contains("Link it to the model"), "{factory}");

    let seeder = read(&root, "src/modules/users/seeders/deal.rs");
    assert!(seeder.contains("pub struct DealSeeder;"), "{seeder}");
    assert!(
        seeder.contains("Deal::factory().count(10).create(db).await?;"),
        "{seeder}"
    );

    let policy = read(&root, "src/modules/users/policies/deal.rs");
    assert!(policy.contains("impl Policy for DealPolicy {"), "{policy}");
    assert!(policy.contains("rules.can(READ, \"Deal\");"), "{policy}");
    assert!(policy.contains("`deals.create`"), "{policy}");
    assert_eq!(
        read(&root, "src/abilities.rs"),
        abilities_before,
        "a model's policy is attached to the model, not to the abilities function"
    );

    let controller = read(&root, "src/modules/users/controllers/deal.rs");
    for expected in [
        "#[controller(\"/deals\", version = \"1\")]\nimpl DealController {",
        "#[can(Read, Deal)]",
        "scope: Accessible<Deal, Read>,",
        "Ok(scope.query().latest().paginate(&uow, page).await?)",
        "#[get(\"/{deal}\")]\n    #[can(Read, Deal, from = \"deal\")]",
        "deal: Authorized<Deal, Update>,",
        "#[can(Delete, Deal, from = \"deal\")]",
        "use super::super::models::deal::Deal;",
    ] {
        assert!(controller.contains(expected), "{expected}\n{controller}");
    }
    assert!(!controller.contains("#[authenticated]"), "{controller}");

    let module = read(&root, "src/modules/users/mod.rs");
    for expected in [
        "pub mod models;",
        "pub mod factories;",
        "pub mod seeders;",
        "pub mod policies;",
        "pub mod controllers;",
        "        navree::orm::ModuleBuilderOrmExt::model::<models::deal::Deal>(m);\n        m.import(navree::access::AccessModule::for_feature::<\n            models::deal::Deal,\n        >());\n",
        "        navree::db::ModuleBuilderDbExt::seeder::<seeders::deal::DealSeeder>(m);\n",
        "        navree::http::ModuleBuilderHttpExt::controller::<controllers::deal::DealController>(m);\n",
    ] {
        assert!(module.contains(expected), "{expected}\n{module}");
    }
    assert!(
        !report
            .hints
            .iter()
            .any(|h| h.contains("make:migration") || h.contains("link it")),
        "{:?}",
        report.hints
    );
    assert!(
        report
            .hints
            .iter()
            .any(|h| h.contains("navree db:seed deal")),
        "{:?}",
        report.hints
    );
}

#[test]
fn a_controller_without_a_policy_binds_records_for_signed_in_users() {
    let (_dir, root) = project();
    make_model(
        &root,
        "src/modules",
        &name("users"),
        &name("line_item"),
        Companions {
            controller: true,
            ..Companions::default()
        },
        now(),
    )
    .unwrap();
    let controller = read(&root, "src/modules/users/controllers/line_item.rs");
    for expected in [
        "#[controller(\"/line-items\", version = \"1\")]\n#[authenticated]\nimpl LineItemController {",
        "Ok(LineItem::query().latest().paginate(&uow, page).await?)",
        "#[get(\"/{line_item}\")]",
        "async fn show(&self, line_item: Bound<LineItem>) -> Json<Record<LineItem>> {",
        "line_item: Bound<LineItem>,",
        "let location = format!(\"/api/v1/line-items/{}\", line_item.id);",
        "#[api(summary = \"List line items\", tag = \"LineItem\")]",
        "#[api(summary = \"Show a line item\", tag = \"LineItem\")]",
    ] {
        assert!(controller.contains(expected), "{expected}\n{controller}");
    }
    assert!(!controller.contains("#[can("), "{controller}");
    let model = read(&root, "src/modules/users/models/line_item.rs");
    assert!(
        model.contains("#[model(table = \"line_items\", timestamps)]"),
        "{model}"
    );
}

#[test]
fn a_factory_for_an_existing_model_says_how_to_link_it() {
    let (_dir, root) = project();
    make_model(
        &root,
        "src/modules",
        &name("users"),
        &name("deal"),
        Companions::default(),
        now(),
    )
    .unwrap();
    let options = MakeOptions {
        model: Some(name("deal")),
        ..MakeOptions::default()
    };
    let report = make_item(
        &root,
        "src/modules",
        MakeKind::Factory,
        &name("users"),
        &name("vip_deal"),
        &options,
        now(),
    )
    .unwrap();
    let factory = read(&root, "src/modules/users/factories/vip_deal.rs");
    assert!(factory.contains("pub struct VipDealFactory;"), "{factory}");
    assert!(factory.contains("    type Model = Deal;\n"), "{factory}");
    assert!(
        factory.contains("name: ctx.fake().title(),"),
        "the model has a name column: {factory}"
    );
    assert!(
        factory.contains("navree::orm::factory::Factory::<VipDealFactory>::new()"),
        "{factory}"
    );
    assert_eq!(report.hints.len(), 1, "{:?}", report.hints);
    assert!(
        report.hints[0].contains("factory = super::super::factories::vip_deal::VipDealFactory"),
        "{:?}",
        report.hints
    );
    assert!(
        report.hints[0].contains("models/deal.rs"),
        "{:?}",
        report.hints
    );
}

#[test]
fn a_factory_of_a_model_without_a_name_column_leaves_the_definition_to_fill() {
    let (_dir, root) = project();
    let models = path(&root, "src/modules/users/models");
    std::fs::create_dir_all(&models).unwrap();
    std::fs::write(
        models.join("visit.rs"),
        "#[derive(Model, Clone, Debug, Default)]\npub struct Visit {\n    pub id: i64,\n    pub path: String,\n}\n",
    )
    .unwrap();
    make_item(
        &root,
        "src/modules",
        MakeKind::Factory,
        &name("users"),
        &name("visit"),
        &MakeOptions::default(),
        now(),
    )
    .unwrap();
    let factory = read(&root, "src/modules/users/factories/visit.rs");
    assert!(!factory.contains("name: ctx.fake()"), "{factory}");
    assert!(factory.contains("        Visit::default()\n"), "{factory}");
}

#[test]
fn a_factory_needs_its_model() {
    let (_dir, root) = project();
    let error = make_item(
        &root,
        "src/modules",
        MakeKind::Factory,
        &name("users"),
        &name("ghost"),
        &MakeOptions::default(),
        now(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("model `ghost` not found"), "{error}");
    assert!(error.contains("navree make:model users ghost"), "{error}");
    assert!(!path(&root, "src/modules/users/factories").exists());
}

#[test]
fn a_seeder_is_registered_for_db_seed() {
    let (_dir, root) = project();
    let report = make_item(
        &root,
        "src/modules",
        MakeKind::Seeder,
        &name("users"),
        &name("demo_data"),
        &MakeOptions::default(),
        now(),
    )
    .unwrap();
    let seeder = read(&root, "src/modules/users/seeders/demo_data.rs");
    assert!(seeder.contains("pub struct DemoDataSeeder;"), "{seeder}");
    assert!(seeder.contains("        \"demo_data\"\n"), "{seeder}");
    assert!(
        seeder.contains("// `Post::factory().count(10).create(db).await?;`"),
        "{seeder}"
    );
    assert_eq!(
        read(&root, "src/modules/users/seeders/mod.rs"),
        "//! Seeders of the users module.\n\npub mod demo_data;\n"
    );
    let module = read(&root, "src/modules/users/mod.rs");
    assert!(
        module.contains(
            "        navree::db::ModuleBuilderDbExt::seeder::<seeders::demo_data::DemoDataSeeder>(m);\n        // navree:wire\n"
        ),
        "{module}"
    );
    assert!(
        report
            .hints
            .iter()
            .any(|h| h.contains("navree db:seed demo_data")),
        "{:?}",
        report.hints
    );
}

#[test]
fn a_model_is_not_generated_twice() {
    let (_dir, root) = project();
    make_model(
        &root,
        "src/modules",
        &name("users"),
        &name("deal"),
        Companions::default(),
        now(),
    )
    .unwrap();
    let module_before = read(&root, "src/modules/users/mod.rs");
    let error = make_model(
        &root,
        "src/modules",
        &name("users"),
        &name("deal"),
        Companions::all(),
        now(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("refusing to overwrite"),
        "{error}"
    );
    assert_eq!(read(&root, "src/modules/users/mod.rs"), module_before);
    assert!(!path(&root, "src/modules/users/factories").exists());
}

#[test]
fn a_pivot_model_holds_the_two_keys() {
    let (_dir, root) = project();
    let report = make_model(
        &root,
        "src/modules",
        &name("users"),
        &name("deal_tag"),
        Companions::all_for_pivot(),
        now(),
    )
    .unwrap();
    let model = read(&root, "src/modules/users/models/deal_tag.rs");
    for expected in [
        "the `deal_tag` pivot table",
        "    table = \"deal_tag\",\n",
        "factory = super::super::factories::deal_tag::DealTagFactory,",
        "    pub deal_id: Uuid,\n",
        "    pub tag_id: Uuid,\n",
    ] {
        assert!(model.contains(expected), "{expected}\n{model}");
    }
    assert!(!model.contains("pub name"), "{model}");
    let migration = read(
        &root,
        "src/modules/users/migrations/m20261025_143005_create_deal_tag_table.rs",
    );
    assert!(
        migration.contains(
            "                t.uuid_primary();\n                t.foreign_uuid(\"deal_id\");\n                t.foreign_uuid(\"tag_id\");\n                t.unique([\"deal_id\", \"tag_id\"]);\n                t.timestamps_tz();\n"
        ),
        "{migration}"
    );
    let factory = read(&root, "src/modules/users/factories/deal_tag.rs");
    assert!(!factory.contains("name:"), "{factory}");
    assert!(!path(&root, "src/modules/users/controllers/deal_tag.rs").exists());
    assert!(
        !report.hints.iter().any(|h| h.contains("make:migration")),
        "{:?}",
        report.hints
    );

    for (pivot, companions, problem) in [
        ("deal", Companions::all_for_pivot(), "one word each"),
        (
            "line_item_tag",
            Companions::all_for_pivot(),
            "one word each",
        ),
        (
            "deal_note",
            Companions {
                controller: true,
                ..Companions::all_for_pivot()
            },
            "no controller or policy",
        ),
    ] {
        let error = make_model(
            &root,
            "src/modules",
            &name("users"),
            &name(pivot),
            companions,
            now(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(problem), "{pivot}: {error}");
    }
}
