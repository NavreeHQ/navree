//! Integration tests for `navree make:component`.

use std::path::{Path, PathBuf};

use navree_cli::names::Name;
use navree_cli::plugin::component::{ComponentOptions, make_component};
use navree_cli::plugin::make::{MakePluginOptions, make_plugin};

fn scaffold_plugin(root: &Path, name: &str) -> PathBuf {
    let options = MakePluginOptions {
        name: Name::parse("plugin", name).expect("valid plugin name"),
        path: "plugins".to_owned(),
        wrap: None,
    };
    make_plugin(root, &options).expect("make_plugin succeeds");
    root.join("plugins").join(name)
}

fn base_options(name: &str) -> ComponentOptions {
    ComponentOptions {
        name: name.to_owned(),
        path: ".".to_owned(),
        script: false,
        sheet: false,
        field: false,
        dry_run: false,
        force: false,
    }
}

#[test]
fn make_component_writes_a_component_its_assets_its_tests_turns_on_the_feature_and_wires_the_extension()
 {
    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = scaffold_plugin(temp.path(), "geo");
    let before_lib = std::fs::read_to_string(plugin_dir.join("src/lib.rs")).expect("read lib.rs");
    let before_toml =
        std::fs::read_to_string(plugin_dir.join("Cargo.toml")).expect("read Cargo.toml");

    // `--dry-run` writes nothing and lists what would be created and updated.
    let dry = make_component(
        &plugin_dir,
        &ComponentOptions {
            script: true,
            sheet: true,
            dry_run: true,
            ..base_options("drop_point")
        },
    )
    .expect("dry run succeeds");
    assert!(!plugin_dir.join("src/ui/drop_point.rs").exists());
    assert!(!plugin_dir.join("assets/drop-point.js").exists());
    assert!(!plugin_dir.join("assets/drop-point.css").exists());
    assert!(!plugin_dir.join("tests/browser_drop_point.rs").exists());
    assert_eq!(
        std::fs::read_to_string(plugin_dir.join("src/lib.rs")).expect("read lib.rs"),
        before_lib
    );
    assert_eq!(
        std::fs::read_to_string(plugin_dir.join("Cargo.toml")).expect("read Cargo.toml"),
        before_toml
    );
    assert_eq!(
        dry.lines,
        vec![
            "  would create     src/ui/drop_point.rs",
            "  would create     assets/drop-point.js",
            "  would create     assets/drop-point.css",
            "  would create     tests/browser_drop_point.rs",
            "  would create     src/ui/mod.rs",
            "  would update     src/lib.rs",
            "  would update     Cargo.toml",
            "  next     cargo test the_drop_point_follows_the_component_rules",
        ]
    );
    let preview = dry.dry_run_files.expect("dry run files");
    assert!(
        preview.contains("==> src/ui/drop_point.rs <=="),
        "{preview}"
    );
    assert!(
        preview.contains("==> assets/drop-point.js <=="),
        "{preview}"
    );

    // Real run writes all four files, edits src/ui/mod.rs, wires src/lib.rs and turns on the feature.
    let report = make_component(
        &plugin_dir,
        &ComponentOptions {
            script: true,
            sheet: true,
            ..base_options("drop_point")
        },
    )
    .expect("make_component succeeds");
    assert_eq!(
        report.lines,
        vec![
            "  created  src/ui/drop_point.rs",
            "  created  assets/drop-point.js",
            "  created  assets/drop-point.css",
            "  created  tests/browser_drop_point.rs",
            "  edited   src/ui/mod.rs",
            "  wired    m.contribute(ui::drop_point::DROP_POINT) (src/lib.rs)",
            "  feature  navree: ui-extension-files (Cargo.toml)",
            "  next     cargo test the_drop_point_follows_the_component_rules",
        ]
    );
    assert!(report.dry_run_files.is_none());

    let component_rs =
        std::fs::read_to_string(plugin_dir.join("src/ui/drop_point.rs")).expect("component.rs");
    assert!(
        component_rs.contains("pub struct DropPoint"),
        "{component_rs}"
    );
    assert!(
        component_rs.contains("navree::ui::component_methods!(DropPoint);"),
        "{component_rs}"
    );
    assert!(
        component_rs.contains("navree::ui::component_node!(DropPoint);"),
        "{component_rs}"
    );
    assert!(
        component_rs.contains(
            r#"pub static DROP_POINT: UiExtension = UiExtension::new("geo", "drop-point")"#
        ),
        "{component_rs}"
    );
    assert!(
        component_rs.contains(r#"extension_script!("assets/drop-point.js""#),
        "{component_rs}"
    );
    assert!(
        component_rs.contains(r#"extension_sheet!("assets/drop-point.css", "geo""#),
        "{component_rs}"
    );
    assert!(component_rs.contains(".contract(1)"), "{component_rs}");
    assert!(
        component_rs.contains(r#".routes(&["/geo/"])"#),
        "{component_rs}"
    );
    assert!(
        component_rs.contains(r#".slot("geo:drop-point")"#),
        "{component_rs}"
    );
    assert!(
        component_rs.contains("pub fn example() -> Node"),
        "{component_rs}"
    );
    assert!(
        component_rs.contains("fn the_drop_point_follows_the_component_rules()"),
        "{component_rs}"
    );
    assert!(
        component_rs.contains("kit::assert_component"),
        "{component_rs}"
    );

    let script_js =
        std::fs::read_to_string(plugin_dir.join("assets/drop-point.js")).expect("script.js");
    assert!(
        script_js.contains("export const contract = 1;"),
        "{script_js}"
    );
    assert!(script_js.contains("ui.find("), "{script_js}");
    assert!(!script_js.contains("document.querySelector"), "{script_js}");

    let sheet_css =
        std::fs::read_to_string(plugin_dir.join("assets/drop-point.css")).expect("sheet.css");
    assert!(
        sheet_css.contains(r#"&[data-slot="geo:drop-point"] {"#),
        "{sheet_css}"
    );

    let browser_rs = std::fs::read_to_string(plugin_dir.join("tests/browser_drop_point.rs"))
        .expect("browser_drop_point.rs");
    assert!(browser_rs.contains("#[ignore"), "{browser_rs}");
    assert!(browser_rs.contains("kit::stored_twin"), "{browser_rs}");
    assert!(
        browser_rs.contains("assert_no_policy_violations"),
        "{browser_rs}"
    );

    let ui_mod = std::fs::read_to_string(plugin_dir.join("src/ui/mod.rs")).expect("ui/mod.rs");
    assert!(ui_mod.contains("pub mod drop_point;"), "{ui_mod}");

    let lib_rs = std::fs::read_to_string(plugin_dir.join("src/lib.rs")).expect("lib.rs");
    let wire_pos = lib_rs
        .find("m.contribute(ui::drop_point::DROP_POINT);")
        .expect("wire line present");
    let marker_pos = lib_rs.find("// navree:wire").expect("marker kept");
    assert!(wire_pos < marker_pos, "{lib_rs}");

    let cargo_toml = std::fs::read_to_string(plugin_dir.join("Cargo.toml")).expect("Cargo.toml");
    assert!(
        cargo_toml.contains(r#""ui-extension-files""#),
        "{cargo_toml}"
    );

    // `--force` replaces the existing files and adds the field control when `--field` is asked.
    let replaced = make_component(
        &plugin_dir,
        &ComponentOptions {
            script: true,
            sheet: true,
            field: true,
            force: true,
            ..base_options("drop_point")
        },
    )
    .expect("force replace succeeds");
    assert_eq!(
        replaced.lines,
        vec![
            "  replaced src/ui/drop_point.rs",
            "  replaced assets/drop-point.js",
            "  replaced assets/drop-point.css",
            "  replaced tests/browser_drop_point.rs",
            "  next     cargo test the_drop_point_follows_the_component_rules",
        ]
    );
    let with_field =
        std::fs::read_to_string(plugin_dir.join("src/ui/drop_point.rs")).expect("component.rs");
    assert!(
        with_field.contains("pub static DROP_POINT_CONTROL: DropPointControl"),
        "{with_field}"
    );
    assert!(
        with_field.contains("fn the_drop_point_control_follows_the_control_rules()"),
        "{with_field}"
    );
    assert!(
        with_field.contains("kit::assert_control(&DROP_POINT_CONTROL)"),
        "{with_field}"
    );
}

#[test]
fn make_component_without_script_or_sheet_writes_no_extension() {
    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = scaffold_plugin(temp.path(), "geo");
    let before_lib = std::fs::read_to_string(plugin_dir.join("src/lib.rs")).expect("read lib.rs");
    let before_toml =
        std::fs::read_to_string(plugin_dir.join("Cargo.toml")).expect("read Cargo.toml");

    let report = make_component(&plugin_dir, &base_options("drop_point"))
        .expect("make_component without flags succeeds");
    assert_eq!(
        report.lines,
        vec![
            "  created  src/ui/drop_point.rs",
            "  edited   src/ui/mod.rs",
            "  next     cargo test the_drop_point_follows_the_component_rules",
        ]
    );
    assert!(!report.lines.iter().any(|line| line.starts_with("  wired")));
    assert!(
        !report
            .lines
            .iter()
            .any(|line| line.starts_with("  feature"))
    );

    let component_rs =
        std::fs::read_to_string(plugin_dir.join("src/ui/drop_point.rs")).expect("component.rs");
    assert!(!component_rs.contains("UiExtension"), "{component_rs}");
    assert!(!plugin_dir.join("assets").exists());
    assert!(!plugin_dir.join("tests/browser_drop_point.rs").exists());

    let ui_mod = std::fs::read_to_string(plugin_dir.join("src/ui/mod.rs")).expect("ui/mod.rs");
    assert!(ui_mod.contains("pub mod drop_point;"), "{ui_mod}");

    assert_eq!(
        std::fs::read_to_string(plugin_dir.join("src/lib.rs")).expect("lib.rs"),
        before_lib
    );
    assert_eq!(
        std::fs::read_to_string(plugin_dir.join("Cargo.toml")).expect("Cargo.toml"),
        before_toml
    );
}

#[test]
fn make_component_refuses_a_name_outside_the_grammar() {
    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = scaffold_plugin(temp.path(), "geo");

    for bad in ["Drop-Point", "1drop", "DropPoint"] {
        let err = make_component(&plugin_dir, &base_options(bad))
            .expect_err("invalid component name must fail");
        assert_eq!(
            err.to_string(),
            format!(
                "{bad} is not a component name: lower-case letters, digits and _, starting with a letter"
            )
        );
        assert!(!plugin_dir.join("src/ui").exists());
    }

    make_component(&plugin_dir, &base_options("drop_point")).expect("first run succeeds");
    let second = make_component(&plugin_dir, &base_options("drop_point"))
        .expect_err("second run without --force must fail");
    assert_eq!(
        second.to_string(),
        "src/ui/drop_point.rs exists; use --force to replace it"
    );
}

#[test]
fn make_component_in_an_app_takes_the_namespace_app_and_prints_the_line_without_a_marker() {
    let temp = tempfile::tempdir().expect("tempdir");

    // 1. App crate whose `src/lib.rs` has `// navree:wire`.
    let with_marker = temp.path().join("kesar_marker");
    std::fs::create_dir_all(with_marker.join("src")).expect("create src");
    std::fs::write(
        with_marker.join("Cargo.toml"),
        "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::write(
        with_marker.join("src/lib.rs"),
        "pub struct Shop;\n\nimpl navree::Module for Shop {\n    fn register(&self, m: &mut navree::ModuleBuilder) {\n        // navree:wire\n    }\n}\n",
    )
    .expect("write lib.rs");

    let wired_report = make_component(
        &with_marker,
        &ComponentOptions {
            script: true,
            sheet: true,
            ..base_options("drop_point")
        },
    )
    .expect("app with marker succeeds");
    assert!(
        wired_report.lines.contains(
            &"  wired    m.contribute(ui::drop_point::DROP_POINT) (src/lib.rs)".to_owned()
        ),
        "{:?}",
        wired_report.lines
    );
    let app_component =
        std::fs::read_to_string(with_marker.join("src/ui/drop_point.rs")).expect("component.rs");
    assert!(
        app_component.contains(r#".slot("app:drop-point")"#),
        "{app_component}"
    );
    assert!(
        app_component.contains(r#".routes(&["/app/"])"#),
        "{app_component}"
    );

    // 2. App crate whose `src/lib.rs` has no `// navree:wire` marker.
    let without_marker = temp.path().join("kesar_no_marker");
    std::fs::create_dir_all(without_marker.join("src")).expect("create src");
    std::fs::write(
        without_marker.join("Cargo.toml"),
        "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
    )
    .expect("write Cargo.toml");
    let untouched_lib = "pub mod modules;\n";
    std::fs::write(without_marker.join("src/lib.rs"), untouched_lib).expect("write lib.rs");

    let unwired_report = make_component(
        &without_marker,
        &ComponentOptions {
            script: true,
            ..base_options("drop_point")
        },
    )
    .expect("app without marker succeeds");
    assert!(
        !unwired_report
            .lines
            .iter()
            .any(|line| line.starts_with("  wired")),
        "{:?}",
        unwired_report.lines
    );
    assert_eq!(
        std::fs::read_to_string(without_marker.join("src/lib.rs")).expect("read lib.rs"),
        untouched_lib
    );
    assert_eq!(
        unwired_report.lines.last().map(String::as_str),
        Some(
            "  next     add m.contribute(crate::ui::drop_point::DROP_POINT) to the register of the module that should show it"
        )
    );
}
