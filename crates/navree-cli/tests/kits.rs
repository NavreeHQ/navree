//! `navree new <name> --kit saas|api|micro`: the files each starter kit
//! writes, their settings, and what `navree new` prints. (The kits compile
//! and pass their own tests in `generated_kits.rs`.)

use std::path::{Path, PathBuf};
use std::process::Command;

use navree_cli::clock::Timestamp;
use navree_cli::kits::{KIT_VERSION, Kit};
use navree_cli::names::AppName;
use navree_cli::new_app::{Database, NavreeSource, NewApp, Ui, create_app, created_message};

fn now() -> Timestamp {
    Timestamp::from_unix(1_792_938_605)
}

fn app(name: &str, kit: Kit) -> NewApp {
    NewApp {
        name: AppName::parse(name).unwrap(),
        ui: Ui::None,
        kit: Some(kit),
        database: Database::default(),
        navree: NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree")),
    }
}

fn generate(name: &str, kit: Kit) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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
            "web/app/api/auth/email/verify/[id]/[hash]/route.ts",
            "web/app/admin/[[...path]]/route.ts",
            "web/app/auth/callback/[provider]/route.ts",
            "web/app/auth/social/[provider]/route.ts",
            "web/app/broadcasting/auth/route.ts",
            "web/app/invitations/accept/page.tsx",
            "web/components/workspace-switcher.tsx",
            "web/components/ui/button.tsx",
            "web/lib/navree.ts",
            "web/lib/social.ts",
            "web/lib/social.test.ts",
            "web/lib/api/client.ts",
            "web/next.config.ts",
            "web/proxy.ts",
            "web/README.md",
            "web/e2e/sign-up.spec.ts",
            "web/e2e/two-factor.spec.ts",
            "web/e2e/workspace.spec.ts",
        ],
    );
    // The front end is named after the app and pins its package manager.
    let package: serde_json::Value =
        serde_json::from_str(&read(&root, "web/package.json")).unwrap();
    assert_eq!(package["name"], "acme-web");
    assert!(
        package["packageManager"]
            .as_str()
            .unwrap()
            .starts_with("pnpm@")
    );
    for script in [
        "dev",
        "build",
        "lint",
        "typecheck",
        "test",
        "e2e",
        "gen:api",
    ] {
        assert!(package["scripts"].get(script).is_some(), "{script}");
    }
    // shadcn/ui's stylesheet is vendored, so the app does not install the
    // shadcn CLI and its dependencies (`pnpm dlx shadcn@latest add` runs it).
    assert!(package["dependencies"].get("shadcn").is_none());
    assert!(package["devDependencies"].get("shadcn").is_none());
    assert!(!read(&root, "web/pnpm-lock.yaml").contains("shadcn"));
    assert!(read(&root, "web/app/globals.css").contains("@import \"./shadcn-tailwind.css\";"));
    // Tailwind scans only the web app's own source folders, never
    // node_modules, whether or not a Git repository surrounds the app.
    let css = read(&root, "web/app/globals.css");
    assert!(
        css.contains("@import \"tailwindcss\" source(none);"),
        "{css}"
    );
    for folder in ["app", "components", "hooks", "lib"] {
        assert!(
            css.contains(&format!("@source \"../{folder}\";")),
            "{folder}"
        );
        assert!(root.join("web").join(folder).is_dir(), "web/{folder}");
    }
    assert!(!css.contains("@source \"../node_modules"), "{css}");
    assert!(read(&root, "web/app/shadcn-tailwind.css").contains("@custom-variant data-open"));
    // The browser reaches the API through the backend-for-frontend; the
    // web app's local settings start as a copy of the example.
    assert!(read(&root, "web/.env.example").contains("NAVREE_API_URL="));
    assert_eq!(
        read(&root, "web/.env.local"),
        read(&root, "web/.env.example")
    );
    assert!(read(&root, "web/.env.local").contains("NEXT_PUBLIC_REALTIME_KEY=acme\n"));
    assert!(read(&root, ".gitignore").contains("/web/.env.local"));
    // The API knows the web app's origin.
    let config = toml(&root, "config/app.toml");
    assert_eq!(config["app"]["url"].as_str(), Some("http://localhost:3000"));
    assert_eq!(
        config["http"]["cors"]["allowed_origins"][0].as_str(),
        Some("http://localhost:3000")
    );
    // Harbor ships the web app in the same release.
    let harbor =
        navree_harbor::HarborConfig::parse(&read(&root, "harbor.toml"), Path::new("harbor.toml"))
            .unwrap();
    assert_eq!(
        harbor.frontend.as_ref().map(|f| f.path.as_str()),
        Some("web")
    );
    // CI covers the web app, from a committed lockfile.
    assert!(read(&root, ".github/workflows/ci.yml").contains("pnpm build"));
    assert!(read(&root, "web/pnpm-lock.yaml").starts_with("lockfileVersion:"));
    // The browser's types come from the committed OpenAPI document.
    let schema = read(&root, "web/lib/api/schema.d.ts");
    assert!(schema.contains("\"/api/v1/projects\""));
    assert!(schema.contains("ProjectPage: {"));
}

#[test]
fn the_saas_kit_names_the_browser_to_the_api_through_exactly_the_bff_hop() {
    let (_dir, root) = generate("acme", Kit::Saas);
    // The API believes X-Forwarded-For from loopback, where the web app's
    // backend-for-frontend calls from, and from no other peer.
    let config = toml(&root, "config/app.toml");
    let trusted: Vec<_> = config["http"]["trusted_proxies"]
        .as_array()
        .expect("http.trusted_proxies")
        .iter()
        .map(|entry| entry.as_str().unwrap())
        .collect();
    assert_eq!(trusted, ["127.0.0.1", "::1"]);
    assert!(config["http"].get("trust_proxy").is_none());
    // ... and it is a setting: the API's environment moves it, the web
    // app's names the proxies and the header it believes.
    assert!(read(&root, ".env.example").contains("# APP_HTTP__TRUSTED_PROXIES=127.0.0.1,::1"));
    let web_env = read(&root, "web/.env.example");
    assert!(web_env.contains("# NAVREE_TRUSTED_PROXY_HOPS=1"));
    assert!(web_env.contains("# NAVREE_CLIENT_IP_HEADER=x-real-ip"));
    // `@navree/next` reads both (it takes its settings from the environment when
    // the first request arrives) and forwards the client address.
    let navree = read(&root, "web/lib/navree.ts");
    assert!(navree.contains("createNavree()"));
    assert!(!navree.contains("trustedProxyHops"));
    // The API kit has no web app, so no proxy to trust.
    let (_dir2, api) = generate("acme", Kit::Api);
    assert!(
        toml(&api, "config/app.toml")["http"]
            .get("trusted_proxies")
            .is_none()
    );
    assert!(!read(&api, ".env.example").contains("APP_HTTP__TRUSTED_PROXIES"));
}

/// The files of the generated app under `web/`, relative to the app.
fn web_files(root: &Path) -> Vec<String> {
    tree(root)
        .into_iter()
        .filter(|file| file.starts_with("web/"))
        .collect()
}

#[test]
fn the_saas_kits_web_app_runs_on_the_navree_packages_and_carries_no_bff_of_its_own() {
    let (_dir, root) = generate("acme", Kit::Saas);
    let files = web_files(&root);

    // The copies are gone: the BFF, its error bodies, the real-time, abilities and per-route code.
    for gone in [
        "web/lib/bff/bff.ts",
        "web/lib/bff/bff.test.ts",
        "web/lib/bff/http-errors.ts",
        "web/lib/bff/server.ts",
        "web/lib/realtime.ts",
        "web/lib/abilities.ts",
        "web/lib/abilities.test.ts",
        "web/app/api/auth/login/route.ts",
        "web/app/api/auth/logout/route.ts",
        "web/app/api/auth/refresh/route.ts",
        "web/app/api/auth/2fa/verify/route.ts",
    ] {
        assert!(
            !files.iter().any(|file| file == gone),
            "{gone} is still written"
        );
    }
    assert!(files.iter().all(|file| !file.starts_with("web/lib/bff/")));

    // The route handlers are the package's.
    for route in [
        "web/app/api/[...path]/route.ts",
        "web/app/admin/[[...path]]/route.ts",
        "web/app/broadcasting/auth/route.ts",
    ] {
        assert!(read(&root, route).contains("navree.handlers"), "{route}");
    }
    assert!(read(&root, "web/lib/navree.ts").contains("createNavree()"));
    // The admin panel is behind sign-in in the proxy, and the sign-in page is the kit's.
    let proxy = read(&root, "web/proxy.ts");
    assert!(
        proxy.contains("navree.proxy({ loginPath: \"/sign-in\" })"),
        "{proxy}"
    );
    assert!(proxy.contains("matcher: [\"/admin/:path*\"]"), "{proxy}");
    // Live updates and abilities use the packages' entry points.
    assert!(
        read(&root, "web/app/(app)/w/[workspace]/layout.tsx")
            .contains("from \"@navree/next/realtime\"")
    );
    assert!(read(&root, "web/lib/queries.ts").contains("from \"@navree/client/abilities\""));

    // The packages are linked from the checkout `navree` comes from.
    let package: serde_json::Value =
        serde_json::from_str(&read(&root, "web/package.json")).unwrap();
    assert_eq!(
        package["dependencies"]["@navree/next"],
        "link:/opt/navree/packages/js/next"
    );
    assert_eq!(
        package["dependencies"]["@navree/client"],
        "link:/opt/navree/packages/js/client"
    );
    // ... which sits outside the app: Next may read it, and compiles it with the app.
    let config = read(&root, "web/next.config.ts");
    for needle in [
        "\"/opt/navree/packages/js\"",
        "turbopack: { root }",
        "outputFileTracingRoot: root",
        "transpilePackages: [\"@navree/next\", \"@navree/client\", \"@navree/bff\"]",
        "cacheComponents: false",
    ] {
        assert!(config.contains(needle), "{needle} in\n{config}");
    }
    assert!(!config.contains("{{") && !config.contains("{%"), "{config}");
    // The lockfile pins the other packages; the first install adds the two links.
    assert!(!read(&root, "web/pnpm-lock.yaml").contains("@navree/"));
    assert!(read(&root, "web/README.md").contains("without `--frozen-lockfile`"));

    // No file of the web app keeps tokens or the token cookies itself. (Only the social
    // sign-in, which the package does not have, writes the cookies of its own flow.)
    for file in &files {
        let Ok(text) = std::fs::read_to_string(root.join(file)) else {
            continue;
        };
        for needle in [
            "ACCESS_COOKIE",
            "REFRESH_COOKIE",
            "createBff(",
            "originAllowed",
        ] {
            let allowed = file == "web/lib/social.test.ts" && needle == "createBff(";
            assert!(
                allowed || !text.contains(needle),
                "{file} mentions {needle}"
            );
        }
        if file != "web/lib/social.ts" && file != "web/lib/social.test.ts" {
            assert!(!text.contains("navree_at"), "{file} mentions navree_at");
        }
    }
}

#[test]
fn the_saas_kits_web_app_from_a_published_navree_asks_npm_for_the_packages() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let published = NewApp {
        navree: NavreeSource::Version("1.0.0".to_owned()),
        ..app("acme", Kit::Saas)
    };
    let root = create_app(dir.path(), &published, now()).unwrap();

    let package: serde_json::Value =
        serde_json::from_str(&read(&root, "web/package.json")).unwrap();
    assert_eq!(package["dependencies"]["@navree/next"], "^1.0.0");
    assert_eq!(package["dependencies"]["@navree/client"], "^1.0.0");
    let config = read(&root, "web/next.config.ts");
    for needle in [
        "turbopack",
        "outputFileTracingRoot",
        "transpilePackages",
        "node:path",
    ] {
        assert!(!config.contains(needle), "{needle} in\n{config}");
    }
    assert!(config.contains("cacheComponents: false"));
    assert!(read(&root, "web/README.md").contains("not on npm yet"));
}

#[test]
fn no_template_carries_a_backend_for_frontend_of_its_own() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let templates = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    let mut files = Vec::new();
    walk(&templates, &mut files);
    assert!(files.len() > 100, "{} templates", files.len());
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let name = file
            .strip_prefix(&templates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        // The only tests that name `createBff` give the social routes a BFF with a scripted API.
        let test_of_social = name == "kits/web/lib/social.test.ts";
        for implementation in [
            "export function createBff",
            "function callNavreeRefresh",
            "function refresh(",
            "single-flight",
            "ACCESS_COOKIE",
            "REFRESH_COOKIE",
            "originAllowed",
        ] {
            assert!(
                !text.contains(implementation),
                "{name} contains {implementation}"
            );
        }
        assert!(
            test_of_social || !text.contains("createBff("),
            "{name} calls createBff"
        );
    }
}

#[test]
fn saas_and_api_kits_ship_their_openapi_document() {
    for kit in [Kit::Saas, Kit::Api] {
        let (_dir, root) = generate("acme-cloud", kit);
        let document: serde_json::Value =
            serde_json::from_str(&read(&root, "openapi.json")).unwrap();
        assert_eq!(document["info"]["title"], "Acme cloud API", "{kit}");
        for path in [
            "/api/auth/register",
            "/api/auth/login",
            "/api/workspaces",
            "/api/billing/checkout",
            "/api/v1/projects",
            "/api/v1/projects/{id}",
            "/api/v1/settings/workspace",
            "/api/v1/account/sessions",
        ] {
            assert!(document["paths"].get(path).is_some(), "{kit}: {path}");
        }
        // CI fails when it no longer matches the code.
        let ci = read(&root, ".github/workflows/ci.yml");
        assert!(ci.contains("openapi:export openapi.json"));
        assert!(ci.contains("git diff --exit-code -- openapi.json"));
    }
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
    let harbor =
        navree_harbor::HarborConfig::parse(&read(&api, "harbor.toml"), Path::new("harbor.toml"))
            .unwrap();
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
            let features = service["dependencies"]["navree"]["features"]
                .as_array()
                .unwrap();
            for feature in ["full", "micro", "micro-nats", "postgres"] {
                assert!(
                    features.iter().any(|f| f.as_str() == Some(feature)),
                    "{feature}"
                );
            }
        } else {
            let features = dependency["features"].as_array().unwrap();
            for feature in ["billing", "social", "postgres"] {
                assert!(
                    features.iter().any(|f| f.as_str() == Some(feature)),
                    "{feature}"
                );
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
    assert!(
        files
            .iter()
            .all(|file| !file.starts_with("web/") && !file.starts_with("src/"))
    );

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
    assert!(
        read(&root, "crates/billing/src/modules/invoices/mod.rs")
            .contains("m.migrations(migrations::migrations())")
    );

    // NATS is the transport, from Dock in development.
    let config = toml(&root, "config/app.toml");
    assert_eq!(config["micro"]["transport"].as_str(), Some("nats"));
    assert_eq!(config["micro"]["service"].as_str(), Some("billing"));
    let dock = navree_dock::DockFile::parse(&read(&root, "dock.toml"), "billing").unwrap();
    assert!(dock.service("postgres").is_ok() && dock.service("nats").is_ok());
    let env = read(&root, ".env");
    assert!(
        env.contains("APP_MICRO__URL=nats://127.0.0.1:4222"),
        "{env}"
    );
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
    assert!(
        env.contains("APP_DATABASE__URL=postgres://navree:navree@127.0.0.1:5432/"),
        "{env}"
    );
    assert!(
        env.contains("APP_MAIL__TRANSPORT=smtp://127.0.0.1:1025"),
        "{env}"
    );
    assert!(!example.contains("# dock:start"));
    // Secrets are placeholders, never values.
    assert!(env.contains("APP_KEY=\n"));
}

#[test]
fn every_kit_s_harbor_and_dock_files_are_valid() {
    for kit in Kit::ALL {
        let (_dir, root) = generate("acme", kit);
        let harbor = navree_harbor::HarborConfig::parse(
            &read(&root, "harbor.toml"),
            &root.join("harbor.toml"),
        )
        .unwrap_or_else(|e| panic!("{kit}: {e}"));
        assert_eq!(harbor.app, "acme");
        assert_eq!(harbor.validate(), Vec::<String>::new(), "{kit}");
        assert_eq!(harbor.deploy.health_check, "/health/ready");
        navree_dock::DockFile::parse(&read(&root, "dock.toml"), "acme")
            .unwrap_or_else(|e| panic!("{kit}: {e}"));
    }
}

#[test]
fn generated_files_are_free_of_template_syntax() {
    for kit in Kit::ALL {
        let (_dir, root) = generate("acme-cloud", kit);
        for file in tree(&root) {
            let text = read(&root, &file);
            for marker in [
                "{%",
                "%}",
                "{{ package",
                "{{ crate_name",
                "{{ title",
                "{{ kit",
                "__package__",
                "mNOW",
            ] {
                assert!(!text.contains(marker), "{kit}: {file} contains {marker}");
            }
            assert!(
                !file.contains("__package__") && !file.ends_with(".j2"),
                "{file}"
            );
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
        "  navree dock up                 # TimescaleDB and Mailpit\n",
        "  navree key:generate            # then set APP_KEY in .env\n",
        "  navree migrate:fresh --seed    # tables and a demo workspace\n",
        "  pnpm --dir /opt/navree/packages/js install # the Navree packages (not on npm yet)\n",
        "  pnpm --dir /opt/navree/packages/js build # once; the web app links them\n",
        "  pnpm --dir web install         # the web app's packages\n",
        "  navree dev                     # API on :8080, web app on :3000\n",
    ] {
        assert!(message.contains(line), "{line:?} in\n{message}");
    }
    // A published navree has published packages: only the web app's own install.
    let published = NewApp {
        navree: NavreeSource::Version("1.0.0".to_owned()),
        ..app("acme", Kit::Saas)
    };
    let message = created_message(&published, &root);
    assert!(message.contains("  pnpm --dir web install         # the web app's packages\n"));
    assert!(!message.contains("packages/js"), "{message}");
    let micro = created_message(&app("billing", Kit::Micro), &root);
    assert!(micro.contains("(kit: micro)"));
    assert!(micro.contains("# TimescaleDB and NATS"));
    let api = created_message(&app("acme", Kit::Api), &root);
    assert!(api.contains("(kit: api)"));
    assert!(!api.contains("pnpm"));
}

#[test]
fn ui_next_goes_with_the_saas_kit_only() {
    // The SaaS kit's front end is Next.js: saying so changes nothing.
    let (_dir, saas) = generate("acme", Kit::Saas);
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let explicit = create_app(
        dir.path(),
        &NewApp {
            ui: Ui::Next,
            ..app("acme", Kit::Saas)
        },
        now(),
    )
    .unwrap();
    assert_eq!(tree(&explicit), tree(&saas));
    // The other kits have no front end.
    for kit in [Kit::Api, Kit::Micro] {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let error = create_app(
            dir.path(),
            &NewApp {
                ui: Ui::Next,
                ..app("acme", kit)
            },
            now(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("has no front end"), "{error}");
        assert!(!dir.path().join("acme").exists());
    }
}

/// The kits run on TimescaleDB (Dock's default) or plain PostgreSQL; MySQL
/// and SQLite go with the minimal app.
#[test]
fn kits_run_on_timescaledb_or_postgres() {
    let (_dir, timescale) = generate("acme", Kit::Api);
    let dock = std::fs::read_to_string(timescale.join("dock.toml")).unwrap();
    assert!(dock.contains("[services.postgres]\n"), "{dock}");
    assert!(!dock.contains("timescaledb = false"), "{dock}");
    // CI runs the database Dock runs locally.
    let ci = read(&timescale, ".github/workflows/ci.yml");
    assert!(
        ci.contains("image: timescale/timescaledb:2.31.0-pg17\n"),
        "{ci}"
    );
    assert!(read(&timescale, "README.md").contains("# TimescaleDB and Mailpit"));

    for kit in [Kit::Saas, Kit::Api, Kit::Micro] {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let postgres = NewApp {
            database: Database::Postgres,
            ..app("acme", kit)
        };
        let root = create_app(dir.path(), &postgres, now()).unwrap();
        let dock = std::fs::read_to_string(root.join("dock.toml")).unwrap();
        assert!(
            dock.contains("[services.postgres]\ntimescaledb = false\n"),
            "{kit}: {dock}"
        );
        let file = navree_dock::DockFile::parse(&dock, "acme").unwrap();
        assert!(!file.env().is_empty(), "{kit}");
        let ci = read(&root, ".github/workflows/ci.yml");
        assert!(ci.contains("image: postgres:17\n"), "{kit}: {ci}");
        assert!(
            read(&root, "README.md").contains("# PostgreSQL and "),
            "{kit}"
        );
        let message = created_message(&postgres, &root);
        assert!(message.contains("# PostgreSQL and "), "{kit}: {message}");

        for database in [Database::Mysql, Database::Sqlite] {
            let dir = tempfile::Builder::new()
                .prefix("navree-test-")
                .tempdir()
                .unwrap();
            let error = create_app(
                dir.path(),
                &NewApp {
                    database,
                    ..app("acme", kit)
                },
                now(),
            )
            .unwrap_err();
            assert!(error.to_string().contains("runs on PostgreSQL"), "{error}");
            assert!(!dir.path().join("acme").exists());
        }
    }
}

#[test]
fn kits_refuse_a_directory_that_is_not_empty() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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
                .args([
                    "--edition",
                    "2024",
                    "--check",
                    "--config",
                    "skip_children=true",
                ])
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
    assert!(
        unformatted.is_empty(),
        "not rustfmt-clean:\n{}",
        unformatted.join("\n")
    );
}

#[test]
fn the_comment_on_the_admin_module_names_the_resources_it_registers() {
    for kit in [Kit::Api, Kit::Saas] {
        let (_dir, root) = generate("acme", kit);
        let admin = read(&root, "src/modules/admin/mod.rs");
        let wire = admin.split("fn wire").nth(1).expect("the wire function");
        let registered: Vec<&str> = wire
            .split("resource::<")
            .skip(1)
            .filter_map(|rest| rest.split('>').next())
            .collect();
        assert_eq!(
            registered,
            ["UserAdmin", "WorkspaceAdmin", "SubscriptionAdmin"],
            "{kit}"
        );
        let app = read(&root, "src/app.rs");
        let comment = app
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("// Helm resources for"))
            .unwrap_or_else(|| panic!("{kit}: no comment on the admin module in {app}"));
        assert_eq!(
            comment, "// Helm resources for users, workspaces and subscriptions.",
            "{kit}"
        );
    }
}
