//! Ties `docs/plugins/writing.mdx` to `navree-plugin-notices`: every quoted
//! file block is a consecutive slice of the real file, every file carrying the
//! `notices` namespace is listed on the page, the ten app-registration lines
//! are present, and the page's app boots and serves notices in [`TestApp`].

#![allow(missing_docs)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use navree::access::AccessService;
use navree::db::Db;
use navree::orm::OrmModule;
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use navree::ui::UiModule;
use navree::view::ViewModule;
use navree_plugin_notices::{NoticesModule, notice};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Block {
    line: usize,
    lang: String,
    flags: Vec<String>,
    code: String,
}

/// Reads every fenced block of `text` using the same fence rules as `skill-snippets`.
fn fenced_blocks(text: &str) -> std::result::Result<Vec<Block>, String> {
    let mut out = Vec::new();
    let mut open: Option<(usize, usize, usize, String)> = None;
    let mut code = String::new();
    for (index, line) in text.split('\n').enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let ticks = trimmed.chars().take_while(|c| *c == '`').count();
        match &open {
            None if ticks >= 3 => {
                open = Some((index + 1, ticks, indent, trimmed[ticks..].trim().to_owned()));
                code.clear();
            }
            None => {}
            Some((start, opened, at, info)) => {
                if ticks >= *opened && trimmed.trim_end().len() == ticks {
                    let mut words = info.split([',', ' ']).filter(|word| !word.is_empty());
                    out.push(Block {
                        line: *start,
                        lang: words.next().unwrap_or_default().to_owned(),
                        flags: words.map(str::to_owned).collect(),
                        code: std::mem::take(&mut code),
                    });
                    open = None;
                } else {
                    let cut = line.len().min(*at);
                    let strip = line[..cut].trim().is_empty();
                    code.push_str(if strip { &line[cut..] } else { line });
                    code.push('\n');
                }
            }
        }
    }
    match open {
        Some((line, ..)) => Err(format!("the fence opened on line {line} never closes")),
        None => Ok(out),
    }
}

fn writing_page() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins/writing.mdx");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
        .replace("\r\n", "\n")
}

fn check_quoted_blocks(
    repo_root: &Path,
    blocks: &[Block],
) -> std::result::Result<BTreeSet<String>, String> {
    let mut quoted = BTreeSet::new();
    for block in blocks {
        let prefix = match block.lang.as_str() {
            "rust" => "// ",
            "toml" => "# ",
            _ => continue,
        };
        let mut lines = block.code.lines();
        let Some(first) = lines.next() else {
            continue;
        };
        let Some(rel) = first
            .trim()
            .strip_prefix(prefix)
            .map(str::trim)
            .filter(|rest| rest.starts_with("plugins/notices/"))
        else {
            continue;
        };

        let full = repo_root.join(rel);
        if !full.is_file() {
            return Err(format!(
                "line {}: quoted file `{rel}` does not exist",
                block.line
            ));
        }

        let file_text = fs::read_to_string(&full)
            .map_err(|err| format!("line {}: cannot read `{rel}`: {err}", block.line))?
            .replace("\r\n", "\n");
        let hay: Vec<&str> = file_text.lines().map(str::trim_end).collect();
        let needle: Vec<&str> = lines.map(str::trim_end).collect();
        if needle.is_empty() {
            return Err(format!(
                "line {}: quoted block for `{rel}` is empty",
                block.line
            ));
        }

        let matches = hay
            .windows(needle.len())
            .any(|window| window == needle.as_slice());
        if !matches {
            return Err(format!(
                "line {}: block is not a consecutive line slice of `{rel}`",
                block.line
            ));
        }

        let crate_rel = rel.strip_prefix("plugins/notices/").unwrap_or(rel);
        quoted.insert(crate_rel.to_owned());
    }
    Ok(quoted)
}

#[tokio::test]
async fn the_example_plugin_of_the_writing_page_boots_in_test_app() {
    let page = writing_page();
    let blocks = fenced_blocks(&page).expect("valid fences");
    let last_rust = blocks
        .iter()
        .rev()
        .find(|b| b.lang == "rust")
        .expect("closing rust block");
    for required in [
        "ConfigModule",
        "HttpModule",
        "DatabaseModule",
        "OrmModule",
        "ViewModule",
        "UiModule",
        "AccessModule",
        "NoticesModule::from_config()",
    ] {
        assert!(
            last_rust.code.contains(required),
            "closing app block is missing `{required}`:\n{}",
            last_rust.code
        );
    }

    let app = TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(NoticesModule::from_config())
        .build()
        .await;

    let report = app.ctx().plugins();
    let plugin = report
        .plugins
        .iter()
        .find(|plugin| plugin.info.name == "navree-plugin-notices")
        .expect("navree-plugin-notices is listed in plugins");
    assert_eq!(plugin.info.namespace, "notices");
    assert_eq!(plugin.info.title, Some("Notice board"));

    let harpreet = Uuid::now_v7();
    let db = app.get::<Db>().await;
    let access = app.get::<AccessService>().await;
    let uow = db.uow(None, None).await;
    access
        .create_role(&uow, "admin", None)
        .await
        .expect("create admin role");
    access
        .sync_role_permissions(&uow, "admin", ["notices.read", "notices.publish"])
        .await
        .expect("grant notices permissions");
    access
        .assign_role(&uow, harpreet, "admin")
        .await
        .expect("assign admin role to Harpreet Sandhu");
    uow.commit().await.expect("commit grants");

    app.acting_as(principal(harpreet))
        .post("/api/notices")
        .json(&json!({
            "title": "Water supply off on Sunday",
            "body": "Tank cleaning at Model Town, Ludhiana, from 6 am to 10 am IST."
        }))
        .send()
        .await
        .assert_status(201);

    let listed: Vec<notice::Model> = app
        .acting_as(principal(harpreet))
        .get("/api/notices")
        .send()
        .await
        .assert_status(200)
        .json();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "Water supply off on Sunday");
    assert_eq!(
        listed[0].body,
        "Tank cleaning at Model Town, Ludhiana, from 6 am to 10 am IST."
    );
    assert_eq!(listed[0].posted_by, harpreet);

    app.shutdown().await.expect("clean shutdown");
}

#[test]
fn every_code_block_of_the_writing_page_is_a_slice_of_the_example() {
    let page = writing_page();
    let blocks = fenced_blocks(&page).expect("valid fences");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root");

    let quoted = check_quoted_blocks(&repo_root, &blocks)
        .unwrap_or_else(|err| panic!("quoted block check failed: {err}"));

    for expected in [
        "Cargo.toml",
        "src/lib.rs",
        "src/options.rs",
        "src/model.rs",
        "src/controller.rs",
        "src/jobs.rs",
    ] {
        assert!(
            quoted.contains(expected),
            "`writing.mdx` must quote `plugins/notices/{expected}`, found: {quoted:?}"
        );
    }

    // A quoted file that does not exist fails naming the block's opening fence line.
    let missing = vec![Block {
        line: 42,
        lang: "rust".to_owned(),
        flags: Vec::new(),
        code: "// plugins/notices/src/does_not_exist.rs\npub struct Missing;\n".to_owned(),
    }];
    let err = check_quoted_blocks(&repo_root, &missing).expect_err("missing file must fail");
    assert!(
        err.contains("line 42"),
        "expected block line in error: {err}"
    );
}

fn holds_namespace(text: &str) -> bool {
    if text.contains("APP_NOTICES__") {
        return true;
    }
    let bytes = text.as_bytes();
    let needle = b"notices";
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0usize;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let before_ok = i == 0 || !is_word(bytes[i - 1]);
            if before_ok {
                let after = bytes.get(i + needle.len()).copied();
                let matches_form = match after {
                    None => true,
                    Some(b'.' | b'-' | b'_') => true,
                    Some(ch) => !is_word(ch),
                };
                if matches_form {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

fn collect_crate_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read crate directory") {
        let path = entry.expect("dir entry").path();
        let rel = path
            .strip_prefix(root)
            .expect("under crate root")
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "tests" || rel.starts_with("tests/") || rel == "README.md" {
            continue;
        }
        if path.is_dir() {
            collect_crate_files(root, &path, out);
        } else {
            out.push(rel);
        }
    }
}

#[test]
fn the_names_the_page_lists_are_every_place_the_namespace_appears() {
    let page = writing_page();
    let blocks = fenced_blocks(&page).expect("valid fences");
    let names_block = blocks
        .iter()
        .find(|b| {
            b.lang == "text" && b.code.contains("Cargo.toml") && b.code.contains("src/lib.rs")
        })
        .expect("`Names that carry the namespace` text block on writing.mdx");

    let listed: BTreeSet<String> = names_block
        .code
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect();

    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_crate_files(crate_root, crate_root, &mut files);

    let actual: BTreeSet<String> = files
        .into_iter()
        .filter(|rel| {
            let content = fs::read_to_string(crate_root.join(rel))
                .unwrap_or_else(|err| panic!("read {rel}: {err}"));
            holds_namespace(&content)
        })
        .collect();

    assert_eq!(
        listed, actual,
        "files listed in `Names that carry the namespace` differ from files in plugins/notices holding `notices`"
    );
}

#[test]
fn the_app_registration_table_lists_the_ten_lines_the_framework_enforces() {
    let page = writing_page();
    let section_pos = page
        .find("## What the app registers for a plugin")
        .expect("## What the app registers for a plugin heading");
    let section = &page[section_pos..];

    assert!(
        section
            .contains("To use any of these, register the plugin's piece from a module of your app"),
        "missing lead-in sentence before the app-registration table"
    );

    let expected_rows: &[(&str, &str)] = &[
        (
            "apply its policy",
            "`.import(AccessModule::for_policy::<khata::InvoicePolicy>())`",
        ),
        (
            "register its subject",
            "`.import(AccessModule::for_subject::<khata::Ledger>())`",
        ),
        (
            "configure a module it uses",
            "`AccessModule::for_root(..)` in your app's root (the plugin imports `AccessModule`)",
        ),
        (
            "add its permission check",
            "`m.multi(khata::permission_check())`",
        ),
        (
            "add its sign-in strategy",
            "`m.strategy::<khata::LedgerStrategy>()`",
        ),
        ("add its middleware", "`m.http_layer(khata::timeout())`"),
        (
            "let it decide Helm's staff",
            "`m.helm_staff_workspaces(khata::may_enter)`",
        ),
        (
            "use its channel authorizer",
            "`m.channel(\"orders.{id}\", notices::authorize)`",
        ),
        (
            "scope its model",
            "`m.workspace_scoped::<notices::Notice>()`",
        ),
        (
            "apply its scope or hook to your model",
            "`m.global_scope::<Deal>(\"open\", khata::open)`, `m.on_model::<Deal, _>(ModelEvent::Saving, khata::stamp)`",
        ),
    ];

    for (to_cell, line_cell) in expected_rows {
        let row_found = section.lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with('|') && trimmed.contains(to_cell) && trimmed.contains(line_cell)
        });
        assert!(
            row_found,
            "missing table row for `{to_cell}` -> `{line_cell}`"
        );
    }
}
