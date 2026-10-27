//! `navree new <name> --kit saas|api|micro`: the files each starter kit
//! writes, their settings, and what `navree new` prints. (The kits compile
//! and pass their own tests in `generated_kits.rs`.)

use std::path::{Path, PathBuf};
use std::process::Command;

use navree_cli::clock::Timestamp;
use navree_cli::kits::{KIT_VERSION, Kit};
use navree_cli::names::AppName;
use navree_cli::new_app::{NavreeSource, NewApp, Ui, create_app, created_message};

fn now() -> Timestamp {
    Timestamp::from_unix(1_792_938_605)
}

fn app(name: &str, kit: Kit) -> NewApp {
    NewApp {
        name: AppName::parse(name).unwrap(),
        ui: Ui::None,
        kit: Some(kit),
        navree: NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree")),
    }
}

fn generate(name: &str, kit: Kit) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = create_app(dir.path(), &app(name, kit), now()).unwrap();
    (dir, root)
}

/// Every file under `root`, relative, with `/` separators, sorted.
fn tree(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                let parts: Vec<_> = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push(parts.join("/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn read(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn toml(root: &Path, path: &str) -> toml::Table {
    read(root, path)
        .parse()
        .unwrap_or_else(|e| panic!("{path} is not TOML: {e}"))
}

fn assert_has(files: &[String], expected: &[&str]) {
    let missing: Vec<_> = expected
        .iter()
        .filter(|path| !files.iter().any(|file| file == *path))
        .collect();
    assert!(missing.is_empty(), "missing {missing:?} in {files:#?}");
}

/// The files the docs' layout of the SaaS and API kits names.
const API_FILES: &[&str] = &[
    ".env",
    ".env.example",
    ".github/workflows/ci.yml",
    ".gitignore",
    "AGENTS.md",
    "CLAUDE.md",
    "Cargo.toml",
    "README.md",
    "config/app.toml",
    "dock.toml",
    "harbor.toml",
    "src/abilities.rs",
    "src/app.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/modules/accounts/mod.rs",
    "src/modules/admin/mod.rs",
    "src/modules/mod.rs",
    "src/modules/projects/controller.rs",
    "src/modules/projects/entity.rs",
    "src/modules/projects/mod.rs",
    "src/modules/projects/migrations/m20261025_143005_create_projects_table.rs",
    "src/modules/settings/mod.rs",
    "src/platform.rs",
    "src/seed.rs",
    "storage/.gitignore",
    "tests/app.rs",
    "tests/billing.rs",
    "tests/sign_up.rs",
];

#[test]
fn the_saas_kit_writes_the_api_and_the_web_app() {
    let (_dir, root) = generate("acme", Kit::Saas);
    let files = tree(&root);
    assert_has(&files, API_FILES);
    assert_has(
        &files,
        &[
            "web/.env.example",
            "web/components.json",
            "web/package.json",
            "web/app/(auth)/sign-in/page.tsx",
            "web/app/(auth)/sign-up/page.tsx",
            "web/app/(auth)/verify-email/page.tsx",
            "web/app/(auth)/forgot-password/page.tsx",
            "web/app/(auth)/reset-password/page.tsx",
            "web/app/(auth)/two-factor/page.tsx",
            "web/app/(app)/w/[workspace]/page.tsx",
            "web/app/(app)/w/[workspace]/projects/page.tsx",
            "web/app/(app)/w/[workspace]/projects/[id]/page.tsx",
            "web/app/(app)/w/[workspace]/settings/members/page.tsx",
            "web/app/(app)/w/[workspace]/settings/billing/page.tsx",
            "web/app/(app)/w/[workspace]/settings/profile/page.tsx",
            "web/app/(app)/w/[workspace]/settings/security/page.tsx",
            "web/app/(app)/w/[workspace]/settings/notifications/page.tsx",
            "web/app/api/[...path]/route.ts",
            "web/app/api/auth/login/route.ts",
            "web/app/api/auth/logout/route.ts",
            "web/app/api/auth/refresh/route.ts",
            "web/app/api/auth/2fa/verify/route.ts",
            "web/app/admin/[[...path]]/route.ts",
            "web/app/broadcasting/auth/route.ts",
            "web/app/invitations/accept/page.tsx",
            "web/components/workspace-switcher.tsx",
            "web/components/ui/button.tsx",
            "web/lib/bff/bff.ts",
            "web/lib/bff/bff.test.ts",
            "web/lib/api/client.ts",
            "web/lib/realtime.ts",
            "web/e2e/sign-up.spec.ts",
            "web/e2e/two-factor.spec.ts",
            "web/e2e/workspace.spec.ts",
        ],
    );
    // The front end is named after the app and pins its package manager.
    let package: serde_json::Value = serde_json::from_str(&read(&root, "web/package.json")).unwrap();
    assert_eq!(package["name"], "acme-web");
    assert!(
        package["packageManager"]
            .as_str()
            .unwrap()
            .starts_with("pnpm@")
    );
    for script in ["dev", "build", "lint", "typecheck", "test", "e2e", "gen:api"] {
        assert!(package["scripts"].get(script).is_some(), "{script}");
    }
    // The browser reaches the API through the backend-for-frontend.
    assert!(read(&root, "web/.env.example").contains("NAVREE_API_URL="));
    // The API knows the web app's origin.
    let config = toml(&root, "config/app.toml");
    assert_eq!(config["app"]["url"].as_str(), Some("http://localhost:3000"));
    assert_eq!(
        config["http"]["cors"]["allowed_origins"][0].as_str(),
        Some("http://localhost:3000")
    );
    // Harbor ships the web app in the same release.
    let harbor = navree_harbor::HarborConfig::parse(&read(&root, "harbor.toml"), Path::new("harbor.toml")).unwrap();
    assert_eq!(harbor.frontend.as_ref().map(|f| f.path.as_str()), Some("web"));
    // CI covers the web app.
    assert!(read(&root, ".github/workflows/ci.yml").contains("pnpm build"));
}

#[test]
fn the_api_kit_is_the_saas_api_without_the_web_app() {
    let (_dir, saas) = generate("acme", Kit::Saas);
    let (_dir2, api) = generate("acme", Kit::Api);
    let api_files = tree(&api);
    assert_has(&api_files, API_FILES);
    assert!(api_files.iter().all(|file| !file.starts_with("web/")));
    // The same Rust code (doc comments name the kit).
    let rust: Vec<_> = api_files.iter().filter(|f| f.ends_with(".rs")).collect();
    assert!(rust.len() > 30, "{rust:?}");
    for file in rust {
        assert_eq!(
            read(&api, file),
            read(&saas, file).replace("`saas`", "`api`"),
            "{file}"
        );
    }
    let config = toml(&api, "config/app.toml");
    assert_eq!(config["app"]["url"].as_str(), Some("http://localhost:8080"));
    assert!(config["http"].get("cors").is_none());
    let harbor = navree_harbor::HarborConfig::parse(&read(&api, "harbor.toml"), Path::new("harbor.toml")).unwrap();
    assert!(harbor.frontend.is_none());
    assert!(!read(&api, ".github/workflows/ci.yml").contains("pnpm"));
    assert!(read(&api, ".github/workflows/ci.yml").contains("openapi:export"));
}

#[test]
fn saas_and_api_kits_configure_the_documented_modules() {
    let (_dir, root) = generate("acme", Kit::Saas);
    let platform = read(&root, "src/platform.rs");
    for module in [
        "ConfigModule",
        "LoggerModule",
        "CryptoModule",
        "HttpModule",
        "DatabaseModule",
        "CacheModule",
        "EventsModule",
        "ViewModule",
        "MailModule",
        "QueueModule",
        "ScheduleModule",
        "ValidationModule",
        "AuthModule",
        "AccessModule",
        "WorkspaceModule",
        "BillingModule",
        "SocialModule",
        "NotifyModule",
        "RealtimeModule",
        "AuditModule",
        "ApiModule",
        "ObserveModule",
        "HelmModule",
    ] {
        assert!(platform.contains(&format!(".module({module}")), "{module}");
    }
    let app = read(&root, "src/app.rs");
    for module in [
        "AccountsModule",
        "SettingsModule",
        "ProjectsModule",
        "AdminModule",
    ] {
        assert!(app.contains(module), "{module}");
    }
    assert!(app.contains("// navree:modules"));
    let config = toml(&root, "config/app.toml");
    assert_eq!(config["auth"]["registration"].as_bool(), Some(true));
    assert_eq!(
        config["workspace"]["resolvers"].as_array().unwrap()[0].as_str(),
        Some("header")
    );
    assert_eq!(config["billing"]["provider"].as_str(), Some("fake"));
    let plans = config["billing"]["plans"].as_table().unwrap();
    assert!(plans.contains_key("starter") && plans.contains_key("pro"));
}

#[test]
fn every_kit_records_its_kit_and_its_navree_features() {
    for (kit, metadata) in [
        (Kit::Saas, &["package", "metadata", "navree"]),
        (Kit::Api, &["package", "metadata", "navree"]),
        (Kit::Micro, &["workspace", "metadata", "navree"]),
    ] {
        let (_dir, root) = generate("acme", kit);
        let manifest = toml(&root, "Cargo.toml");
        let whole = toml::Value::Table(manifest.clone());
        let navree = metadata.iter().fold(&whole, |value, key| &value[*key]);
        assert_eq!(navree["kit"].as_str(), Some(kit.name()), "{kit}");
        assert_eq!(navree["kit_version"].as_str(), Some(KIT_VERSION), "{kit}");
        let dependency = match kit {
            Kit::Micro => &manifest["workspace"]["dependencies"]["navree"],
            _ => &manifest["dependencies"]["navree"],
        };
        assert_eq!(
            dependency["path"].as_str(),
            Some("/opt/navree/crates/navree"),
            "{kit}"
        );
        if kit == Kit::Micro {
            assert_eq!(dependency["default-features"].as_bool(), Some(false));
            let service = toml(&root, "crates/acme/Cargo.toml");
            let features = service["dependencies"]["navree"]["features"].as_array().unwrap();
            for feature in ["full", "micro", "micro-nats", "postgres"] {
                assert!(features.iter().any(|f| f.as_str() == Some(feature)), "{feature}");
            }
        } else {
            let features = dependency["features"].as_array().unwrap();
            for feature in ["billing", "social", "postgres"] {
                assert!(features.iter().any(|f| f.as_str() == Some(feature)), "{feature}");
            }
        }
    }
}

#[test]
fn the_micro_kit_is_a_workspace_of_the_service_and_its_contracts() {
    let (_dir, root) = generate("billing", Kit::Micro);
    let files = tree(&root);
    assert_has(
        &files,
        &[
            ".env",
            ".env.example",
            ".github/workflows/ci.yml",
            ".gitignore",
            "AGENTS.md",
            "CLAUDE.md",
            "Cargo.toml",
            "README.md",
            "config/app.toml",
            "dock.toml",
            "harbor.toml",
            "storage/.gitignore",
            "crates/billing-contracts/Cargo.toml",
            "crates/billing-contracts/src/lib.rs",
            "crates/billing/Cargo.toml",
            "crates/billing/src/main.rs",
            "crates/billing/src/lib.rs",
            "crates/billing/src/app.rs",
            "crates/billing/src/modules/mod.rs",
            "crates/billing/src/modules/invoices/mod.rs",
            "crates/billing/src/modules/invoices/handlers.rs",
            "crates/billing/src/modules/invoices/service.rs",
            "crates/billing/src/modules/invoices/entity.rs",
            "crates/billing/src/modules/invoices/migrations/mod.rs",
            "crates/billing/src/modules/invoices/migrations/m20261025_143005_create_invoices_table.rs",
            "crates/billing/tests/app.rs",
            "crates/billing/tests/invoices.rs",
            "crates/billing/tests/support/mod.rs",
        ],
    );
    assert!(files.iter().all(|file| !file.starts_with("web/") && !file.starts_with("src/")));

    // `cargo run` (and `navree dev`) run the service.
    let manifest = toml(&root, "Cargo.toml");
    assert_eq!(
        manifest["workspace"]["default-members"].as_array().unwrap()[0].as_str(),
        Some("crates/billing")
    );
    assert_eq!(
        navree_cli::project::run_root(&root.join("crates").join("billing").join("src")),
        Some(root.join("crates").join("billing"))
    );
    assert_eq!(navree_cli::project::run_root(&root), Some(root.clone()));
    // The service depends on its contracts.
    let service = toml(&root, "crates/billing/Cargo.toml");
    assert_eq!(service["package"]["name"].as_str(), Some("billing"));
    assert!(service["dependencies"].get("billing-contracts").is_some());
    let contracts = read(&root, "crates/billing-contracts/src/lib.rs");
    for item in [
        "pub struct GetInvoice",
        "pub struct Invoice",
        "pub struct DealWon",
        "pub struct InvoiceDrafted",
        "const NAME: &'static str = \"deal.won\"",
    ] {
        assert!(contracts.contains(item), "{item}");
    }
    let handlers = read(&root, "crates/billing/src/modules/invoices/handlers.rs");
    assert!(handlers.contains("#[message_pattern(\"invoices.get\")]"));
    assert!(handlers.contains("#[event_pattern(\"deal.won\")]"));
    assert!(handlers.contains("use billing_contracts::"));
    assert!(read(&root, "crates/billing/src/modules/invoices/mod.rs").contains("m.migrations(migrations::migrations())"));

    // NATS is the transport, from Dock in development.
    let config = toml(&root, "config/app.toml");
    assert_eq!(config["micro"]["transport"].as_str(), Some("nats"));
    assert_eq!(config["micro"]["service"].as_str(), Some("billing"));
    let dock = navree_dock::DockFile::parse(&read(&root, "dock.toml"), "billing").unwrap();
    assert!(dock.service("postgres").is_ok() && dock.service("nats").is_ok());
    let env = read(&root, ".env");
    assert!(env.contains("APP_MICRO__URL=nats://127.0.0.1:4222"), "{env}");
    assert!(env.contains("APP_DATABASE__URL=postgres://"), "{env}");
}

#[test]
fn env_points_at_the_services_dock_starts() {
    let (_dir, root) = generate("acme", Kit::Saas);
    let env = read(&root, ".env");
    let example = read(&root, ".env.example");
    // `.env.example`'s settings, then Dock's between its markers.
    for line in example.lines().filter(|line| line.starts_with("APP_")) {
        assert!(env.lines().any(|l| l == line), "{line} in {env}");
    }
    assert!(env.contains("# dock:start"), "{env}");
    assert!(env.contains("APP_DATABASE__URL=postgres://navree:navree@127.0.0.1:5432/"), "{env}");
    assert!(env.contains("APP_MAIL__TRANSPORT=smtp://127.0.0.1:1025"), "{env}");
    assert!(!example.contains("# dock:start"));
    // Secrets are placeholders, never values.
    assert!(env.contains("APP_KEY=\n"));
}

#[test]
fn every_kit_s_harbor_and_dock_files_are_valid() {
    for kit in Kit::ALL {
        let (_dir, root) = generate("acme", kit);
        let harbor = navree_harbor::HarborConfig::parse(&read(&root, "harbor.toml"), &root.join("harbor.toml"))
            .unwrap_or_else(|e| panic!("{kit}: {e}"));
        assert_eq!(harbor.app, "acme");
        assert_eq!(harbor.validate(), Vec::<String>::new(), "{kit}");
        assert_eq!(harbor.deploy.health_check, "/health/ready");
        navree_dock::DockFile::parse(&read(&root, "dock.toml"), "acme").unwrap_or_else(|e| panic!("{kit}: {e}"));
    }
}

#[test]
fn generated_files_are_free_of_template_syntax() {
    for kit in Kit::ALL {
        let (_dir, root) = generate("acme-cloud", kit);
        for file in tree(&root) {
            let text = read(&root, &file);
            for marker in ["{%", "%}", "{{ package", "{{ crate_name", "{{ title", "{{ kit", "__package__", "mNOW"] {
                assert!(!text.contains(marker), "{kit}: {file} contains {marker}");
            }
            assert!(!file.contains("__package__") && !file.ends_with(".j2"), "{file}");
            assert!(!text.contains('\r'), "{kit}: {file} has CRLF line endings");
        }
    }
}

#[test]
fn the_created_message_lists_the_kit_s_next_steps() {
    let (_dir, root) = generate("acme", Kit::Saas);
    let message = created_message(&app("acme", Kit::Saas), &root);
    assert!(message.starts_with(&format!("Created acme in {} (kit: saas)\n", root.display())));
    for line in [
        "  cd acme\n",
        "  navree dock up                 # PostgreSQL and Mailpit\n",
        "  navree key:generate            # then set APP_KEY in .env\n",
        "  navree migrate:fresh --seed    # tables and a demo workspace\n",
        "  navree dev                     # API on :8080, web app on :3000\n",
    ] {
        assert!(message.contains(line), "{line:?} in\n{message}");
    }
    let micro = created_message(&app("billing", Kit::Micro), &root);
    assert!(micro.contains("(kit: micro)"));
    assert!(micro.contains("# PostgreSQL and NATS"));
    let api = created_message(&app("acme", Kit::Api), &root);
    assert!(api.contains("(kit: api)"));
    assert!(!api.contains("pnpm"));
}

#[test]
fn kits_refuse_a_directory_that_is_not_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("acme")).unwrap();
    std::fs::write(dir.path().join("acme").join("notes.txt"), "mine").unwrap();
    assert!(create_app(dir.path(), &app("acme", Kit::Saas), now()).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("acme").join("notes.txt")).unwrap(),
        "mine"
    );
}

#[test]
fn a_published_navree_keeps_the_kit_features() {
    let source = NavreeSource::Version("0.1".into());
    assert_eq!(
        source.dependency_with_features(&["billing", "social"]),
        r#"{ version = "0.1", features = ["billing", "social"] }"#
    );
    assert_eq!(
        source.dependency_without_default_features(),
        r#"{ version = "0.1", default-features = false }"#
    );
    let path = NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree"));
    assert_eq!(
        path.dependency_without_default_features(),
        r#"{ path = "/opt/navree/crates/navree", default-features = false }"#
    );
}

/// Every `.rs` file under `dir`.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The kits' Rust is rustfmt-clean as written, so apps look right even
/// where `navree new` cannot run rustfmt. Skipped without rustfmt.
#[test]
fn kit_rust_is_rustfmt_clean() {
    if Command::new("rustfmt").arg("--version").output().is_err() {
        eprintln!("rustfmt is not installed; skipping");
        return;
    }
    let mut unformatted = Vec::new();
    for kit in [Kit::Api, Kit::Micro] {
        let (_dir, root) = generate("acme", kit);
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        for file in files {
            let output = Command::new("rustfmt")
                .args(["--edition", "2024", "--check", "--config", "skip_children=true"])
                .arg(&file)
                .output()
                .unwrap();
            // Migration sets stay one call per line so `make:migration`
            // can extend them.
            let migration_set = file.ends_with(Path::new("migrations").join("mod.rs"));
            if !output.status.success() && !migration_set {
                unformatted.push(format!(
                    "{kit}: {}\n{}",
                    file.strip_prefix(&root).unwrap().display(),
                    String::from_utf8_lossy(&output.stdout)
                ));
            }
        }
    }
    assert!(unformatted.is_empty(), "not rustfmt-clean:\n{}", unformatted.join("\n"));
}
