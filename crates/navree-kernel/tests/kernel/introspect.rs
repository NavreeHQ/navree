//! `introspect --json`: the module graph as JSON, for tools such as
//! Navigator (`navree mcp`), without constructing any provider.

use std::sync::Arc;

use navree_kernel::{BoxError, CommandSpec, Factory, Module, ModuleBuilder, Navree};
use serde_json::{Value, json};

use crate::support::Counter;

pub struct Store;
pub struct Service;

/// Exports `Service` (built from `Store`) and registers one command.
struct DealsModule {
    built: Arc<Counter>,
}

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let built = self.built.clone();
        m.value(Store)
            .factory(
                Factory::<Service>::new(move |_| {
                    built.hit();
                    async { Ok::<_, BoxError>(Arc::new(Service)) }
                })
                .deps(vec![navree_kernel::ProviderKey::of::<Store>()]),
            )
            .exports::<Service>()
            .command(CommandSpec::new(
                "deals:close",
                "Close stale deals",
                |_, _| async { Ok(()) },
            ));
    }
}

fn graph() -> (Value, Arc<Counter>) {
    let built = Arc::new(Counter::default());
    let app = Navree::app().module(DealsModule {
        built: built.clone(),
    });
    let report = app.validate().expect("valid graph");
    let json = serde_json::from_str(&report.to_json()).expect("valid JSON");
    (json, built)
}

#[test]
fn the_report_lists_modules_providers_and_commands() {
    let (json, built) = graph();
    assert_eq!(json["navree"], env!("CARGO_PKG_VERSION"));
    let modules = json["modules"].as_array().expect("modules");
    let deals = modules
        .iter()
        .find(|m| m["name"] == "DealsModule")
        .expect("DealsModule");
    assert_eq!(deals["global"], false);
    assert_eq!(deals["root"], false);
    assert_eq!(deals["type"], std::any::type_name::<DealsModule>());
    let service = deals["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["type"].as_str().unwrap().ends_with("::Service"))
        .expect("Service");
    assert_eq!(service["scope"], "singleton");
    assert_eq!(service["exported"], true);
    assert_eq!(service["multi"], false);
    assert!(
        service["dependencies"][0]
            .as_str()
            .unwrap()
            .ends_with("::Store"),
        "{service}"
    );
    let store = deals["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["type"].as_str().unwrap().ends_with("::Store"))
        .expect("Store");
    assert_eq!(store["exported"], false);
    assert_eq!(
        deals["commands"],
        json!([{"name": "deals:close", "about": "Close stale deals"}])
    );

    // The implicit root comes last and imports the root modules.
    let root = modules.last().unwrap();
    assert_eq!(root["root"], true);
    assert_eq!(root["imports"], json!(["DealsModule"]));
    assert_eq!(built.get(), 0, "introspection constructs nothing");
}

#[test]
fn the_command_list_starts_with_the_built_ins() {
    let (json, _) = graph();
    let names: Vec<&str> = json["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "check",
            "help",
            "introspect",
            "plugins",
            "plugin:publish",
            "deals:close",
        ]
    );
    let close = &json["commands"][5];
    assert_eq!(close["module"], "DealsModule");
    assert_eq!(json["commands"][0]["module"], Value::Null);
}

#[test]
fn strings_are_escaped() {
    struct Quoted;
    impl Module for Quoted {
        fn register(&self, m: &mut ModuleBuilder) {
            m.command(CommandSpec::new(
                "say",
                "Prints \"hi\"\\ and a\ttab\nthen a line \u{1}",
                |_, _| async { Ok(()) },
            ));
        }
    }
    let report = Navree::app().module(Quoted).validate().unwrap();
    let json: Value = serde_json::from_str(&report.to_json()).expect("valid JSON");
    assert_eq!(
        json["commands"][5]["about"],
        "Prints \"hi\"\\ and a\ttab\nthen a line \u{1}"
    );
}

#[test]
fn a_built_in_that_yields_is_left_out_of_the_command_list() {
    struct OwnPlugins;
    impl Module for OwnPlugins {
        fn register(&self, m: &mut ModuleBuilder) {
            m.command(CommandSpec::new(
                "plugins",
                "The app's own list",
                |_, _| async { Ok(()) },
            ));
        }
    }
    let report = Navree::app().module(OwnPlugins).validate().unwrap();
    let json: Value = serde_json::from_str(&report.to_json()).expect("valid JSON");
    let names: Vec<&str> = json["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["check", "help", "introspect", "plugin:publish", "plugins"]
    );
    assert_eq!(json["commands"][4]["module"], "OwnPlugins");
}

#[tokio::test]
async fn introspect_runs_without_booting() {
    let built = Arc::new(Counter::default());
    Navree::app()
        .module(DealsModule {
            built: built.clone(),
        })
        .run_with_args(vec!["introspect".to_owned(), "--json".to_owned()])
        .await
        .expect("introspect");
    assert_eq!(built.get(), 0);
}
