//! The examples on the CLI page for the native commands are the ones the
//! commands read: a `native.toml` that parses, a feature that is found, and
//! settings that `navree native:doctor` finds nothing missing in.

use std::path::PathBuf;

use navree_cli::native::{self, Preview};
use navree_cli::native_doctor::{DoctorOptions, Machine, Os, Scope};

fn page() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("digging-deeper")
        .join("cli.mdx");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .replace('\r', "")
}

/// The body of the fenced block that starts ```` ```toml title="<title>" ````.
fn block(title: &str) -> String {
    let page = page();
    let start = format!("```toml title=\"{title}\"\n");
    let at = page
        .find(&start)
        .unwrap_or_else(|| panic!("the CLI page has no `{title}` block"));
    let rest = &page[at + start.len()..];
    let end = rest.find("\n```").expect("a closing fence");
    rest[..end].to_owned()
}

#[test]
fn the_native_toml_on_the_page_is_read() {
    let text = block("native.toml");
    let plan = native::parse(&text, &|name| {
        (name == "COVERAGE_SESSION").then(|| "abc".to_owned())
    })
    .unwrap();
    assert_eq!(plan.values["id"], "1");
    assert_eq!(plan.values["order"], "CS-20481");
    assert_eq!(plan.routes["/orders/{order}/lines/{line}"]["line"], "2");
    assert_eq!(plan.also, ["/orders?filter[status][in]=open&sort=total"]);
    assert_eq!(plan.skip, ["/admin/*"]);
    assert_eq!(plan.headers["cookie"], "navree_session=abc");
}

#[test]
fn the_feature_on_the_page_is_the_one_that_is_found() {
    let manifest: toml::Table = block("Cargo.toml").parse().unwrap();
    assert_eq!(
        native::preview_of(&manifest, &[]),
        Preview::Feature("native-preview".to_owned())
    );
}

/// A machine with nothing on it, so only the settings are judged.
struct Bare;

impl Machine for Bare {
    fn os(&self) -> Os {
        Os::Linux
    }
    fn var(&self, _name: &str) -> Option<String> {
        None
    }
    fn home(&self) -> Option<PathBuf> {
        None
    }
    fn is_dir(&self, _path: &std::path::Path) -> bool {
        false
    }
    fn is_file(&self, _path: &std::path::Path) -> bool {
        false
    }
    fn list(&self, _path: &std::path::Path) -> Vec<String> {
        Vec::new()
    }
    fn run(&self, _program: &str, _args: &[&str]) -> Option<String> {
        None
    }
}

#[test]
fn the_settings_on_the_page_pass_the_checks_of_the_settings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"orders\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("config")).unwrap();
    std::fs::write(
        dir.path().join("config").join("app.toml"),
        block("config/app.toml"),
    )
    .unwrap();
    // The secrets the page says come from `.env`.
    std::fs::write(
        dir.path().join(".env"),
        "APP_NOTIFY__PUSH__APNS__KEY=\"-----BEGIN PRIVATE KEY-----\\nabc\"\nAPP_NOTIFY__PUSH__FCM__SERVICE_ACCOUNT='{\"client_email\":\"a@b.test\",\"private_key\":\"abc\"}'\n",
    )
    .unwrap();
    let mut out = Vec::new();
    let options = DoctorOptions {
        scope: Scope::All,
        url: None,
        json: true,
    };
    navree_cli::native_doctor::run_doctor(dir.path(), &options, &Bare, &mut out).unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    for item in report["items"].as_array().unwrap() {
        let group = item["group"].as_str().unwrap();
        if group == "settings" {
            assert_ne!(
                item["status"], "missing",
                "the page's settings leave this missing: {item}"
            );
        }
    }
    let status = |id: &str| {
        report["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .unwrap_or_else(|| panic!("no item {id}"))["status"]
            .clone()
    };
    for id in [
        "auth.apple",
        "auth.android",
        "auth.paths",
        "links.apple",
        "links.android",
        "passkeys",
        "push.apple",
        "push.android",
    ] {
        assert_eq!(status(id), "ready", "{id}: {report}");
    }
    // Secrets stay out of what is printed.
    assert!(!report.to_string().contains("BEGIN PRIVATE KEY"));
}
