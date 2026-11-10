//! The tools that ask the app binary: app_info, list_routes, module_graph,
//! list_providers and list_commands.

use navree_navigator::NavigatorOptions;
use navree_navigator::app::ProcessOutput;
use navree_navigator::docs::EMBEDDED_VERSION;
use serde_json::{Value, json};

use crate::support::{Fixture, call, refused};

fn graph() -> Value {
    json!({
        "navree": EMBEDDED_VERSION,
        "modules": [
            {"name": "DatabaseModule", "global": true, "root": false, "imports": [], "exports": ["navree_db::Db"],
             "providers": [{"type": "navree_db::Db", "scope": "singleton", "exported": true, "multi": false, "dependencies": []}],
             "commands": [{"name": "migrate", "about": "Run every pending migration"}]},
            {"name": "DealsModule", "global": false, "root": false, "imports": ["DatabaseModule"], "exports": ["crm::DealService"],
             "providers": [
                {"type": "crm::DealService", "scope": "singleton", "exported": true, "multi": false, "dependencies": ["navree_db::Db"]},
                {"type": "crm::DealsController", "scope": "singleton", "exported": false, "multi": false, "dependencies": ["crm::DealService"]}
             ],
             "commands": [{"name": "crm:task-reminders", "about": "Send task reminders"}]},
            {"name": "App", "global": false, "root": true, "imports": ["DatabaseModule", "DealsModule"], "exports": [], "providers": [], "commands": []}
        ],
        "commands": [
            {"name": "check", "about": "Validate the module graph", "module": null},
            {"name": "migrate", "about": "Run every pending migration", "module": "DatabaseModule"},
            {"name": "crm:task-reminders", "about": "Send task reminders", "module": "DealsModule"}
        ]
    })
}

fn routes() -> Value {
    json!([
        {"method": "GET", "path": "/api/v1/deals", "access": "can:read Deal", "module": "DealsModule", "handler": "DealsController::index"},
        {"method": "POST", "path": "/api/v1/deals", "access": "can:create Deal", "module": "DealsModule", "handler": "DealsController::store"},
        {"method": "GET", "path": "/api/v1/companies", "access": "auth", "module": "CompaniesModule", "handler": "CompaniesController::index"},
        {"method": "GET", "path": "/health", "access": "public", "module": "ObserveModule", "handler": null}
    ])
}

fn fixture() -> Fixture {
    let fixture =
        Fixture::new("[database]\nurl = \"postgres://navree:secret@127.0.0.1:5432/crm\"\n");
    fixture.built();
    fixture.answers(&["introspect", "--json"], graph());
    fixture.answers(&["routes", "--json"], routes());
    fixture
}

#[tokio::test]
async fn app_info_describes_the_project() {
    let fixture = fixture();
    let info = call(&fixture.navigator(), "app_info", json!({})).await;
    assert_eq!(info["package"], "crm");
    assert_eq!(info["navree"], EMBEDDED_VERSION);
    assert_eq!(info["rust"], "rustc 1.95.0 (abc 2026-09-01)");
    assert_eq!(info["features"], json!(["default", "postgres"]));
    assert_eq!(info["modules"], json!(["DatabaseModule", "DealsModule"]));
    assert_eq!(
        info["database"],
        json!({"driver": "postgres", "url": "postgres://navree:***@127.0.0.1:5432/crm"})
    );
    assert_eq!(info["config_files"], json!(["config/app.toml"]));
}

#[tokio::test]
async fn routes_are_filtered_by_path_method_and_module() {
    let fixture = fixture();
    let navigator = fixture.navigator();
    let all = call(&navigator, "list_routes", json!({})).await;
    assert_eq!(all.as_array().unwrap().len(), 4);
    let deals = call(&navigator, "list_routes", json!({"path": "/api/v1/deals"})).await;
    assert_eq!(deals.as_array().unwrap().len(), 2);
    assert_eq!(deals[0]["handler"], "DealsController::index");
    let posts = call(&navigator, "list_routes", json!({"method": "post"})).await;
    assert_eq!(posts.as_array().unwrap().len(), 1);
    let companies = call(
        &navigator,
        "list_routes",
        json!({"module": "CompaniesModule"}),
    )
    .await;
    assert_eq!(companies[0]["path"], "/api/v1/companies");
    let message = refused(&navigator, "list_routes", json!({"verb": "GET"})).await;
    assert!(message.contains("invalid arguments"), "{message}");
}

#[tokio::test]
async fn the_module_graph_whole_or_one_module() {
    let fixture = fixture();
    let navigator = fixture.navigator();
    let graph = call(&navigator, "module_graph", json!({})).await;
    assert_eq!(graph["modules"].as_array().unwrap().len(), 3);
    let deals = call(&navigator, "module_graph", json!({"module": "DealsModule"})).await;
    assert_eq!(deals["imports"], json!(["DatabaseModule"]));
    assert_eq!(deals["exports"], json!(["crm::DealService"]));
    let message = refused(&navigator, "module_graph", json!({"module": "Nope"})).await;
    assert!(message.contains("DatabaseModule, DealsModule"), "{message}");
}

#[tokio::test]
async fn providers_carry_their_module() {
    let fixture = fixture();
    let providers = call(&fixture.navigator(), "list_providers", json!({})).await;
    let service = providers
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["type"] == "crm::DealService")
        .unwrap();
    assert_eq!(service["module"], "DealsModule");
    assert_eq!(service["exported"], true);
    assert_eq!(service["dependencies"], json!(["navree_db::Db"]));
    let only = call(
        &fixture.navigator(),
        "list_providers",
        json!({"module": "DatabaseModule"}),
    )
    .await;
    assert_eq!(only.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn commands_come_from_the_graph() {
    let fixture = fixture();
    let commands = call(&fixture.navigator(), "list_commands", json!({})).await;
    let names: Vec<&str> = commands
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["check", "migrate", "crm:task-reminders"]);
}

#[tokio::test]
async fn a_build_error_reaches_the_app_tools_and_the_others_keep_working() {
    let fixture = Fixture::new("[app]\nname = \"crm\"\n");
    fixture.runner.on(
        "cargo",
        &["metadata"],
        crate::support::ok(
            &json!({"packages": [{"name": "crm", "targets": [{"name": "crm", "kind": ["bin"]}]}], "target_directory": fixture.root().join("target")})
                .to_string(),
        ),
    );
    let error = json!({"reason": "compiler-message", "message": {"level": "error", "rendered": "error[E0425]: cannot find value `deal`"}});
    fixture.runner.on(
        "cargo",
        &["build"],
        ProcessOutput {
            success: false,
            stdout: format!(
                "{error}\n{}",
                json!({"reason": "build-finished", "success": false})
            ),
            stderr: String::new(),
        },
    );
    let navigator = fixture.navigator();
    for tool in [
        "list_routes",
        "module_graph",
        "list_providers",
        "list_commands",
    ] {
        let message = refused(&navigator, tool, json!({})).await;
        assert!(
            message.contains("cannot find value `deal`"),
            "{tool}: {message}"
        );
    }
    let info = call(&navigator, "app_info", json!({})).await;
    assert!(
        info["modules"]["error"]
            .as_str()
            .unwrap()
            .contains("doesn't build")
    );
    let config = call(&navigator, "read_config", json!({})).await;
    assert_eq!(config["app"]["name"], "crm");
}

/// `read_config` asks the app's `config:show`, which knows the app's sections:
/// a credential with a name that does not look like one is masked too.
#[tokio::test]
async fn read_config_answers_with_the_apps_config_show() {
    let fixture = fixture();
    fixture.answers(
        &["config:show", "--defaults"],
        json!({"app": {"name": "crm"}, "billing": {"razorpay": {"webhook": "[redacted]"}}, "queue": {"workers": 2}}),
    );
    fixture.answers(
        &["config:show", "--json"],
        json!({"app": {"name": "crm"}, "billing": {"razorpay": {"webhook": "[redacted]"}}}),
    );
    let navigator = fixture.navigator();

    let config = call(&navigator, "read_config", json!({})).await;
    assert_eq!(config["billing"]["razorpay"]["webhook"], "[redacted]");
    assert!(config.get("queue").is_none(), "no defaults unless asked");
    let webhook = call(
        &navigator,
        "read_config",
        json!({"key": "billing.razorpay.webhook"}),
    )
    .await;
    assert_eq!(webhook, "[redacted]");
    let workers = call(
        &navigator,
        "read_config",
        json!({"key": "queue.workers", "defaults": true}),
    )
    .await;
    assert_eq!(workers, 2);
    let message = refused(&navigator, "read_config", json!({"key": "nope"})).await;
    assert!(
        message.contains("no `nope`") && message.contains("billing"),
        "{message}"
    );

    let shows: Vec<_> = fixture
        .runner
        .calls()
        .into_iter()
        .filter(|made| made.args.first().map(String::as_str) == Some("config:show"))
        .collect();
    // Asked every time (the files change without a build), and without the
    // probe's own settings, which it would show as the app's.
    assert_eq!(shows.len(), 4);
    assert_eq!(shows[0].args, ["config:show", "--json"]);
    assert_eq!(shows[2].args, ["config:show", "--defaults", "--json"]);
    for show in &shows {
        assert!(
            show.env.iter().all(
                |(name, _)| name != "APP_LOG__LEVEL" && name != "APP_DATABASE__MIGRATE_ON_BOOT"
            ),
            "{:?}",
            show.env
        );
    }
}

/// A Navree without `config:show`, or one that fails: the files answer.
#[tokio::test]
async fn read_config_reads_the_files_when_the_app_cannot_answer() {
    let fixture = fixture();
    fixture.runner.on(
        "crm",
        &["config:show"],
        ProcessOutput {
            success: false,
            stdout: String::new(),
            stderr: "unknown command `config:show`".to_owned(),
        },
    );
    let config = call(&fixture.navigator(), "read_config", json!({})).await;
    assert_eq!(
        config["database"]["url"],
        "postgres://navree:[redacted]@127.0.0.1:5432/crm"
    );
}

#[tokio::test]
async fn no_build_never_runs_cargo_build() {
    let fixture = Fixture::new("");
    fixture.runner.on(
        "cargo",
        &["metadata"],
        crate::support::ok(
            &json!({"packages": [{"name": "crm", "targets": [{"name": "crm", "kind": ["bin"]}]}], "target_directory": fixture.root().join("target")})
                .to_string(),
        ),
    );
    let navigator = fixture.navigator_with(NavigatorOptions {
        path: fixture.root().to_path_buf(),
        no_build: true,
        docs_offline: true,
        ..NavigatorOptions::default()
    });
    let message = refused(&navigator, "list_routes", json!({})).await;
    assert!(message.contains("--no-build"), "{message}");
    assert!(
        fixture
            .runner
            .calls()
            .iter()
            .all(|c| c.args.first().map(String::as_str) != Some("build"))
    );
}

#[tokio::test]
async fn outside_an_app_every_tool_says_where_to_start() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let fixture = Fixture::new("");
    let navigator = fixture.navigator_with(NavigatorOptions {
        path: dir.path().to_path_buf(),
        docs_offline: true,
        ..NavigatorOptions::default()
    });
    for tool in ["app_info", "list_routes", "read_config", "read_logs"] {
        let message = refused(&navigator, tool, json!({})).await;
        assert!(message.contains("--path"), "{tool}: {message}");
    }
    // The docs still answer.
    let hits = call(&navigator, "search_docs", json!({"query": "navigator"})).await;
    assert!(!hits["results"].as_array().unwrap().is_empty());
}
