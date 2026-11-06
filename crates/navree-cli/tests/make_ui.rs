//! `navree make:page`, `make:form` and `make:table`: Navree UI pages, forms and
//! data tables in a module.


use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use navree_cli::clock::Timestamp;
use navree_cli::make::{Companions, MakeKind, MakeOptions, make_item, make_model, make_module};
use navree_cli::make_ui::{UiLayout, UiOptions, default_module, make_ui_item, ui_files};
use navree_cli::names::{AppName, Name};
use navree_cli::new_app::{Database, NavreeSource, NewApp, Ui, create_app};

fn now() -> Timestamp {
    Timestamp::from_unix(1_792_938_605)
}

fn name(s: &str) -> Name {
    Name::parse("name", s).unwrap()
}

/// A new app, with its one module, `users`.
fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let app = NewApp {
        name: AppName::parse("blog").unwrap(),
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

/// The paths of a report, from the app's root, with `/` separators.
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

const USERS_WIRE: &str = "        // navree:wire\n";

fn page(public: bool) -> UiOptions {
    UiOptions {
        public,
        ..UiOptions::default()
    }
}

// The files, as the templates write them

#[test]
fn a_page_is_a_controller_and_its_test() {
    let layout = UiLayout {
        new_dir: true,
        new_tests: true,
        model: None,
    };
    let files = ui_files(
        MakeKind::Page,
        &name("users"),
        &name("dashboard"),
        &UiOptions::default(),
        &layout,
    )
    .unwrap();
    assert_eq!(
        files.paths(),
        [
            "pages/dashboard.rs",
            "pages/mod.rs",
            "tests/dashboard_page.rs",
            "tests/mod.rs"
        ]
    );
    insta::assert_snapshot!("make_page_dashboard", files.to_string());
}

#[test]
fn a_public_page_is_marked_public() {
    let files = ui_files(
        MakeKind::Page,
        &name("users"),
        &name("about"),
        &page(true),
        &UiLayout::default(),
    )
    .unwrap();
    assert_eq!(files.paths(), ["pages/about.rs", "tests/about_page.rs"]);
    insta::assert_snapshot!("make_page_about_public", files.to_string());
}

#[test]
fn a_form_is_a_struct_with_its_handlers() {
    let files = ui_files(
        MakeKind::Form,
        &name("deals"),
        &name("new_deal"),
        &UiOptions::default(),
        &UiLayout::fresh(),
    )
    .unwrap();
    assert_eq!(files.paths(), ["forms/new_deal.rs", "forms/mod.rs"]);
    insta::assert_snapshot!("make_form_new_deal", files.to_string());
}

#[test]
fn a_table_is_a_row_with_its_model_and_its_handler() {
    let layout = UiLayout {
        model: Some("super::super::entity::Deal".to_owned()),
        ..UiLayout::fresh()
    };
    let files = ui_files(
        MakeKind::Table,
        &name("deals"),
        &name("deals"),
        &UiOptions::default(),
        &layout,
    )
    .unwrap();
    assert_eq!(files.paths(), ["tables/deals.rs", "tables/mod.rs"]);
    insta::assert_snapshot!("make_table_deals", files.to_string());
}

#[test]
fn a_table_without_a_model_sketches_the_handler() {
    let files = ui_files(
        MakeKind::Table,
        &name("deals"),
        &name("deals"),
        &UiOptions::default(),
        &UiLayout::fresh(),
    )
    .unwrap();
    insta::assert_snapshot!("make_table_deals_without_a_model", files.to_string());
}

// What the files contain

#[test]
fn a_page_takes_the_shell_and_asks_for_a_signed_in_user() {
    let files = ui_files(
        MakeKind::Page,
        &name("users"),
        &name("dashboard"),
        &UiOptions::default(),
        &UiLayout::default(),
    )
    .unwrap();
    let controller = files.get("pages/dashboard.rs").unwrap();
    assert!(
        controller.contains("async fn show(&self, shell: AppShell) -> AppPage {"),
        "{controller}"
    );
    assert!(
        controller.contains("#[get(\"!/dashboard\")]"),
        "{controller}"
    );
    assert!(
        controller.contains("\n#[authenticated]\nimpl DashboardPage {"),
        "{controller}"
    );
    assert!(controller.contains(".authenticated()"), "{controller}");
    assert!(!controller.contains("#[public]\nimpl"), "{controller}");
    assert!(
        controller.contains(".page(\"Dashboard\")\n            .crumb(\"Home\", \"/\")"),
        "{controller}"
    );
    let test = files.get("tests/dashboard_page.rs").unwrap();
    assert!(
        test.contains(".module(UiModule)") && test.contains("Factory::<Shell>::new"),
        "the test boots the app with a shell: {test}"
    );
    assert!(test.contains("assert_status(401)"), "{test}");
    assert!(test.contains("assert_status(200)"), "{test}");
    assert!(test.contains("use super::super::UsersModule;"), "{test}");
}

#[test]
fn a_public_page_lets_guests_in() {
    let files = ui_files(
        MakeKind::Page,
        &name("site"),
        &name("about"),
        &page(true),
        &UiLayout::default(),
    )
    .unwrap();
    let controller = files.get("pages/about.rs").unwrap();
    assert!(
        controller.contains("\n#[public]\nimpl AboutPage {"),
        "{controller}"
    );
    assert!(controller.contains(".public()"), "{controller}");
    assert!(
        !controller.contains("#[authenticated]\nimpl"),
        "{controller}"
    );
    let test = files.get("tests/about_page.rs").unwrap();
    assert!(test.contains("guests_may_open_the_page"), "{test}");
    assert!(!test.contains("assert_status(401)"), "{test}");
    assert!(!test.contains("principal"), "no unused import: {test}");
}

#[test]
fn names_follow_rust_and_http_conventions() {
    let module = name("sales");
    let none = UiOptions::default();
    let layout = UiLayout::default();

    let page = ui_files(
        MakeKind::Page,
        &module,
        &name("sales_report"),
        &none,
        &layout,
    )
    .unwrap();
    let source = page.get("pages/sales_report.rs").unwrap();
    assert!(source.contains("pub struct SalesReportPage;"), "{source}");
    assert!(source.contains("#[get(\"!/sales-report\")]"), "{source}");
    assert!(source.contains(".page(\"Sales report\")"), "{source}");
    assert!(
        page.get("tests/sales_report_page.rs")
            .unwrap()
            .contains("use super::super::SalesModule;")
    );

    let form = ui_files(
        MakeKind::Form,
        &module,
        &name("sales_report"),
        &none,
        &layout,
    )
    .unwrap();
    let source = form.get("forms/sales_report.rs").unwrap();
    assert!(source.contains("pub struct SalesReportForm {"), "{source}");
    assert!(source.contains(".post(\"/sales-report\")"), "{source}");

    // A name that ends in the kind's word does not say it twice.
    let page = ui_files(MakeKind::Page, &module, &name("about_page"), &none, &layout).unwrap();
    assert!(
        page.get("pages/about_page.rs")
            .unwrap()
            .contains("pub struct AboutPage;")
    );
    let form = ui_files(MakeKind::Form, &module, &name("deal_form"), &none, &layout).unwrap();
    assert!(
        form.get("forms/deal_form.rs")
            .unwrap()
            .contains("pub struct DealForm {")
    );
}

#[test]
fn a_table_is_named_for_one_row_whatever_the_number_of_the_name() {
    let module = name("deals");
    let layout = UiLayout::default();
    let plural = ui_files(
        MakeKind::Table,
        &module,
        &name("deals"),
        &UiOptions::default(),
        &layout,
    )
    .unwrap();
    let singular = ui_files(
        MakeKind::Table,
        &module,
        &name("deal"),
        &UiOptions::default(),
        &layout,
    )
    .unwrap();
    let (plural, singular) = (
        plural.get("tables/deals.rs").unwrap(),
        singular.get("tables/deal.rs").unwrap(),
    );
    assert_eq!(plural, singular, "the same table, in a file of either name");
    assert!(plural.contains("pub struct DealRow {"), "{plural}");
    assert!(plural.contains("DataTable::new(\"deals\""), "{plural}");
    assert!(plural.contains("link = \"/deals/{id}\""), "{plural}");
    assert!(plural.contains("pub fn deals_table<'a>("), "{plural}");

    let words = ui_files(
        MakeKind::Table,
        &module,
        &name("sales_reports"),
        &UiOptions::default(),
        &layout,
    )
    .unwrap();
    let source = words.get("tables/sales_reports.rs").unwrap();
    assert!(source.contains("pub struct SalesReportRow {"), "{source}");
    assert!(
        source.contains("DataTable::new(\"sales_reports\""),
        "{source}"
    );
    assert!(
        source.contains("link = \"/sales-reports/{id}\""),
        "{source}"
    );
    assert!(source.contains(".caption(\"Sales reports\")"), "{source}");
    assert!(source.contains("\"No sales reports yet\""), "{source}");
}

#[test]
fn a_table_shows_what_a_column_can_do() {
    let files = ui_files(
        MakeKind::Table,
        &name("deals"),
        &name("deals"),
        &UiOptions::default(),
        &UiLayout::default(),
    )
    .unwrap();
    let source = files.get("tables/deals.rs").unwrap();
    for expected in [
        "#[derive(Clone, TableRow)]",
        "#[table(sort = \"-created_at\", per = 25, per_pages(25, 50, 100))]",
        "#[col(skip)]",
        "#[col(sortable, search, sticky, filter = text, link = \"/deals/{id}\")]",
        "#[col(sortable, filter = date)]",
        // The columns to copy: a badge, and an amount in rupees.
        "badge = stage_tone",
        "numeric, filter = range",
        "label = \"Value (₹)\"",
    ] {
        assert!(
            source.contains(expected),
            "`{expected}` is missing: {source}"
        );
    }
}

#[test]
fn a_form_shows_the_widgets_and_the_handlers() {
    let files = ui_files(
        MakeKind::Form,
        &name("deals"),
        &name("new_deal"),
        &UiOptions::default(),
        &UiLayout::default(),
    )
    .unwrap();
    let source = files.get("forms/new_deal.rs").unwrap();
    for expected in [
        "#[derive(Debug, Deserialize, Validate, Form)]",
        "pub struct NewDealForm {",
        // text, money, an enum with options, a date
        "#[validate(length(min = 1, max = 255))]",
        "pub amount: Money,",
        "#[derive(Clone, Copy, Debug, Deserialize, Options)]",
        "pub closes_on: Option<chrono::NaiveDate>,",
        // email, number and a checkbox, to copy
        "#[validate(email)]",
        "#[validate(range(min = 1, max = 500))]",
        "pub hot: bool,",
        // the handlers
        "pub fn page(shell: &AppShell) -> AppPage {",
        "submit: Result<Submit<NewDealForm>, Rejected<NewDealForm>>,",
        "Err(rejected) => rejected.respond(|form| page_with(shell, form)),",
        // Indian demo data
        "placeholder = \"1,25,000\"",
        "help = \"In rupees.\"",
    ] {
        assert!(
            source.contains(expected),
            "`{expected}` is missing: {source}"
        );
    }
}

#[test]
fn generated_files_never_leak_template_syntax() {
    let options = [UiOptions::default(), page(true)];
    let layouts = [
        UiLayout::default(),
        UiLayout::fresh(),
        UiLayout {
            new_dir: true,
            new_tests: true,
            model: Some("super::super::models::deal::Deal".to_owned()),
        },
    ];
    for kind in MakeKind::UI {
        for options in &options {
            for layout in &layouts {
                let files =
                    ui_files(kind, &name("deals"), &name("new_deal"), options, layout).unwrap();
                for file in files.iter() {
                    for marker in ["{{", "}}", "{%", "%}", "{#"] {
                        assert!(
                            !file.contents.contains(marker),
                            "{} contains `{marker}`",
                            file.path
                        );
                    }
                    assert!(
                        file.contents.ends_with('\n'),
                        "{} ends with a newline",
                        file.path
                    );
                    assert!(!file.contents.contains('\r'), "{} uses LF", file.path);
                }
            }
        }
    }
}

#[test]
fn generated_ui_rust_is_rustfmt_clean() {
    if Command::new("rustfmt").arg("--version").output().is_err() {
        eprintln!("rustfmt is not installed; skipping");
        return;
    }
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let with_model = UiLayout {
        model: Some("super::super::entity::Deal".to_owned()),
        ..UiLayout::fresh()
    };
    let cases = [
        (
            MakeKind::Page,
            "dashboard",
            UiOptions::default(),
            UiLayout::fresh(),
        ),
        (MakeKind::Page, "about", page(true), UiLayout::fresh()),
        (
            MakeKind::Form,
            "new_deal",
            UiOptions::default(),
            UiLayout::fresh(),
        ),
        (MakeKind::Table, "deals", UiOptions::default(), with_model),
        (
            MakeKind::Table,
            "deals",
            UiOptions::default(),
            UiLayout::fresh(),
        ),
    ];
    let mut unformatted = Vec::new();
    for (at, (kind, item, options, layout)) in cases.iter().enumerate() {
        let files = ui_files(*kind, &name("deals"), &name(item), options, layout).unwrap();
        for file in files.iter() {
            let target = dir
                .path()
                .join(format!("{at}_{}", file.path.replace('/', "_")));
            std::fs::write(&target, &file.contents).unwrap();
            let output = Command::new("rustfmt")
                .args([
                    "--edition",
                    "2024",
                    "--check",
                    "--config",
                    "skip_children=true",
                ])
                .arg(&target)
                .output()
                .unwrap();
            if !output.status.success() {
                unformatted.push(format!(
                    "{}\n{}",
                    file.path,
                    String::from_utf8_lossy(&output.stdout)
                ));
            }
        }
    }
    assert!(
        unformatted.is_empty(),
        "not rustfmt-clean:\n{}",
        unformatted.join("\n")
    );
}

// In an app

#[test]
fn make_page_declares_registers_and_tests_it() {
    let (_dir, root) = project();
    let report = make_ui_item(
        &root,
        "src/modules",
        MakeKind::Page,
        &name("users"),
        &name("dashboard"),
        &UiOptions::default(),
    )
    .unwrap();
    assert_eq!(
        shown(&root, &report.made.created),
        [
            "src/modules/users/pages/dashboard.rs",
            "src/modules/users/pages/mod.rs",
            "src/modules/users/tests/dashboard_page.rs",
        ]
    );
    assert_eq!(
        shown(&root, &report.made.updated),
        ["src/modules/users/mod.rs", "src/modules/users/tests/mod.rs"]
    );
    assert!(report.overwritten.is_empty() && !report.dry_run);

    assert_eq!(
        read(&root, "src/modules/users/pages/mod.rs"),
        "//! UI pages of the users module.\n\npub mod dashboard;\n"
    );
    let module = read(&root, "src/modules/users/mod.rs");
    assert!(
        module.contains("mod migrations;\npub mod pages;\nmod service;\n"),
        "declared in name order: {module}"
    );
    assert!(
        module.contains(
            "        navree::http::ModuleBuilderHttpExt::controller::<pages::dashboard::DashboardPage>(m);\n        // navree:wire\n"
        ),
        "{module}"
    );
    let tests = read(&root, "src/modules/users/tests/mod.rs");
    assert!(tests.ends_with("\nmod dashboard_page;\n"), "{tests}");

    // The files are what `ui_files` renders.
    let files = ui_files(
        MakeKind::Page,
        &name("users"),
        &name("dashboard"),
        &UiOptions::default(),
        &UiLayout::default(),
    )
    .unwrap();
    assert_eq!(
        read(&root, "src/modules/users/pages/dashboard.rs"),
        files.get("pages/dashboard.rs").unwrap()
    );
    assert_eq!(
        read(&root, "src/modules/users/tests/dashboard_page.rs"),
        files.get("tests/dashboard_page.rs").unwrap()
    );

    // What the user must do: the shell, and a link to the page.
    assert_eq!(report.made.hints.len(), 2, "{:?}", report.made.hints);
    let hint = &report.made.hints[0];
    for expected in [
        "AppShell",
        "navree::ui::UiModule",
        "registered `Shell`",
        "--ui navree",
        "/docs/ui/app-shell/",
    ] {
        assert!(hint.contains(expected), "`{expected}` is missing: {hint}");
    }
    assert!(
        report.made.hints[1].contains("NavItem::new(\"Dashboard\", \"/dashboard\")"),
        "{:?}",
        report.made.hints
    );
}

#[test]
fn a_second_page_joins_the_first_in_name_order() {
    let (_dir, root) = project();
    for page in ["dashboard", "about"] {
        make_ui_item(
            &root,
            "src/modules",
            MakeKind::Page,
            &name("users"),
            &name(page),
            &UiOptions::default(),
        )
        .unwrap();
    }
    assert_eq!(
        read(&root, "src/modules/users/pages/mod.rs"),
        "//! UI pages of the users module.\n\npub mod about;\npub mod dashboard;\n"
    );
    let module = read(&root, "src/modules/users/mod.rs");
    assert_eq!(module.matches("pub mod pages;").count(), 1, "{module}");
    assert!(
        module.contains(
            "controller::<pages::dashboard::DashboardPage>(m);\n        navree::http::ModuleBuilderHttpExt::controller::<pages::about::AboutPage>(m);\n        // navree:wire\n"
        ),
        "{module}"
    );
    let tests = read(&root, "src/modules/users/tests/mod.rs");
    assert!(
        tests.ends_with("\nmod about_page;\nmod dashboard_page;\n"),
        "{tests}"
    );
}

#[test]
fn a_module_without_tests_gets_them() {
    let (_dir, root) = project();
    std::fs::create_dir_all(path(&root, "src/modules/site")).unwrap();
    std::fs::write(
        path(&root, "src/modules/site/mod.rs"),
        "//! The site module.\n\nuse navree::kernel::ModuleBuilder;\n\npub struct SiteModule;\n\nimpl SiteModule {\n    fn wire(&self, m: &mut ModuleBuilder) {\n        // navree:wire\n    }\n}\n",
    )
    .unwrap();
    let report = make_ui_item(
        &root,
        "src/modules",
        MakeKind::Page,
        &name("site"),
        &name("about"),
        &page(true),
    )
    .unwrap();
    assert_eq!(
        shown(&root, &report.made.created),
        [
            "src/modules/site/pages/about.rs",
            "src/modules/site/pages/mod.rs",
            "src/modules/site/tests/about_page.rs",
            "src/modules/site/tests/mod.rs",
        ]
    );
    assert_eq!(
        shown(&root, &report.made.updated),
        ["src/modules/site/mod.rs"]
    );
    assert_eq!(
        read(&root, "src/modules/site/tests/mod.rs"),
        "//! Tests of the site module.\n\nmod about_page;\n"
    );
    let module = read(&root, "src/modules/site/mod.rs");
    assert!(
        module.contains("}\n\npub mod pages;\n#[cfg(test)]\nmod tests;\n"),
        "{module}"
    );
    assert!(
        module.contains("controller::<pages::about::AboutPage>(m);\n        // navree:wire\n"),
        "{module}"
    );
}

#[test]
fn a_second_run_refuses_to_overwrite_without_force() {
    let (_dir, root) = project();
    let run = |options: &UiOptions| {
        make_ui_item(
            &root,
            "src/modules",
            MakeKind::Page,
            &name("users"),
            &name("dashboard"),
            options,
        )
    };
    run(&UiOptions::default()).unwrap();
    let module = read(&root, "src/modules/users/mod.rs");
    let page = read(&root, "src/modules/users/pages/dashboard.rs");
    std::fs::write(
        path(&root, "src/modules/users/pages/dashboard.rs"),
        "mine\n",
    )
    .unwrap();

    let error = run(&UiOptions::default()).unwrap_err().to_string();
    assert!(error.contains("refusing to overwrite"), "{error}");
    assert!(error.contains("--force"), "{error}");
    assert!(error.contains("dashboard.rs"), "{error}");
    assert_eq!(
        read(&root, "src/modules/users/pages/dashboard.rs"),
        "mine\n",
        "nothing was touched"
    );
    assert_eq!(read(&root, "src/modules/users/mod.rs"), module);

    // --force replaces the files, and registers nothing twice.
    let report = run(&UiOptions {
        force: true,
        ..UiOptions::default()
    })
    .unwrap();
    assert_eq!(
        shown(&root, &report.overwritten),
        [
            "src/modules/users/pages/dashboard.rs",
            "src/modules/users/tests/dashboard_page.rs"
        ]
    );
    assert!(report.made.created.is_empty(), "{:?}", report.made.created);
    assert!(report.made.updated.is_empty(), "{:?}", report.made.updated);
    assert_eq!(read(&root, "src/modules/users/pages/dashboard.rs"), page);
    assert_eq!(read(&root, "src/modules/users/mod.rs"), module);
}

#[test]
fn dry_run_writes_nothing() {
    let (_dir, root) = project();
    let module = read(&root, "src/modules/users/mod.rs");
    let tests = read(&root, "src/modules/users/tests/mod.rs");
    for kind in MakeKind::UI {
        let report = make_ui_item(
            &root,
            "src/modules",
            kind,
            &name("users"),
            &name("deals"),
            &UiOptions {
                dry_run: true,
                ..UiOptions::default()
            },
        )
        .unwrap();
        assert!(report.dry_run);
        assert!(!report.made.created.is_empty() && !report.made.updated.is_empty());
        assert!(
            report.made.created.iter().all(|p| !p.exists()),
            "{kind:?}: {:?}",
            report.made.created
        );
    }
    for dir in ["pages", "forms", "tables"] {
        assert!(
            !path(&root, &format!("src/modules/users/{dir}")).exists(),
            "{dir}"
        );
    }
    assert!(!path(&root, "src/modules/users/tests/deals_page.rs").exists());
    assert_eq!(read(&root, "src/modules/users/mod.rs"), module);
    assert_eq!(read(&root, "src/modules/users/tests/mod.rs"), tests);

    // It says what it would do, with the contents under their paths.
    let report = make_ui_item(
        &root,
        "src/modules",
        MakeKind::Page,
        &name("users"),
        &name("dashboard"),
        &UiOptions {
            dry_run: true,
            ..UiOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        shown(&root, &report.made.updated),
        ["src/modules/users/mod.rs", "src/modules/users/tests/mod.rs"]
    );
    let shown_files = report.files.to_string();
    assert!(
        shown_files.starts_with("==> src/modules/users/pages/dashboard.rs <==\n"),
        "{shown_files}"
    );
    assert!(shown_files.contains("async fn show(&self, shell: AppShell)"));
    assert!(!report.made.hints.is_empty());
}

#[test]
fn a_form_declares_its_directory_and_registers_nothing() {
    let (_dir, root) = project();
    let report = make_ui_item(
        &root,
        "src/modules",
        MakeKind::Form,
        &name("users"),
        &name("new_deal"),
        &UiOptions::default(),
    )
    .unwrap();
    assert_eq!(
        shown(&root, &report.made.created),
        [
            "src/modules/users/forms/new_deal.rs",
            "src/modules/users/forms/mod.rs"
        ]
    );
    assert_eq!(
        shown(&root, &report.made.updated),
        ["src/modules/users/mod.rs"]
    );
    assert_eq!(
        read(&root, "src/modules/users/forms/mod.rs"),
        "//! Forms of the users module.\n\npub mod new_deal;\n"
    );
    let module = read(&root, "src/modules/users/mod.rs");
    assert!(
        module.contains("pub mod entity;\npub mod forms;\nmod migrations;\n"),
        "{module}"
    );
    assert!(
        !module.contains("new_deal"),
        "a form is not registered: {module}"
    );
    assert!(
        report
            .made
            .hints
            .iter()
            .any(|h| h.contains("forms::new_deal::page"))
    );

    // A second form joins the directory.
    make_ui_item(
        &root,
        "src/modules",
        MakeKind::Form,
        &name("users"),
        &name("invite"),
        &UiOptions::default(),
    )
    .unwrap();
    assert_eq!(
        read(&root, "src/modules/users/forms/mod.rs"),
        "//! Forms of the users module.\n\npub mod invite;\npub mod new_deal;\n"
    );
}

#[test]
fn a_table_reads_the_models_it_finds() {
    let (_dir, root) = project();
    make_module(&root, "src/modules", &name("deals"), true, now()).unwrap();
    // The module's own `entity.rs` (`make:module`): `Deal`.
    make_ui_item(
        &root,
        "src/modules",
        MakeKind::Table,
        &name("deals"),
        &name("deals"),
        &UiOptions::default(),
    )
    .unwrap();
    let table = read(&root, "src/modules/deals/tables/deals.rs");
    assert!(table.contains("use super::super::entity::Deal;"), "{table}");
    assert!(table.contains("impl From<Deal> for DealRow {"), "{table}");
    assert!(table.contains("pub async fn index("), "{table}");
    assert_eq!(
        read(&root, "src/modules/deals/tables/mod.rs"),
        "//! Data tables of the deals module.\n\npub mod deals;\n"
    );
    let module = read(&root, "src/modules/deals/mod.rs");
    assert!(module.contains("pub mod tables;\n"), "{module}");

    // A model made by `make:model` (`models/tag.rs`), by the singular of the table's name.
    make_model(
        &root,
        "src/modules",
        &name("deals"),
        &name("tag"),
        Companions::default(),
        now(),
    )
    .unwrap();
    let report = make_ui_item(
        &root,
        "src/modules",
        MakeKind::Table,
        &name("deals"),
        &name("tags"),
        &UiOptions::default(),
    )
    .unwrap();
    let table = read(&root, "src/modules/deals/tables/tags.rs");
    assert!(
        table.contains("use super::super::models::tag::Tag;"),
        "{table}"
    );
    assert!(table.contains("pub struct TagRow {"), "{table}");
    assert!(
        report
            .made
            .hints
            .iter()
            .any(|h| h.contains("tables::tags::index")),
        "{:?}",
        report.made.hints
    );
    assert_eq!(
        read(&root, "src/modules/deals/tables/mod.rs"),
        "//! Data tables of the deals module.\n\npub mod deals;\npub mod tags;\n"
    );
}

#[test]
fn a_table_without_a_model_it_can_read_says_what_to_do() {
    let (_dir, root) = project();
    // `users` has a `Profile` model, which has no name column; `labels` has no model.
    for table in ["profiles", "labels"] {
        let report = make_ui_item(
            &root,
            "src/modules",
            MakeKind::Table,
            &name("users"),
            &name(table),
            &UiOptions::default(),
        )
        .unwrap();
        let source = read(&root, &format!("src/modules/users/tables/{table}.rs"));
        assert!(!source.contains("use super::super::"), "{source}");
        assert!(!source.contains("\npub async fn index("), "{source}");
        assert!(source.contains("// pub async fn index("), "{source}");
        assert!(
            report
                .made
                .hints
                .iter()
                .any(|h| h.contains("no model with an id, a name and a created_at")),
            "{:?}",
            report.made.hints
        );
    }
}

#[test]
fn the_module_must_exist() {
    let (_dir, root) = project();
    let error = make_ui_item(
        &root,
        "src/modules",
        MakeKind::Page,
        &name("ghost"),
        &name("dashboard"),
        &UiOptions::default(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("module `ghost` not found"), "{error}");
    assert!(error.contains("navree make:module ghost"), "{error}");
    assert!(!path(&root, "src/modules/ghost").exists());
}

#[test]
fn without_a_module_name_the_only_module_is_used() {
    let (_dir, root) = project();
    assert_eq!(
        default_module(&root, "src/modules").unwrap().snake(),
        "users"
    );

    make_module(&root, "src/modules", &name("deals"), false, now()).unwrap();
    let error = default_module(&root, "src/modules")
        .unwrap_err()
        .to_string();
    assert!(error.contains("which module?"), "{error}");
    assert!(error.contains("deals, users"), "{error}");
    assert!(error.contains("--module"), "{error}");

    let empty = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    std::fs::create_dir_all(empty.path().join("src").join("modules")).unwrap();
    let error = default_module(empty.path(), "src/modules")
        .unwrap_err()
        .to_string();
    assert!(error.contains("no module found"), "{error}");
    assert!(error.contains("navree make:module"), "{error}");
}

#[test]
fn a_module_without_the_wire_marker_gets_a_hint() {
    let (_dir, root) = project();
    let module = read(&root, "src/modules/users/mod.rs");
    std::fs::write(
        path(&root, "src/modules/users/mod.rs"),
        module.replace(USERS_WIRE, ""),
    )
    .unwrap();
    let report = make_ui_item(
        &root,
        "src/modules",
        MakeKind::Page,
        &name("users"),
        &name("dashboard"),
        &UiOptions::default(),
    )
    .unwrap();
    assert!(
        path(&root, "src/modules/users/pages/dashboard.rs").is_file(),
        "the page is written all the same"
    );
    assert!(
        report.made.hints[0].contains(
            "register it in the module's `register` (or `wire`) method: `navree::http::ModuleBuilderHttpExt::controller::<pages::dashboard::DashboardPage>(m);`"
        ),
        "{:?}",
        report.made.hints
    );
}

#[test]
fn make_item_hands_the_ui_kinds_over() {
    let (_dir, root) = project();
    let report = make_item(
        &root,
        "src/modules",
        MakeKind::Page,
        &name("users"),
        &name("dashboard"),
        &MakeOptions::default(),
        now(),
    )
    .unwrap();
    assert_eq!(report.created.len(), 3, "{:?}", report.created);
    assert!(path(&root, "src/modules/users/pages/dashboard.rs").is_file());
}

// The binary

fn navree(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_navree"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("navree runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn the_commands_dry_run_create_replace_and_refuse() {
    let (_dir, root) = project();

    // --dry-run: what would happen, and nothing happens. The name may be `SalesReport`.
    let output = navree(&root, &["make:page", "SalesReport", "--dry-run"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    for expected in [
        "  would create     src/modules/users/pages/sales_report.rs\n",
        "  would create     src/modules/users/pages/mod.rs\n",
        "  would create     src/modules/users/tests/sales_report_page.rs\n",
        "  would update     src/modules/users/mod.rs\n",
        "  would update     src/modules/users/tests/mod.rs\n",
        "Nothing was written (--dry-run).",
        "==> src/modules/users/pages/sales_report.rs <==\n",
        "#[get(\"!/sales-report\")]",
    ] {
        assert!(out.contains(expected), "`{expected}` is missing:\n{out}");
    }
    assert!(!path(&root, "src/modules/users/pages").exists());

    // The real thing, with the module left out: the app has one.
    let output = navree(&root, &["make:page", "SalesReport"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    for expected in [
        "  created  src/modules/users/pages/sales_report.rs\n",
        "  created  src/modules/users/pages/mod.rs\n",
        "  created  src/modules/users/tests/sales_report_page.rs\n",
        "  updated  src/modules/users/mod.rs\n",
        "  updated  src/modules/users/tests/mod.rs\n",
        "  next     the page takes `AppShell`",
        "/docs/ui/app-shell/",
    ] {
        assert!(out.contains(expected), "`{expected}` is missing:\n{out}");
    }
    assert!(read(&root, "src/modules/users/pages/sales_report.rs").contains("SalesReportPage"));

    // A second run refuses; --force replaces.
    let output = navree(&root, &["make:page", "SalesReport"]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(err.contains("refusing to overwrite"), "{err}");
    assert!(err.contains("--force"), "{err}");
    let output = navree(&root, &["make:page", "sales-report", "--force", "--public"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(
        out.contains("  replaced src/modules/users/pages/sales_report.rs\n"),
        "{out}"
    );
    assert!(read(&root, "src/modules/users/pages/sales_report.rs").contains("#[public]"));

    // A form and a table: a name with a hyphen, a name as a type, the plural.
    let output = navree(&root, &["make:form", "new-deal", "--module", "users"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(root.join("src/modules/users/forms/new_deal.rs").is_file());
    let output = navree(&root, &["make:table", "Deals", "--module", "users"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(read(&root, "src/modules/users/tables/deals.rs").contains("pub struct DealRow"));

    // Names that cannot be Rust names are refused with a suggestion.
    for (name, expected) in [("mod", "is a Rust keyword"), ("2fa", "use snake_case")] {
        let output = navree(&root, &["make:page", name]);
        assert!(!output.status.success(), "{name}");
        let err = stderr(&output);
        assert!(err.contains(expected), "{name}: {err}");
    }
}

#[test]
fn several_modules_need_a_module_option() {
    let (_dir, root) = project();
    make_module(&root, "src/modules", &name("deals"), false, now()).unwrap();
    let output = navree(&root, &["make:table", "deals"]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(err.contains("which module?"), "{err}");
    assert!(err.contains("deals, users"), "{err}");
    let output = navree(&root, &["make:table", "deals", "--module", "deals"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(root.join("src/modules/deals/tables/deals.rs").is_file());
}
