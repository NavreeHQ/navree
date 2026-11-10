//! `icon_file!`, `icon_set!`, `lucide!` and `flag!` fail the build, for the right reason.
//!
//! Each case is a tiny crate that depends on navree-ui and uses one macro; the test runs
//! `cargo build` on it and looks at what the compiler said. A case that should build is
//! there too, so a failure can't be the setup: the good icon and the good set build, and
//! each bad one stops with its file, its line and its reason. The bad files are the
//! ones in `tests/bad-icons/`.
//!
//! The first run compiles navree-ui again, in `CARGO_TARGET_TMPDIR`,
//! which later runs reuse.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CRATE: &str = env!("CARGO_MANIFEST_DIR");

fn slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// What a case expects of the build.
enum Expect {
    Builds,
    Fails(&'static [&'static str]),
}

struct Case {
    name: &'static str,
    /// The body of `main.rs`.
    code: &'static str,
    /// Files to put in the case's crate: `(path in it, a file of tests/bad-icons or the text itself)`.
    files: &'static [(&'static str, Source)],
    expect: Expect,
}

#[derive(Clone, Copy)]
enum Source {
    /// A file of `tests/bad-icons/` or `tests/own-icons/`.
    Fixture(&'static str),
    Text(&'static str),
}

use Source::{Fixture, Text};

const GOOD: &str =
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M5 12h14"/></svg>"#;

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "good_file_and_set",
            code: r#"
                const A: navree_ui::icons::Icon = navree_ui::icon_file!("assets/good.svg");
                mod set { navree_ui::icon_set!("assets/good-set"); }
                fn main() { let _ = (A, set::ONE, set::ALL.len()); }
            "#,
            files: &[
                ("assets/good.svg", Text(GOOD)),
                ("assets/good-set/one.svg", Text(GOOD)),
            ],
            expect: Expect::Builds,
        },
        Case {
            name: "script",
            code: r#"const A: navree_ui::icons::Icon = navree_ui::icon_file!("assets/script.svg"); fn main() { let _ = A; }"#,
            files: &[("assets/script.svg", Fixture("bad-icons/script.svg"))],
            expect: Expect::Fails(&["script.svg", "line 3", "<script>", "scripts cannot run"]),
        },
        Case {
            name: "style_element",
            code: r#"const A: navree_ui::icons::Icon = navree_ui::icon_file!("assets/style.svg"); fn main() { let _ = A; }"#,
            files: &[("assets/style.svg", Fixture("bad-icons/style-text.svg"))],
            expect: Expect::Fails(&["style.svg", "line 2", "<style>", "content security policy"]),
        },
        Case {
            name: "event_handler",
            code: r#"const A: navree_ui::icons::Icon = navree_ui::icon_file!("assets/onclick.svg"); fn main() { let _ = A; }"#,
            files: &[("assets/onclick.svg", Fixture("bad-icons/onclick.svg"))],
            expect: Expect::Fails(&["onclick.svg", "line 2", "onclick", "event handler"]),
        },
        Case {
            name: "link_to_the_web",
            code: r#"const A: navree_ui::icons::Icon = navree_ui::icon_file!("assets/use.svg"); fn main() { let _ = A; }"#,
            files: &[("assets/use.svg", Fixture("bad-icons/use-link.svg"))],
            expect: Expect::Fails(&["use.svg", "line 2", "<use>", "cannot refer to another file"]),
        },
        Case {
            name: "bad_viewbox",
            code: r#"const A: navree_ui::icons::Icon = navree_ui::icon_file!("assets/box.svg"); fn main() { let _ = A; }"#,
            files: &[("assets/box.svg", Fixture("bad-icons/bad-viewbox.svg"))],
            expect: Expect::Fails(&["box.svg", "line 1", "viewBox"]),
        },
        Case {
            name: "missing_file",
            code: r#"const A: navree_ui::icons::Icon = navree_ui::icon_file!("assets/nope.svg"); fn main() { let _ = A; }"#,
            files: &[("assets/other.svg", Text(GOOD))],
            expect: Expect::Fails(&["nope.svg"]),
        },
        Case {
            name: "path_climbs_out",
            code: r#"const A: navree_ui::icons::Icon = navree_ui::icon_file!("../outside.svg"); fn main() { let _ = A; }"#,
            files: &[],
            expect: Expect::Fails(&["stays inside the crate"]),
        },
        Case {
            name: "absolute_path",
            code: r#"const A: navree_ui::icons::Icon = navree_ui::icon_file!("/etc/passwd"); fn main() { let _ = A; }"#,
            files: &[],
            expect: Expect::Fails(&["stays inside the crate"]),
        },
        Case {
            name: "backslash_path",
            code: r#"const A: navree_ui::icons::Icon = navree_ui::icon_file!("assets\\good.svg"); fn main() { let _ = A; }"#,
            files: &[("assets/good.svg", Text(GOOD))],
            expect: Expect::Fails(&["forward slashes"]),
        },
        Case {
            name: "set_with_a_bad_file",
            code: r#"mod set { navree_ui::icon_set!("assets/set"); } fn main() { let _ = set::ALL.len(); }"#,
            files: &[
                ("assets/set/a-good.svg", Text(GOOD)),
                ("assets/set/script.svg", Fixture("bad-icons/script.svg")),
            ],
            expect: Expect::Fails(&["script.svg", "line 3", "scripts cannot run"]),
        },
        Case {
            name: "set_with_a_name_clash",
            code: r#"mod set { navree_ui::icon_set!("assets/set"); } fn main() { let _ = set::ALL.len(); }"#,
            files: &[
                ("assets/set/a-b.svg", Text(GOOD)),
                ("assets/set/a_b.svg", Text(GOOD)),
            ],
            expect: Expect::Fails(&["another file gives the constant A_B"]),
        },
        Case {
            name: "set_with_no_svg",
            code: r#"mod set { navree_ui::icon_set!("assets/set"); } fn main() { let _ = set::ALL.len(); }"#,
            files: &[("assets/set/readme.txt", Text("nothing to draw"))],
            expect: Expect::Fails(&["holds no .svg files"]),
        },
        Case {
            name: "lucide_name_that_is_not_an_icon",
            code: r#"fn main() { let _ = navree_ui::lucide!("not-an-icon"); }"#,
            files: &[],
            expect: Expect::Fails(&["no Lucide icon is named not-an-icon"]),
        },
        Case {
            name: "flag_code_that_is_not_a_flag",
            code: r#"fn main() { let _ = navree_ui::flag!("ZZ"); }"#,
            files: &[],
            expect: Expect::Fails(&["no flag has the code ZZ"]),
        },
        Case {
            name: "lucide_and_flag_that_exist",
            code: r#"fn main() { let _ = (navree_ui::lucide!("mail-open"), navree_ui::flag!("gb-sct")); }"#,
            files: &[],
            expect: Expect::Builds,
        },
    ]
}

fn fixture(name: &str) -> String {
    let path = Path::new(CRATE).join("tests").join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The crate the cases live in, made once and kept: its target directory is the cache.
fn project() -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("macro-failures");
    fs::create_dir_all(root.join("src/bin")).expect("create the project");
    let manifest = format!(
        "[package]\nname = \"macro-failures\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n\n[dependencies]\nnavree-ui = {{ path = \"{}\" }}\n",
        slashes(Path::new(CRATE))
    );
    fs::write(root.join("Cargo.toml"), manifest).expect("write Cargo.toml");
    // The same dependency versions as this workspace, so the check needs no network.
    let lock = Path::new(CRATE).join("../../Cargo.lock");
    if lock.exists() {
        fs::copy(lock, root.join("Cargo.lock")).expect("copy Cargo.lock");
    }
    root
}

/// Runs `cargo build` on one case: whether it built, and what the compiler said.
fn check(root: &Path, case: &Case) -> (bool, String) {
    let assets = root.join("assets");
    let _ = fs::remove_dir_all(&assets);
    for (path, source) in case.files {
        let full = root.join(path);
        fs::create_dir_all(full.parent().expect("a parent")).expect("create a directory");
        let text = match source {
            Fixture(name) => fixture(name),
            Text(text) => (*text).to_owned(),
        };
        fs::write(full, text).expect("write a file");
    }
    fs::write(root.join(format!("src/bin/{}.rs", case.name)), case.code).expect("write the case");

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .current_dir(root)
        .args([
            "build",
            "--offline",
            "--bin",
            case.name,
            "--message-format",
            "short",
            "--color",
            "never",
        ])
        .env("CARGO_TARGET_DIR", root.join("target"));
    // What cargo set for this test would describe this crate, not the case.
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy().into_owned();
        if name.starts_with("CARGO_PKG_")
            || name.starts_with("CARGO_BIN_")
            || name.starts_with("CARGO_CRATE")
            || name == "CARGO_MANIFEST_DIR"
            || name == "CARGO_PRIMARY_PACKAGE"
        {
            command.env_remove(name);
        }
    }
    let output = command.output().expect("run cargo");
    let said = String::from_utf8_lossy(&output.stderr).replace('\\', "/");
    // Only this case's file: the others stay in the crate and may not build.
    let _ = fs::remove_file(root.join(format!("src/bin/{}.rs", case.name)));
    (output.status.success(), said)
}

#[test]
fn every_bad_icon_name_and_path_stops_the_build_for_its_reason() {
    let root = project();
    let mut problems = Vec::new();
    for case in cases() {
        let (built, said) = check(&root, &case);
        match case.expect {
            Expect::Builds if !built => {
                problems.push(format!("{}: should build, but:\n{said}", case.name))
            }
            Expect::Builds => {}
            Expect::Fails(_) if built => {
                problems.push(format!("{}: built, and should not have", case.name))
            }
            Expect::Fails(parts) => {
                for part in parts {
                    if !said.contains(part) {
                        problems.push(format!(
                            "{}: the compiler did not say {part:?}:\n{said}",
                            case.name
                        ));
                    }
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n---\n"));
}
