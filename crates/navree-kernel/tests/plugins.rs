//! Plugins: what a module says about itself, the boot checks, the report and the
//! `plugins` built-in. This file's crate is `plugins`, in the package `navree-kernel`.

use navree_kernel::{BootErrors, KernelError, Module, ModuleBuilder, ModuleId, Navree};
use navree_kernel::{PluginInfo, plugin_info};

/// The text of the errors `app` fails to validate with.
fn errors(app: navree_kernel::AppBuilder) -> String {
    app.validate().expect_err("must fail").to_string()
}

/// A module that is no plugin.
struct Plain;
impl Module for Plain {
    fn register(&self, _: &mut ModuleBuilder) {}
}

/// A module type whose path can name another crate in a generic argument.
struct Holder<T>(std::marker::PhantomData<T>);

#[test]
fn plugin_info_fills_every_field_from_the_calling_crate() {
    let info = plugin_info!();
    assert_eq!(info.name, "navree-kernel");
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.namespace, "kernel");
    assert_eq!(info.title, None);
    assert_eq!(info.description, Some(env!("CARGO_PKG_DESCRIPTION")));
    assert_eq!(info.homepage, Some("https://navree.com"));
    assert_eq!(info.repository, Some("https://github.com/NavreeHQ/navree"));
    assert_eq!(info.license, Some("MIT OR Apache-2.0"));
}

#[test]
fn plugin_info_records_the_rust_crate_name_even_when_lib_name_differs() {
    // The package is `navree-kernel`; the crate this test is compiled in is `plugins`.
    let info = plugin_info!();
    assert_eq!(info.crate_name, "plugins");
    assert_eq!(info.name, "navree-kernel");
}

#[test]
fn plugin_info_takes_a_namespace_and_a_title() {
    let both = plugin_info!(namespace = "notice_board", title = "Notice board");
    assert_eq!(
        (both.namespace, both.title),
        ("notice_board", Some("Notice board"))
    );
    assert_eq!(
        plugin_info!(title = "Notice board", namespace = "notice_board",),
        both
    );
    assert_eq!(plugin_info!(title = "Notices").namespace, "kernel");
    assert_eq!(plugin_info!(namespace = "notices").title, None);
    assert_eq!(plugin_info!(title = "").title, None);
    const AT_COMPILE_TIME: PluginInfo = plugin_info!(namespace = "notices");
    assert_eq!(AT_COMPILE_TIME.namespace, "notices");

    let by_hand = PluginInfo::new(
        "navree-plugin-announcements",
        "navree_plugin_announcements",
        "announcements",
        "1.0.0",
    )
    .title("Announcements")
    .description("")
    .repository("https://code.ludhiana.example/navree-plugin-announcements");
    assert_eq!(by_hand.description, None);
    assert_eq!(by_hand.homepage, None);
    assert_eq!(
        by_hand.repository,
        Some("https://code.ludhiana.example/navree-plugin-announcements")
    );
}

/// A plugin of two modules of this crate, one namespace.
struct NoticesModule;
impl Module for NoticesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(plugin_info!(namespace = "announcements"));
    }
}
struct NoticeAdminModule;
impl Module for NoticeAdminModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(plugin_info!(namespace = "announcements"));
    }
}

#[tokio::test]
async fn m_plugin_records_the_plugin_for_its_module() {
    let app = Navree::app()
        .module(NoticesModule)
        .module(NoticeAdminModule)
        .module(Plain);
    app.validate().expect("one plugin, two modules");
    let app = app.boot().await.expect("boots");
    let info = plugin_info!(namespace = "announcements");
    assert_eq!(
        app.ctx().plugin(ModuleId::of::<NoticesModule>()),
        Some(&info)
    );
    assert_eq!(
        app.ctx().plugin(ModuleId::of::<NoticeAdminModule>()),
        Some(&info)
    );
}

#[tokio::test]
async fn the_context_answers_which_plugin_a_module_is() {
    let app = Navree::app()
        .module(NoticesModule)
        .module(Plain)
        .boot()
        .await
        .expect("boots");
    assert!(app.ctx().plugin(ModuleId::of::<NoticesModule>()).is_some());
    assert_eq!(
        app.ctx().plugin(ModuleId::of::<Plain>()),
        None,
        "not a plugin"
    );
    assert_eq!(
        app.ctx().plugin(ModuleId::of::<NoticeAdminModule>()),
        None,
        "not in the app"
    );
}

/// `m.plugin(info)` with a hand-built `info`.
struct Claims(&'static str, PluginInfo);
impl Module for Claims {
    fn id(&self) -> ModuleId {
        ModuleId::named::<Claims>(self.0)
    }
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(self.1.clone());
    }
}
fn weather(crate_name: &'static str, namespace: &'static str, version: &'static str) -> PluginInfo {
    PluginInfo::new("navree-plugin-weather", crate_name, namespace, version)
}

#[test]
fn calling_m_plugin_twice_fails_the_boot() {
    struct Twice;
    impl Module for Twice {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(plugin_info!(namespace = "announcements"))
                .plugin(plugin_info!(namespace = "notices"));
        }
    }
    assert_eq!(
        errors(Navree::app().module(Twice)),
        "module `Twice` calls m.plugin(..) twice; call it once in its register"
    );
}

#[test]
fn a_bad_namespace_fails_the_boot() {
    assert_eq!(
        errors(Navree::app().module(Claims(
            "WeatherModule",
            weather("plugins", "Weather-Reports", "1.0.0")
        ))),
        "plugin `navree-plugin-weather` in module `WeatherModule` has the namespace `Weather-Reports`, which is not a lower-case letter followed by up to 63 lower-case letters, digits or _; set one with plugin_info!(namespace = \"..\")"
    );
}

#[test]
fn a_module_naming_another_crates_plugin_fails_the_boot() {
    assert_eq!(
        errors(Navree::app().module(Claims(
            "WeatherModule",
            weather("navree_plugin_weather", "weather", "1.0.0")
        ))),
        "module `WeatherModule` names the plugin `navree-plugin-weather`, which is another crate's; a plugin's modules are in its own crate (a ClientModule names it through an options type of the plugin's crate)"
    );
}

/// A module whose type names `alloc` only in a generic argument.
struct Shelf;
impl Module for Shelf {
    fn id(&self) -> ModuleId {
        ModuleId::of::<Holder<String>>()
    }
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(PluginInfo::new("navree-plugin-weather", "alloc", "weather", "1.0.0"));
    }
}

#[test]
fn two_crates_claiming_one_plugin_name_fail_the_boot() {
    // A generic argument naming the crate is enough for membership.
    Navree::app()
        .module(Shelf)
        .validate()
        .expect("Holder<alloc::string::String> names alloc");
    let errors: BootErrors = Navree::app()
        .module(Claims("WeatherModule", weather("plugins", "weather", "1.0.0")))
        .module(Shelf)
        .validate()
        .expect_err("two crates, one name");
    assert!(
        matches!(
            errors.0.as_slice(),
            [KernelError::PluginOfAnotherCrate { module, plugin: "navree-plugin-weather" }]
                if module.starts_with("Holder<")
        ),
        "{errors:?}"
    );
}

#[test]
fn a_plugin_in_a_reserved_namespace_fails_the_boot() {
    for namespace in ["billing", "views", "plugins", "all", "users", "operations"] {
        let ledger = PluginInfo::new("navree-plugin-ledger", "plugins", namespace, "1.0.0");
        assert_eq!(
            errors(Navree::app().module(Claims("LedgerModule", ledger))),
            format!(
                "plugin `navree-plugin-ledger` uses the namespace `{namespace}`, which the framework keeps for itself; set another with plugin_info!(namespace = \"..\")"
            )
        );
    }
}

#[test]
fn a_first_party_name_in_another_crate_is_a_community_plugin() {
    // On the kernel's list and at its version, but a module of this crate: no exemption.
    let flags = PluginInfo::new(
        "navree-flags",
        "plugins",
        "flags",
        env!("CARGO_PKG_VERSION"),
    );
    assert_eq!(
        errors(Navree::app().module(Claims("FlagsModule", flags))),
        "plugin `navree-flags` uses the namespace `flags`, which the framework keeps for itself; set another with plugin_info!(namespace = \"..\")"
    );
}

#[test]
fn one_plugin_at_two_versions_fails_the_boot() {
    let app = Navree::app()
        .module(Claims("WeatherV1", weather("plugins", "weather", "1.0.0")))
        .module(Claims("WeatherV2", weather("plugins", "weather", "1.1.0")));
    assert_eq!(
        errors(app),
        "plugin `navree-plugin-weather` is in the app at two versions (1.0.0 in `WeatherV1`, 1.1.0 in `WeatherV2`); depend on one version"
    );
}

#[test]
fn one_plugin_described_two_ways_fails_the_boot() {
    let described = "plugin `navree-plugin-weather` is described differently by `WeatherModule` and `WeatherAdmin`; give every module of a plugin the same plugin_info!(..)";
    // Another title.
    let app = Navree::app()
        .module(Claims("WeatherModule", weather("plugins", "weather", "1.0.0")))
        .module(Claims(
            "WeatherAdmin",
            weather("plugins", "weather", "1.0.0").title("Weather"),
        ));
    assert_eq!(errors(app), described);
    // Another namespace, a reserved one: the second module cannot slip past NamespaceReserved.
    let app = Navree::app()
        .module(Claims("WeatherModule", weather("plugins", "weather", "1.0.0")))
        .module(Claims("WeatherAdmin", weather("plugins", "workspace", "1.0.0")));
    assert_eq!(errors(app), described);
}

#[test]
fn two_plugins_with_one_namespace_fail_the_boot() {
    let forecast = PluginInfo::new("navree-plugin-forecast", "plugins", "weather", "1.0.0");
    let app = Navree::app()
        .module(Claims("WeatherModule", weather("plugins", "weather", "1.0.0")))
        .module(Claims("ForecastModule", forecast));
    assert_eq!(
        errors(app),
        "plugins `navree-plugin-weather` and `navree-plugin-forecast` both use the namespace `weather`; set another with plugin_info!(namespace = \"..\") in one of them"
    );
}
