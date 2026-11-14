//! ClientModule: a client crate's client as a module of the app.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use navree_config::{AppOptions, ClientModule, ConfigModule, ConfigSection, ConfigSources};
use navree_contracts::{HealthCheck, HealthStatus};
use navree_kernel::{Module, ModuleId, Navree, plugin_info};
use serde::Deserialize;
use serde_json::json;

/// The weather feed's options (section `weather`).
#[derive(Clone, Deserialize)]
struct WeatherOptions {
    feed: String,
}

impl ConfigSection for WeatherOptions {
    const KEY: &'static str = "weather";
}

/// The client a feed crate would give: where it reads, and whether it was closed.
struct Forecasts {
    feed: String,
    closed: AtomicBool,
}

fn config(feed: &str) -> impl Module {
    ConfigModule::for_root(ConfigSources::new().set("weather.feed", json!(feed)))
}

fn weather(name: &'static str) -> ClientModule<Forecasts, WeatherOptions> {
    ClientModule::<Forecasts>::new(name)
        .options::<WeatherOptions>()
        .connect(|options, _| async move {
            Ok(Forecasts {
                feed: options.feed.clone(),
                closed: AtomicBool::new(false),
            })
        })
        .health(|forecasts| async move {
            if forecasts.feed.starts_with("https://") {
                Ok(())
            } else {
                Err("the mausam feed is not https".into())
            }
        })
        .shutdown(|forecasts| async move {
            forecasts.closed.store(true, Ordering::SeqCst);
            Ok(())
        })
}

#[tokio::test]
async fn a_client_module_reads_its_section_connects_reports_health_and_shuts_down() {
    let app = Navree::app()
        .module(config("https://mausam.example/ludhiana"))
        .module(weather("weather"))
        .boot()
        .await
        .expect("boots");
    let forecasts = app.ctx().get::<Forecasts>().await.unwrap();
    assert_eq!(forecasts.feed, "https://mausam.example/ludhiana");
    let by_name = app.ctx().get_named::<Forecasts>("weather").await.unwrap();
    assert!(
        Arc::ptr_eq(&forecasts, &by_name),
        "one client, also under its name"
    );
    let checks = app.ctx().get_all::<dyn HealthCheck>().await.unwrap();
    assert_eq!(
        checks.iter().map(|check| check.name()).collect::<Vec<_>>(),
        ["weather"]
    );
    assert_eq!(checks[0].check().await, HealthStatus::Up);
    app.shutdown().await.unwrap();
    assert!(
        forecasts.closed.load(Ordering::SeqCst),
        "the shutdown hook ran"
    );

    // A failing readiness check is down, with its message.
    let app = Navree::app()
        .module(config("http://mausam.example/amritsar"))
        .module(weather("weather"))
        .boot()
        .await
        .unwrap();
    let checks = app.ctx().get_all::<dyn HealthCheck>().await.unwrap();
    assert_eq!(
        checks[0].check().await,
        HealthStatus::Down("the mausam feed is not https".into())
    );

    // Options in code: the section is not read.
    let app = Navree::app()
        .module(config("https://mausam.example/ludhiana"))
        .module(weather("weather").for_root(WeatherOptions {
            feed: "https://mausam.example/jaipur".into(),
        }))
        .boot()
        .await
        .unwrap();
    assert_eq!(
        app.ctx().get::<Forecasts>().await.unwrap().feed,
        "https://mausam.example/jaipur"
    );

    // No connect function, also when `options` came after it: the boot fails naming the module.
    let missing = ClientModule::<Forecasts>::new("weather").options::<WeatherOptions>();
    let late = ClientModule::<Forecasts>::new("weather")
        .connect(|_, _| async {
            Ok(Forecasts {
                feed: String::new(),
                closed: AtomicBool::new(false),
            })
        })
        .options::<WeatherOptions>();
    for module in [missing, late] {
        let error = Navree::app()
            .module(config("https://mausam.example/ludhiana"))
            .module(module)
            .boot()
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("ClientModule `weather` has no connect function: call .connect(..)"),
            "{error}"
        );
    }

    // A connect function that fails stops the boot with its error.
    let refusing = ClientModule::<Forecasts>::new("weather")
        .options::<WeatherOptions>()
        .connect(|_, _| async { Err("the mausam feed refused the key".into()) });
    let error = Navree::app()
        .module(config("https://mausam.example/ludhiana"))
        .module(refusing)
        .boot()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("the mausam feed refused the key"), "{error}");

    // Without options: no section, and `connect` gets `()`.
    let bare = ClientModule::<Forecasts>::new("bare").connect(|_unit, _| async {
        Ok(Forecasts {
            feed: "https://mausam.example/patiala".into(),
            closed: AtomicBool::new(false),
        })
    });
    let app = Navree::app().module(bare).boot().await.unwrap();
    assert_eq!(
        app.ctx().get_named::<Forecasts>("bare").await.unwrap().feed,
        "https://mausam.example/patiala"
    );
}

#[tokio::test]
async fn two_client_modules_of_one_type_are_two_modules() {
    let report = Navree::app()
        .module(config("https://mausam.example/ludhiana"))
        .module(weather("delhi"))
        .module(weather("ludhiana"))
        .validate()
        .expect("two modules");
    let names: Vec<&str> = report.modules.iter().map(|module| module.name).collect();
    assert!(
        names.contains(&"delhi") && names.contains(&"ludhiana"),
        "{names:?}"
    );
    assert_ne!(
        ModuleId::named::<ClientModule<Forecasts, WeatherOptions>>("delhi"),
        ModuleId::named::<ClientModule<Forecasts, WeatherOptions>>("ludhiana")
    );
    let app = Navree::app()
        .module(config("https://mausam.example/ludhiana"))
        .module(weather("delhi"))
        .module(weather("ludhiana"))
        .boot()
        .await
        .unwrap();
    let delhi = app.ctx().get_named::<Forecasts>("delhi").await.unwrap();
    let ludhiana = app.ctx().get_named::<Forecasts>("ludhiana").await.unwrap();
    assert!(
        !Arc::ptr_eq(&delhi, &ludhiana),
        "each module builds its own client"
    );
    let error = app
        .ctx()
        .get::<Forecasts>()
        .await
        .err()
        .expect("two unnamed exports")
        .to_string();
    assert!(
        error.contains("delhi")
            && error.contains("ludhiana")
            && error.contains("use named providers"),
        "{error}"
    );

    // One name twice is one module.
    let report = Navree::app()
        .module(config("https://mausam.example/ludhiana"))
        .module(weather("delhi"))
        .module(weather("delhi"))
        .validate()
        .unwrap();
    assert_eq!(
        report
            .modules
            .iter()
            .filter(|module| module.name == "delhi")
            .count(),
        1
    );
}

#[tokio::test]
async fn a_client_module_of_another_crates_client_belongs_to_the_plugin_of_its_options_type() {
    // `String` stands for another crate's client; the options type is this crate's.
    let feed = ClientModule::<String>::new("weather")
        .options::<WeatherOptions>()
        .connect(|options, _| async move { Ok(options.feed.clone()) })
        .plugin(plugin_info!(namespace = "weather"));
    let report = Navree::app()
        .module(config("https://mausam.example/ludhiana"))
        .module(feed)
        .plugin_report()
        .expect("the options type ties the module to this crate");
    assert_eq!(report.plugins.len(), 1);
    let plugin = &report.plugins[0];
    assert_eq!(
        (
            plugin.info.name,
            plugin.info.crate_name,
            plugin.info.namespace
        ),
        ("navree-config", "client", "weather")
    );
    assert!(!plugin.first_party);
    assert_eq!(
        plugin.modules[0].id,
        ModuleId::named::<ClientModule<String, WeatherOptions>>("weather")
    );
    assert_eq!(
        plugin.modules[0].id.type_name(),
        "navree_config::client::ClientModule<alloc::string::String, client::WeatherOptions>"
    );

    // Another crate's client with another crate's options type names no path of this crate.
    let foreign = ClientModule::<String>::new("app")
        .options::<AppOptions>()
        .connect(|_, _| async { Ok(String::new()) })
        .plugin(plugin_info!(namespace = "weather"));
    let error = Navree::app()
        .module(config("https://mausam.example/ludhiana"))
        .module(foreign)
        .validate()
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "module `app` names the plugin `navree-config`, which is another crate's; a plugin's modules are in its own crate (a ClientModule names it through an options type of the plugin's crate)"
    );
}
