//! The `navree` binary end to end: exit codes, output and forwarding.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

fn workspace() -> PathBuf {
    std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")).unwrap()
}

#[test]
fn new_then_make_module_from_inside_the_app() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let workspace = workspace();
    let output = navree(
        dir.path(),
        &["new", "blog", "--navree-path", workspace.to_str().unwrap()],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("Created blog"),
        "{}",
        stdout(&output)
    );
    let root = dir.path().join("blog");
    let manifest: toml::Table = std::fs::read_to_string(root.join("Cargo.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let navree_path = manifest["dependencies"]["navree"]["path"].as_str().unwrap();
    assert!(!navree_path.contains('\\'), "{navree_path}");
    assert!(
        Path::new(navree_path).join("Cargo.toml").is_file(),
        "{navree_path} is the facade crate"
    );

    // From a subdirectory: navree finds the app.
    let output = navree(&root.join("src"), &["make:module", "posts", "--crud"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(
        out.contains("created  src/modules/posts/controller.rs"),
        "{out}"
    );
    assert!(out.contains("updated  src/app.rs"), "{out}");
    assert!(root.join("src/modules/posts/entity.rs").is_file());

    let output = navree(&root, &["make:job", "posts", "publish_post"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        root.join("src/modules/posts/jobs/publish_post.rs")
            .is_file()
    );
}

#[test]
fn invalid_names_fail_with_a_suggestion() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let output = navree(dir.path(), &["make:module", "BlogPosts"]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(
        err.contains("snake_case") && err.contains("`blog_posts`"),
        "{err}"
    );

    let output = navree(dir.path(), &["new", "My App"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("invalid app name"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn make_outside_an_app_fails() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let output = navree(dir.path(), &["make:service", "posts", "pricing"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("not inside a Rust package"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn runtime_commands_run_in_the_app_binary_with_their_exit_code() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let app = dir.path();
    std::fs::create_dir(app.join("src")).unwrap();
    std::fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"echo-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(
        app.join("src").join("main.rs"),
        "fn main() {\n    let args: Vec<String> = std::env::args().skip(1).collect();\n    println!(\"app got: {}\", args.join(\" \"));\n    std::process::exit(if args[0] == \"fail\" { 3 } else { 0 });\n}\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_navree"))
            .args(args)
            .current_dir(app)
            .env("CARGO_TARGET_DIR", app.join("target"))
            .env_remove("RUSTC_WRAPPER")
            .output()
            .expect("navree runs")
    };

    let output = run(&["migrate:fresh", "--seed"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output).trim(), "app got: migrate:fresh --seed");

    let output = run(&["fail", "--queue", "a,b"]);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert_eq!(stdout(&output).trim(), "app got: fail --queue a,b");
}

#[test]
fn forwarded_commands_get_the_dock_settings() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let app = dir.path();
    std::fs::create_dir(app.join("src")).unwrap();
    std::fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"env-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(
        app.join("src").join("main.rs"),
        "fn main() {\n    for name in [\"REDIS_URL\", \"APP_MICRO__URL\"] {\n        println!(\"{name}={}\", std::env::var(name).unwrap_or_default());\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(
        app.join("dock.toml"),
        "[services.redis]\nport = 6380\n\n[services.nats]\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_navree"))
        .arg("show-env")
        .current_dir(app)
        .env("CARGO_TARGET_DIR", app.join("target"))
        .env_remove("RUSTC_WRAPPER")
        .env_remove("REDIS_URL")
        // An explicit variable wins over dock.toml.
        .env("APP_MICRO__URL", "nats://elsewhere:4222")
        .output()
        .expect("navree runs");
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "REDIS_URL=redis://127.0.0.1:6380\nAPP_MICRO__URL=nats://elsewhere:4222\n"
    );
}

#[test]
fn dock_add_env_and_remove_need_no_container_runtime() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"crm-api\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir(&src).unwrap();

    // From a subdirectory: navree finds the app.
    let output = navree(&src, &["dock:add", "postgres"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "  created  dock.toml        # [services.postgres]\nAdded postgres. Your app needs navree's `postgres` feature.\n"
    );
    let output = navree(dir.path(), &["dock:add", "mailpit"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let output = navree(dir.path(), &["dock", "env"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "APP_DATABASE__URL=postgres://navree:navree@127.0.0.1:5432/crm_api\nAPP_MAIL__TRANSPORT=smtp://127.0.0.1:1025\n"
    );
    let output = navree(dir.path(), &["dock", "env", "--export"]);
    assert!(
        stdout(&output).starts_with("export APP_DATABASE__URL="),
        "{}",
        stdout(&output)
    );

    let output = navree(dir.path(), &["dock:add", "mysql"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("unknown service `mysql`"),
        "{}",
        stderr(&output)
    );

    std::fs::write(
        dir.path().join("dock.toml"),
        "[services.redis]\nport = \"x\"\n",
    )
    .unwrap();
    let output = navree(dir.path(), &["dock", "env"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("dock.toml: [services.redis]: `port` must be a port number"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn harbor_commands_run_in_the_cli_not_in_the_app() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let app = dir.path();
    std::fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_navree"))
            .args(args)
            .current_dir(app)
            .env_remove("NAVREE_HARBOR_ENV")
            .env_remove("NAVREE_HARBOR_KEY")
            .env_remove("NAVREE_HARBOR_SSH_KEY")
            .output()
            .expect("navree runs")
    };

    let output = run(&["harbor:init"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("created  harbor.toml"),
        "{}",
        stdout(&output)
    );
    assert!(app.join("harbor.key").is_file());

    let output = run(&["harbor:secrets", "set", "APP_KEY", "--generate"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let output = run(&["harbor:secrets", "list"]);
    assert_eq!(stdout(&output).trim(), "APP_KEY");

    // A deploy needs the host key that harbor:provision records.
    let output = run(&["deploy"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("run `navree harbor:provision app-1`"),
        "{}",
        stderr(&output)
    );
}
