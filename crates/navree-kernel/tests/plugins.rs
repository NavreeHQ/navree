//! Plugins: what a module says about itself, the boot checks, the report and the
//! `plugins` built-in. This file's crate is `plugins`, in the package `navree-kernel`.

use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use navree_kernel::{BootErrors, KernelError, Module, ModuleBuilder, ModuleId, Navree};
use navree_kernel::{BoxError, CommandSpec, ConfiguredModule, Factory};
use navree_kernel::{PluginInfo, plugin_info};
use navree_kernel::{PublishTag, Publishable};

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
        m.plugin(PluginInfo::new(
            "navree-plugin-weather",
            "alloc",
            "weather",
            "1.0.0",
        ));
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
        .module(Claims(
            "WeatherModule",
            weather("plugins", "weather", "1.0.0"),
        ))
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
fn a_plugin_named_after_a_folder_the_framework_reads_or_a_device_fails_the_boot() {
    // `layouts` would put a plugin's templates over the framework's mail and document
    // layouts, `fonts` its files into the documents' font folder, and a device name can be
    // no folder at all on Windows.
    for namespace in [
        "layouts", "fonts", "con", "nul", "aux", "prn", "com1", "lpt9",
    ] {
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
        .module(Claims(
            "WeatherModule",
            weather("plugins", "weather", "1.0.0"),
        ))
        .module(Claims(
            "WeatherAdmin",
            weather("plugins", "weather", "1.0.0").title("Weather"),
        ));
    assert_eq!(errors(app), described);
    // Another namespace, a reserved one: the second module cannot slip past NamespaceReserved.
    let app = Navree::app()
        .module(Claims(
            "WeatherModule",
            weather("plugins", "weather", "1.0.0"),
        ))
        .module(Claims(
            "WeatherAdmin",
            weather("plugins", "workspace", "1.0.0"),
        ));
    assert_eq!(errors(app), described);
}

#[test]
fn two_plugins_with_one_namespace_fail_the_boot() {
    let forecast = PluginInfo::new("navree-plugin-forecast", "plugins", "weather", "1.0.0");
    let app = Navree::app()
        .module(Claims(
            "WeatherModule",
            weather("plugins", "weather", "1.0.0"),
        ))
        .module(Claims("ForecastModule", forecast));
    assert_eq!(
        errors(app),
        "plugins `navree-plugin-weather` and `navree-plugin-forecast` both use the namespace `weather`; set another with plugin_info!(namespace = \"..\") in one of them"
    );
}

struct LooseFiles;
impl Module for LooseFiles {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(Publishable::new(PublishTag::Files).file("banner.css", b"body{}"));
    }
}

#[test]
fn files_to_publish_from_a_module_that_is_no_plugin_fail_the_boot() {
    assert_eq!(
        errors(Navree::app().module(LooseFiles)),
        "module `LooseFiles` has files to publish but does not name itself a plugin; call m.plugin(plugin_info!()) in its register"
    );
}

/// A plugin whose files are given by the test.
struct Ships(PublishTag, Vec<&'static str>);
impl Module for Ships {
    fn register(&self, m: &mut ModuleBuilder) {
        let files = self.1.iter().fold(Publishable::new(self.0), |files, path| {
            files.file(*path, b"x")
        });
        m.plugin(plugin_info!(namespace = "announcements"))
            .contribute(files);
    }
}

#[test]
fn an_unsafe_publish_path_fails_the_boot() {
    let rule = "is not a safe relative path inside its folder (no `..`, no leading `/`, no `\\`, no `:`, none of `<>\"|?*`, no segment ending in `.` or a space, no device name such as `con` or `nul`, no two names that differ only in case, no file that is also a folder); fix the path in the plugin";
    for (tag, paths, named) in [
        (
            PublishTag::Views,
            vec!["list.html", "../escape.html"],
            ("views", "../escape.html"),
        ),
        (
            PublishTag::Views,
            vec!["list.html", "List.html"],
            ("views", "List.html"),
        ),
        // Equal only once upper-cased (`ß` is `SS`).
        (
            PublishTag::Views,
            vec!["straße.html", "STRASSE.html"],
            ("views", "STRASSE.html"),
        ),
        (
            PublishTag::Locales,
            vec!["announcements.ftl"],
            ("locales", "announcements.ftl"),
        ),
        (PublishTag::Files, vec!["con.css"], ("files", "con.css")),
        // A file that the other path needs as its folder: one would have to be written over
        // the other.
        (
            PublishTag::Views,
            vec!["admin", "admin/edit.html"],
            ("views", "admin"),
        ),
        (
            PublishTag::Views,
            vec!["Admin", "admin/edit.html"],
            ("views", "Admin"),
        ),
    ] {
        assert_eq!(
            errors(Navree::app().module(Ships(tag, paths))),
            format!(
                "plugin `navree-kernel`: the {} file `{}` {rule}",
                named.0, named.1
            )
        );
    }
    // Safe as the plugin's own path (241 bytes), but its destination
    // `resources/views/announcements/<path>` is 271 bytes: `check` refuses what
    // `plugin:publish` would refuse, and names the plugin's path.
    let long: &'static str = format!("{}/{}", "a".repeat(120), "b".repeat(120)).leak();
    assert_eq!(
        errors(Navree::app().module(Ships(PublishTag::Views, vec![long]))),
        format!("plugin `navree-kernel`: the views file `{long}` {rule}")
    );
    Navree::app()
        .module(Ships(
            PublishTag::Locales,
            vec!["en/announcements.ftl", "pa-IN/announcements.ftl"],
        ))
        .validate()
        .expect("safe paths boot");
}

/// A value the report counts; nothing reads its text.
#[allow(dead_code)]
struct Notice(&'static str);
/// A provider the report and the `plugins` built-in must not build.
struct Costly;

/// The plugin the report and the `plugins` built-in are tried on.
struct Announcements {
    built: Arc<AtomicUsize>,
}
impl Module for Announcements {
    fn register(&self, m: &mut ModuleBuilder) {
        let built = self.built.clone();
        m.plugin(plugin_info!(
            namespace = "announcements",
            title = "Announcements"
        ))
        .factory(Factory::<Costly>::new(move |_| {
            built.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, BoxError>(Arc::new(Costly)) }
        }))
        .contribute(Notice("Gurpreet Singh posted the Ludhiana schedule"))
        .contribute(
            Publishable::new(PublishTag::Views)
                .file("list.html", b"<ul></ul>")
                .file("show.html", b"<p></p>")
                .file("admin/edit.html", b"<form></form>"),
        )
        .contribute(
            Publishable::new(PublishTag::Locales)
                .file("en/announcements.ftl", b"announcements-title = Notices\n")
                .file(
                    "pa-IN/announcements.ftl",
                    "announcements-title = ਸੂਚਨਾਵਾਂ\n".as_bytes(),
                ),
        )
        .command(CommandSpec::new(
            "announcements:prune",
            "Remove old notices",
            |_, _| async { Ok(()) },
        ));
    }
}

/// A module of the app beside a plugin of `plugin_info!()`, whose crate is this whole file
/// (`plugins`): every module type of this file is of that crate, and `Plain` would be listed
/// under the plugin. So the app's module here is one of another crate, the kernel's own
/// `ConfiguredModule`, with the id of `String`.
fn module_of_another_crate() -> impl Module {
    ConfiguredModule::<String, ()>::from_config(|_, _| {})
}

#[tokio::test]
async fn the_plugins_report_needs_no_boot_and_lists_what_each_module_adds() {
    let built = Arc::new(AtomicUsize::new(0));
    let report = Navree::app()
        .module(Announcements {
            built: built.clone(),
        })
        .module(module_of_another_crate())
        .plugin_report()
        .expect("valid");
    let [plugin] = report.plugins.as_slice() else {
        panic!("one plugin: {report:?}")
    };
    assert_eq!(
        plugin.info,
        plugin_info!(namespace = "announcements", title = "Announcements")
    );
    assert!(!plugin.first_party);
    let [module] = plugin.modules.as_slice() else {
        panic!("one module: {plugin:?}")
    };
    assert_eq!(module.id, ModuleId::of::<Announcements>());
    assert_eq!(module.commands, ["announcements:prune"]);
    assert_eq!(
        module.contributions,
        [
            ("navree_kernel::plugin::Publishable", 2),
            ("plugins::Notice", 1),
        ]
    );
    assert_eq!(
        module.publishable,
        [(PublishTag::Views, 3), (PublishTag::Locales, 2)]
    );
    assert_eq!(built.load(Ordering::SeqCst), 0, "nothing is built");

    let app = Navree::app()
        .module(Announcements {
            built: built.clone(),
        })
        .boot()
        .await
        .expect("boots");
    assert_eq!(
        app.ctx().plugins().plugins.len(),
        1,
        "a booted app answers the same"
    );
}

#[test]
fn a_community_plugin_using_plugin_info_is_not_first_party() {
    let report = Navree::app()
        .module(NoticesModule)
        .plugin_report()
        .expect("valid");
    assert!(!report.plugins[0].first_party);
    let json: serde_json::Value = serde_json::from_str(&report.to_json()).expect("JSON");
    assert_eq!(json["plugins"][0]["first_party"], false);
}

#[tokio::test]
async fn the_plugins_command_prints_the_report_and_yields_to_a_module_command() {
    let built = Arc::new(AtomicUsize::new(0));
    for args in [vec!["plugins"], vec!["plugins", "--json"]] {
        Navree::app()
            .module(Announcements {
                built: built.clone(),
            })
            .run_with_args(args.into_iter().map(String::from).collect())
            .await
            .expect("listed");
    }
    assert_eq!(built.load(Ordering::SeqCst), 0, "plugins starts nothing");
    let error = Navree::app()
        .module(Announcements { built })
        .run_with_args(vec!["plugins".into(), "--yaml".into()])
        .await
        .expect_err("unknown argument");
    assert_eq!(
        error.to_string(),
        "plugins: unexpected argument `--yaml`; usage: plugins [--json]"
    );

    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    struct OwnPlugins(Arc<std::sync::Mutex<Vec<String>>>);
    impl Module for OwnPlugins {
        fn register(&self, m: &mut ModuleBuilder) {
            let seen = self.0.clone();
            m.command(CommandSpec::new(
                "plugins",
                "The app's own list",
                move |_, args| {
                    seen.lock().unwrap().extend(args);
                    async { Ok(()) }
                },
            ));
        }
    }
    Navree::app()
        .module(OwnPlugins(seen.clone()))
        .run_with_args(vec!["plugins".into(), "--json".into()])
        .await
        .expect("the app's own command");
    assert_eq!(*seen.lock().unwrap(), ["--json"]);
}

/// Ledger is imported by Billing, which Shop imports. It is the app's ledger: a plugin's module
/// may import it by type but not configure it. Each of its forms contributes a `LedgerForm`.
struct Ledger;
impl Ledger {
    /// The ledger configured in code.
    fn for_root(options: LedgerOptions) -> impl Module {
        ConfiguredModule::<Ledger, LedgerOptions>::for_root(options, register_ledger)
    }

    /// The ledger configured from the app's configuration.
    fn from_config() -> impl Module {
        ConfiguredModule::<Ledger, LedgerOptions>::from_config(register_ledger)
    }
}
impl Module for Ledger {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(LedgerForm("plain"));
    }
}

/// The options of the app's ledger: the city its books are kept in.
#[derive(Clone)]
struct LedgerOptions {
    city: &'static str,
}

/// Which form of `Ledger` registered it: `plain`, `from_config`, or the city `for_root` gave.
struct LedgerForm(&'static str);

/// What a configured `Ledger` registers.
fn register_ledger(m: &mut ModuleBuilder, options: Option<LedgerOptions>) {
    m.contribute(LedgerForm(
        options.map_or("from_config", |options| options.city),
    ));
}
struct Billing;
impl Module for Billing {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(Ledger);
    }
}
struct Shop;
impl Module for Shop {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(Billing);
    }
}

#[tokio::test]
async fn a_module_imports_what_its_imports_import() {
    use navree_kernel::__private::imports;
    let app = Navree::app()
        .module(Shop)
        .module(Plain)
        .boot()
        .await
        .expect("boots");
    let (shop, billing, ledger) = (
        ModuleId::of::<Shop>(),
        ModuleId::of::<Billing>(),
        ModuleId::of::<Ledger>(),
    );
    assert!(imports(app.ctx(), shop, billing));
    assert!(imports(app.ctx(), shop, ledger), "through Billing");
    assert!(!imports(app.ctx(), ledger, shop), "not the other way round");
    assert!(!imports(app.ctx(), shop, shop));
    assert!(!imports(app.ctx(), shop, ModuleId::of::<Plain>()));
    assert!(
        !imports(app.ctx(), shop, ModuleId::of::<NoticesModule>()),
        "not in the app"
    );
}

#[tokio::test]
async fn a_module_of_a_plugin_the_kernel_does_not_find_first_party_is_a_community_plugin() {
    use navree_kernel::__private::is_community_plugin;
    let app = Navree::app()
        .module(NoticesModule)
        .module(Plain)
        .boot()
        .await
        .expect("boots");
    assert!(is_community_plugin(
        app.ctx(),
        ModuleId::of::<NoticesModule>()
    ));
    assert!(
        !is_community_plugin(app.ctx(), ModuleId::of::<Plain>()),
        "no plugin"
    );
    assert!(
        !is_community_plugin(app.ctx(), ModuleId::of::<Shop>()),
        "not in the app"
    );
}

/// A module of the app: its type is at the top of this file, outside the modules that stand for
/// a plugin's crate below. Its id is that of a type of another crate, `String`; the kernel reads
/// the module's own type too, so the id alone would not make it the app's beside a plugin whose
/// crate is this whole file.
struct Outside;
impl Module for Outside {
    fn id(&self) -> ModuleId {
        ModuleId::named::<String>("Outside")
    }

    fn register(&self, _: &mut ModuleBuilder) {}
}

#[tokio::test]
async fn a_module_of_the_app_names_no_plugin_and_is_in_no_plugins_crate() {
    use navree_kernel::__private::is_app_module;
    use navree_plugin_notices::{Helper, NoticesModule};
    let app = Navree::app()
        .module(NoticesModule::with_policy())
        .module(Helper)
        .module(Outside)
        .boot()
        .await
        .expect("boots");
    assert!(is_app_module(
        app.ctx(),
        ModuleId::named::<String>("Outside")
    ));
    assert!(
        !is_app_module(app.ctx(), ModuleId::of::<NoticesModule>()),
        "a module of a plugin"
    );
    assert!(
        !is_app_module(app.ctx(), ModuleId::of::<Helper>()),
        "names no plugin, but it is in the crate the plugin names"
    );
    assert!(
        !is_app_module(app.ctx(), ModuleId::of::<Shop>()),
        "not in the app"
    );
}

// Which plugin a module acts for. No plugin of this file's crate (`plugins`) is in these apps:
// its crate would hold every type at the top of this file.

/// The notices plugin: its crate is this module.
mod navree_plugin_notices {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    /// The plugin's info.
    pub const NOTICES: PluginInfo =
        PluginInfo::new("navree-plugin-notices", module_path!(), "notices", "1.0.0");

    /// A policy type of the plugin, named in a generic module of another crate.
    pub struct Open;

    /// The plugin's module: it names the plugin, then registers what its function adds.
    pub struct NoticesModule(pub fn(&mut ModuleBuilder));

    impl NoticesModule {
        /// The plugin's module importing `PolicyOf<Open>`, as a plugin imports its policy.
        pub fn with_policy() -> NoticesModule {
            NoticesModule(|m| {
                m.import(super::PolicyOf::<Open>::new());
            })
        }
    }

    impl Module for NoticesModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(NOTICES);
            (self.0)(m);
        }
    }

    /// A module of the plugin's crate that names no plugin.
    pub struct Helper;

    impl Module for Helper {
        fn register(&self, _: &mut ModuleBuilder) {}
    }

    /// The notice board plugin, a crate nested in the notices crate.
    pub mod board {
        use navree_kernel::{Module, ModuleBuilder, PluginInfo};

        /// The plugin's info.
        pub const BOARD: PluginInfo = PluginInfo::new(
            "navree-plugin-notice-board",
            module_path!(),
            "notice_board",
            "1.0.0",
        );

        /// The notice board's module.
        pub struct BoardModule;

        impl Module for BoardModule {
            fn register(&self, m: &mut ModuleBuilder) {
                m.plugin(BOARD);
            }
        }

        /// A module of the notice board's crate that names no plugin.
        pub struct BoardHelper;

        impl Module for BoardHelper {
            fn register(&self, _: &mut ModuleBuilder) {}
        }
    }
}

/// The weather and monsoon plugins: two plugins of one crate, this module.
mod navree_plugin_weather {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    /// The weather plugin's module.
    pub struct WeatherModule;

    impl Module for WeatherModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PluginInfo::new(
                "navree-plugin-weather",
                module_path!(),
                "weather",
                "1.0.0",
            ));
        }
    }

    /// The monsoon plugin's module.
    pub struct MonsoonModule;

    impl Module for MonsoonModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PluginInfo::new(
                "navree-plugin-monsoon",
                module_path!(),
                "monsoon",
                "1.0.0",
            ));
        }
    }

    /// A module of the crate of both that names no plugin.
    pub struct Rain;

    impl Module for Rain {
        fn register(&self, _: &mut ModuleBuilder) {}
    }
}

/// A type of a crate that is no plugin's, such as a plugin's `-core` crate.
mod khata_core {
    /// A policy type of that crate.
    pub struct Open;
}

/// The khata plugin: its crate is this module.
mod navree_plugin_khata {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    /// The plugin's info.
    pub const KHATA: PluginInfo =
        PluginInfo::new("navree-plugin-khata", module_path!(), "khata", "1.0.0");

    /// A policy type of the plugin.
    pub struct Open;

    /// The plugin's module: it names the plugin, then registers what its function adds.
    pub struct KhataModule(pub fn(&mut ModuleBuilder));

    impl Module for KhataModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(KHATA);
            (self.0)(m);
        }
    }
}

/// A generic module of this crate standing for `AccessModule::for_policy::<P>()`: it contributes a
/// `Grant`.
struct PolicyOf<P>(PhantomData<fn() -> P>);

impl<P> PolicyOf<P> {
    fn new() -> PolicyOf<P> {
        PolicyOf(PhantomData)
    }
}

impl<P: 'static> Module for PolicyOf<P> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(Grant);
    }
}

/// What `PolicyOf` contributes.
struct Grant;

/// A generic module that imports `PolicyOf<P>`.
struct Outer<P>(PhantomData<fn() -> P>);

impl<P: 'static> Module for Outer<P> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(PolicyOf::<P>::new());
    }
}

/// A type of this crate, of no plugin's.
struct Shared;

/// The name of the plugin `module` acts for in `app`.
fn acting_for(app: &navree_kernel::App, module: ModuleId) -> Option<&'static str> {
    navree_kernel::__private::acts_for(app.ctx(), module).map(|plugin| plugin.name)
}

#[tokio::test]
async fn a_wrapper_module_acts_for_the_plugin_that_imports_it_and_for_the_app_that_does() {
    use navree_plugin_notices::{NoticesModule, Open};
    let policy = ModuleId::of::<PolicyOf<Open>>();

    // Only the plugin imports it: the plugin's.
    let app = Navree::app()
        .module(NoticesModule::with_policy())
        .module(Outside)
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, policy), Some("navree-plugin-notices"));
    assert_eq!(
        acting_for(&app, ModuleId::of::<NoticesModule>()),
        Some("navree-plugin-notices")
    );
    assert_eq!(acting_for(&app, ModuleId::named::<String>("Outside")), None);
    assert_eq!(
        acting_for(&app, ModuleId::of::<Shop>()),
        None,
        "not in the app"
    );

    // The app imports it too: the app's.
    let app = Navree::app()
        .module(NoticesModule::with_policy())
        .module(PolicyOf::<Open>::new())
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, policy), None);

    // Only the app imports it: the app's.
    let app = Navree::app()
        .module(NoticesModule(|_| {}))
        .module(PolicyOf::<Open>::new())
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, policy), None);

    // Two levels below the plugin: both the plugin's.
    let app = Navree::app()
        .module(NoticesModule(|m| {
            m.import(Outer::<Open>(PhantomData));
        }))
        .boot()
        .await
        .expect("boots");
    assert_eq!(
        acting_for(&app, ModuleId::of::<Outer<Open>>()),
        Some("navree-plugin-notices")
    );
    assert_eq!(acting_for(&app, policy), Some("navree-plugin-notices"));
}

#[tokio::test]
async fn a_module_of_a_plugins_crate_acts_for_it_without_naming_it() {
    use navree_kernel::__private::{is_app_module, is_community_plugin};
    use navree_plugin_notices::{Helper, NoticesModule};
    let app = Navree::app()
        .module(NoticesModule::with_policy())
        .module(Helper)
        .module(Outside)
        .boot()
        .await
        .expect("boots");
    let helper = ModuleId::of::<Helper>();
    assert_eq!(acting_for(&app, helper), Some("navree-plugin-notices"));
    assert!(
        !is_community_plugin(app.ctx(), helper),
        "it names no plugin"
    );
    assert!(!is_app_module(app.ctx(), helper));
    assert!(is_app_module(
        app.ctx(),
        ModuleId::named::<String>("Outside")
    ));
}

#[tokio::test]
async fn the_most_specific_crate_decides_which_plugin_a_module_acts_for() {
    use navree_plugin_notices::board::{BoardHelper, BoardModule};
    use navree_plugin_notices::{Helper, NoticesModule};
    let app = Navree::app()
        .module(NoticesModule::with_policy())
        .module(BoardModule)
        .module(Helper)
        .module(BoardHelper)
        .boot()
        .await
        .expect("boots");
    assert_eq!(
        acting_for(&app, ModuleId::of::<Helper>()),
        Some("navree-plugin-notices")
    );
    assert_eq!(
        acting_for(&app, ModuleId::of::<BoardHelper>()),
        Some("navree-plugin-notice-board")
    );

    // Two plugins of one crate: the plugin whose module the app lists first.
    use navree_plugin_weather::{MonsoonModule, Rain, WeatherModule};
    let rain = ModuleId::of::<Rain>();
    let app = Navree::app()
        .module(WeatherModule)
        .module(MonsoonModule)
        .module(Rain)
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, rain), Some("navree-plugin-weather"));
    let app = Navree::app()
        .module(MonsoonModule)
        .module(WeatherModule)
        .module(Rain)
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, rain), Some("navree-plugin-monsoon"));
}

#[test]
fn plugins_lists_a_policy_module_under_its_plugin() {
    use navree_plugin_notices::{NoticesModule, Open};
    let report = Navree::app()
        .module(NoticesModule::with_policy())
        .plugin_report()
        .expect("valid");
    let [plugin] = report.plugins.as_slice() else {
        panic!("one plugin: {report:?}")
    };
    let ids: Vec<ModuleId> = plugin.modules.iter().map(|module| module.id).collect();
    assert_eq!(
        ids,
        [
            ModuleId::of::<PolicyOf<Open>>(),
            ModuleId::of::<NoticesModule>()
        ],
        "in dependency order"
    );
    assert_eq!(plugin.modules[0].contributions, [("plugins::Grant", 1)]);
    let text = report.to_string();
    let lines: Vec<&str> = text.lines().collect();
    for expected in [
        "  PolicyOf<plugins::navree_plugin_notices::Open>  plugins::PolicyOf<plugins::navree_plugin_notices::Open>",
        "    adds      1 Grant",
    ] {
        assert!(lines.contains(&expected), "{text}");
    }

    // The app imports the policy module too: the plugin lists its own module alone.
    let report = Navree::app()
        .module(NoticesModule::with_policy())
        .module(PolicyOf::<Open>::new())
        .plugin_report()
        .expect("valid");
    let ids: Vec<ModuleId> = report.plugins[0]
        .modules
        .iter()
        .map(|module| module.id)
        .collect();
    assert_eq!(ids, [ModuleId::of::<NoticesModule>()]);
}

#[tokio::test]
async fn a_generic_module_only_plugins_import_is_never_the_apps() {
    use navree_plugin_khata::KhataModule;
    use navree_plugin_notices::NoticesModule;

    // A policy type of the plugin's `-core` crate: the plugin's.
    let core = ModuleId::of::<PolicyOf<khata_core::Open>>();
    let app = Navree::app()
        .module(KhataModule(|m| {
            m.import(PolicyOf::<khata_core::Open>::new());
        }))
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, core), Some("navree-plugin-khata"));

    // A wrapper two plugins import: the plugin the app lists first.
    let shared = ModuleId::of::<PolicyOf<Shared>>();
    let notices = || {
        NoticesModule(|m| {
            m.import(PolicyOf::<Shared>::new());
        })
    };
    let khata = || {
        KhataModule(|m| {
            m.import(PolicyOf::<Shared>::new());
        })
    };
    let app = Navree::app()
        .module(notices())
        .module(khata())
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, shared), Some("navree-plugin-notices"));
    let app = Navree::app()
        .module(khata())
        .module(notices())
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, shared), Some("navree-plugin-khata"));

    // Its arguments name one of them: that one, whatever the order.
    let own = ModuleId::of::<PolicyOf<navree_plugin_khata::Open>>();
    let app = Navree::app()
        .module(NoticesModule(|m| {
            m.import(PolicyOf::<navree_plugin_khata::Open>::new());
        }))
        .module(KhataModule(|m| {
            m.import(PolicyOf::<navree_plugin_khata::Open>::new());
        }))
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, own), Some("navree-plugin-khata"));

    // The app imports it as well: the app's.
    let app = Navree::app()
        .module(NoticesModule(|m| {
            m.import(PolicyOf::<navree_plugin_khata::Open>::new());
        }))
        .module(KhataModule(|m| {
            m.import(PolicyOf::<navree_plugin_khata::Open>::new());
        }))
        .module(PolicyOf::<navree_plugin_khata::Open>::new())
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, own), None);
}

// A plugin that configures a module of another crate.

/// The forecast plugin: its crate is this module.
mod navree_plugin_forecast {
    use navree_kernel::{ConfiguredModule, Module, ModuleBuilder, PluginInfo};

    /// The plugin's info.
    const FORECAST: PluginInfo = PluginInfo::new(
        "navree-plugin-forecast",
        module_path!(),
        "forecast",
        "1.0.0",
    );

    /// The plugin's module: it names the plugin, then registers what its function adds.
    pub struct ForecastModule(pub fn(&mut ModuleBuilder));

    impl Module for ForecastModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(FORECAST);
            (self.0)(m);
        }
    }

    /// The options of the plugin's own store.
    #[derive(Clone)]
    pub struct StoreOptions;

    /// The plugin's own store.
    pub struct ForecastStore;

    impl ForecastStore {
        /// The store configured from the app's configuration.
        pub fn from_config() -> impl Module {
            ConfiguredModule::<ForecastStore, StoreOptions>::from_config(|_, _| {})
        }
    }
}

/// The refusal of the forecast plugin's module configuring the app's `Ledger`.
const FORECAST_CONFIGURES_LEDGER: &str = "plugin `navree-plugin-forecast` configures the module `Ledger` in `ForecastModule` (for_root or from_config); only a module of the app configures a module of another crate: remove the plugin or update it to a version that imports `Ledger` without configuring it, and configure `Ledger` in a module of the app";

#[test]
fn a_plugin_that_configures_another_crates_module_fails_the_boot() {
    use navree_plugin_forecast::ForecastModule;
    let ludhiana = || LedgerOptions { city: "Ludhiana" };
    for app in [
        // Only the plugin imports the module, configured in code or from the configuration.
        Navree::app().module(ForecastModule(|m| {
            m.import(Ledger::for_root(LedgerOptions { city: "Ludhiana" }));
        })),
        Navree::app().module(ForecastModule(|m| {
            m.import(Ledger::from_config());
        })),
        // The app imports it by type.
        Navree::app().module(Ledger).module(ForecastModule(|m| {
            m.import(Ledger::for_root(LedgerOptions { city: "Ludhiana" }));
        })),
        // The app configures it too, in either order: the plugin's refusal, alone.
        Navree::app()
            .module(Ledger::for_root(ludhiana()))
            .module(ForecastModule(|m| {
                m.import(Ledger::from_config());
            })),
        Navree::app()
            .module(ForecastModule(|m| {
                m.import(Ledger::from_config());
            }))
            .module(Ledger::for_root(ludhiana())),
    ] {
        let text = errors(app);
        assert_eq!(text, FORECAST_CONFIGURES_LEDGER);
        assert!(!text.contains("is configured more than once"));
    }
}

#[test]
fn a_plugin_configures_a_module_its_types_name() {
    use navree_plugin_forecast::{ForecastModule, ForecastStore, StoreOptions};
    Navree::app()
        .module(ForecastModule(|m| {
            m.import(ForecastStore::from_config())
                .import(
                    ConfiguredModule::<Holder<StoreOptions>, StoreOptions>::for_root(
                        StoreOptions,
                        |_, _| {},
                    ),
                );
        }))
        .validate()
        .expect("its own store, and a module whose generic arguments name its crate");
}

#[tokio::test]
async fn the_app_configures_a_module_a_plugin_imports_by_type() {
    use navree_plugin_forecast::ForecastModule;
    let app = Navree::app()
        .module(Ledger::for_root(LedgerOptions { city: "Pune" }))
        .module(ForecastModule(|m| {
            m.import(Ledger);
        }))
        .boot()
        .await
        .expect("the app configures its ledger");
    let forms: Vec<&str> = app
        .ctx()
        .contributions::<LedgerForm>()
        .iter()
        .map(|form| form.value.0)
        .collect();
    assert_eq!(forms, ["Pune"]);
}

// What a module is, whoever passes it.

/// A crate that hands its modules over through a list of its own.
mod navree_plugin_claims {
    use navree_kernel::__private::PassedModule;
    use navree_kernel::{IntoModules, Module, ModuleBuilder, PluginInfo};

    /// A module that names the flags package's plugin, at the kernel's version.
    pub struct Claim;

    impl Module for Claim {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PluginInfo::new(
                "navree-flags",
                "navree_flags",
                "flags",
                env!("CARGO_PKG_VERSION"),
            ));
        }
    }

    /// The crate's own list of modules: it can pass `Claim` only as what it is.
    pub struct Claims;

    impl IntoModules for Claims {
        fn into_modules(self) -> Vec<Box<dyn Module>> {
            vec![Box::new(Claim)]
        }

        fn into_passed_modules(self) -> Vec<PassedModule> {
            vec![PassedModule::of(Claim)]
        }
    }
}

/// A module of the app that imports the claims crate's list.
struct ImportsClaims;

impl Module for ImportsClaims {
    fn register(&self, m: &mut ModuleBuilder) {
        m.imports(navree_plugin_claims::Claims);
    }
}

#[test]
fn a_crates_own_list_of_modules_passes_each_as_what_it_is() {
    // `Claim` names the flags package's crate, but it is a type of this crate: refused, and
    // never first-party.
    assert_eq!(
        errors(Navree::app().module(ImportsClaims)),
        "module `Claim` names the plugin `navree-flags`, which is another crate's; a plugin's modules are in its own crate (a ClientModule names it through an options type of the plugin's crate)"
    );
}

// A plugin's commands.

/// The listing plugin: its crate is this module.
mod navree_plugin_listing {
    use navree_kernel::{CommandSpec, Module, ModuleBuilder, PluginInfo};

    /// The plugin's info.
    const LISTING: PluginInfo =
        PluginInfo::new("navree-plugin-listing", module_path!(), "listing", "1.0.0");

    /// The plugin's module: it registers the command `.0`.
    pub struct Commands(pub &'static str);

    impl Module for Commands {
        fn register(&self, m: &mut ModuleBuilder) {
            let command = CommandSpec::new(self.0, "The plugin's", |_, _| async { Ok(()) });
            m.plugin(LISTING).command(command);
        }
    }
}

/// A module of the app that registers the command `.0`.
struct AppCommand(&'static str);

impl Module for AppCommand {
    fn register(&self, m: &mut ModuleBuilder) {
        let command = CommandSpec::new(self.0, "The app's", |_, _| async { Ok(()) });
        m.command(command);
    }
}

#[test]
fn a_plugin_may_not_take_the_name_of_a_framework_command() {
    use navree_plugin_listing::Commands;
    // A plugin's `plugins` would print its own listing in place of the framework's, and its
    // `check` would stand beside the framework's in the app's list.
    for name in ["plugins", "plugin:publish", "check", "help"] {
        assert_eq!(
            errors(Navree::app().module(Commands(name))),
            format!(
                "plugin `navree-plugin-listing` registers the command `{name}` in `Commands`, a command of the framework's own; only a module of the app may take its name: remove the plugin or update it to a version that names its command otherwise"
            )
        );
    }
    // A command of its own name is the plugin's, and the app's module takes the built-in's
    // name beside it, as before.
    Navree::app()
        .module(Commands("listing:show"))
        .module(AppCommand("plugins"))
        .validate()
        .expect("the plugin's own command, and the app's `plugins`");
}

// A module typed as a reference.

/// The helper plugin: its crate is this module.
mod navree_plugin_helper {
    use navree_kernel::{CommandSpec, Module, ModuleBuilder, ModuleId, PluginInfo};

    /// The plugin's info.
    const HELPER: PluginInfo =
        PluginInfo::new("navree-plugin-helper", module_path!(), "helper", "1.0.0");

    /// A helper of the plugin's crate, which the plugin's module imports by reference.
    pub struct Helper;

    /// The one helper.
    pub static THE_HELPER: Helper = Helper;

    /// The helper as a module, typed as a reference to it, with an id that names no crate of
    /// a plugin: it registers the framework's command `check`.
    impl Module for &'static Helper {
        fn id(&self) -> ModuleId {
            ModuleId::named::<String>("helper")
        }

        fn register(&self, m: &mut ModuleBuilder) {
            let command = CommandSpec::new("check", "The helper's", |_, _| async { Ok(()) });
            m.command(command);
        }
    }

    /// The plugin's module: it imports the helper by reference.
    pub struct HelperPlugin;

    impl Module for HelperPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(HELPER).import(&THE_HELPER);
        }
    }
}

#[test]
fn a_module_typed_as_a_reference_acts_for_the_plugin_of_the_type_behind_it() {
    use navree_plugin_helper::HelperPlugin;
    // Its own type is `&Helper`, read as `Helper`, a type of the plugin's crate: it acts for
    // the plugin, whatever its id names, and is held to the plugin's rules.
    assert_eq!(
        errors(Navree::app().module(HelperPlugin)),
        "plugin `navree-plugin-helper` registers the command `check` in `helper` (a module of the plugin's crate `plugins::navree_plugin_helper`), a command of the framework's own; only a module of the app may take its name: remove the plugin or update it to a version that names its command otherwise"
    );
}

// A module typed as a reference to a trait object.

/// The parts plugin: its crate is this module.
mod navree_plugin_parts {
    use navree_kernel::{CommandSpec, Module, ModuleBuilder, ModuleId, PluginInfo};

    /// The plugin's info.
    const PARTS: PluginInfo =
        PluginInfo::new("navree-plugin-parts", module_path!(), "parts", "1.0.0");

    /// A part of the plugin: a trait of the plugin's crate.
    pub trait Part: Sync {
        /// The command the part registers.
        fn command(&self) -> &'static str;
    }

    /// The plugin's one part: it registers the framework's command `check`.
    pub struct Gear;

    impl Part for Gear {
        fn command(&self) -> &'static str {
            "check"
        }
    }

    /// The one part, as a trait object.
    pub static THE_PART: &dyn Part = &Gear;

    /// A part as a module, typed as a reference to the trait object, with an id that names no
    /// crate of a plugin.
    impl Module for &'static dyn Part {
        fn id(&self) -> ModuleId {
            ModuleId::named::<String>("part")
        }

        fn register(&self, m: &mut ModuleBuilder) {
            let command = CommandSpec::new(self.command(), "The part's", |_, _| async { Ok(()) });
            m.command(command);
        }
    }

    /// The plugin's module: it imports the part as a trait object.
    pub struct PartsPlugin;

    impl Module for PartsPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PARTS).import(THE_PART);
        }
    }
}

#[test]
fn a_module_typed_as_a_trait_object_acts_for_the_plugin_of_the_traits_crate() {
    use navree_plugin_parts::PartsPlugin;
    // Its own type is `&dyn Part`, read as `Part`, a trait of the plugin's crate: only that
    // crate can make such a type a module, so it acts for the plugin, whatever its id names,
    // and is held to the plugin's rules.
    assert_eq!(
        errors(Navree::app().module(PartsPlugin)),
        "plugin `navree-plugin-parts` registers the command `check` in `part` (a module of the plugin's crate `plugins::navree_plugin_parts`), a command of the framework's own; only a module of the app may take its name: remove the plugin or update it to a version that names its command otherwise"
    );
}

// A plugin's second crate.

/// A second crate that names no plugin, such as a plugin's `-core` crate.
mod navree_plugin_khata_core {
    use navree_kernel::{CommandSpec, Module, ModuleBuilder};

    /// A note contributed by the second crate's module.
    pub struct CoreEntry;

    /// A plain module of the second crate: it names no plugin and registers what its function
    /// adds.
    pub struct KhataCoreModule(pub fn(&mut ModuleBuilder));

    impl Module for KhataCoreModule {
        fn register(&self, m: &mut ModuleBuilder) {
            (self.0)(m);
        }
    }

    /// A deeper plain module of the second crate, imported only by `KhataCoreModule`.
    pub struct KhataStoreModule;

    impl Module for KhataStoreModule {
        fn register(&self, m: &mut ModuleBuilder) {
            let sync = CommandSpec::new("khata:sync", "Sync ledgers", |_, _| async { Ok(()) });
            m.contribute(CoreEntry).command(sync);
        }
    }
}

/// A module of the app that imports the second crate's module.
struct ShopImportsCore;

impl Module for ShopImportsCore {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(navree_plugin_khata_core::KhataCoreModule(|_| {}));
    }
}

#[tokio::test]
async fn a_module_of_a_second_crate_that_only_a_plugins_modules_import_acts_for_that_plugin() {
    use navree_kernel::__private::{is_app_module, is_community_plugin};
    use navree_plugin_khata::KhataModule;
    use navree_plugin_khata_core::{KhataCoreModule, KhataStoreModule};

    let app = Navree::app()
        .module(KhataModule(|m| {
            m.import(KhataCoreModule(|m| {
                m.import(KhataStoreModule);
            }));
        }))
        .module(Outside)
        .boot()
        .await
        .expect("boots");

    let core = ModuleId::of::<KhataCoreModule>();
    let store = ModuleId::of::<KhataStoreModule>();
    assert_eq!(acting_for(&app, core), Some("navree-plugin-khata"));
    assert_eq!(
        acting_for(&app, store),
        Some("navree-plugin-khata"),
        "imported through the second crate's own module"
    );
    assert!(!is_app_module(app.ctx(), core));
    assert!(!is_app_module(app.ctx(), store));
    assert!(
        !is_community_plugin(app.ctx(), core),
        "it names no plugin itself"
    );
}

#[tokio::test]
async fn a_module_of_a_second_crate_that_the_app_imports_too_stays_the_apps() {
    use navree_kernel::__private::is_app_module;
    use navree_plugin_khata::KhataModule;
    use navree_plugin_khata_core::KhataCoreModule;

    let core = ModuleId::of::<KhataCoreModule>();
    let khata = || {
        KhataModule(|m| {
            m.import(KhataCoreModule(|_| {}));
        })
    };

    // Imported directly by the app's root module beside the plugin.
    let app = Navree::app()
        .module(khata())
        .module(KhataCoreModule(|_| {}))
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, core), None);
    assert!(is_app_module(app.ctx(), core));

    // Imported by a module of the app, whether listed after or before the plugin.
    for app in [
        Navree::app().module(khata()).module(ShopImportsCore),
        Navree::app().module(ShopImportsCore).module(khata()),
    ] {
        let app = app.boot().await.expect("boots");
        assert_eq!(acting_for(&app, core), None);
        assert!(is_app_module(app.ctx(), core));
    }
}

#[tokio::test]
async fn a_module_of_a_second_crate_imported_by_two_plugins_acts_for_the_first_of_them() {
    use navree_plugin_khata::KhataModule;
    use navree_plugin_khata_core::KhataCoreModule;
    use navree_plugin_notices::NoticesModule;

    let core = ModuleId::of::<KhataCoreModule>();
    let notices = || {
        NoticesModule(|m| {
            m.import(KhataCoreModule(|_| {}));
        })
    };
    let khata = || {
        KhataModule(|m| {
            m.import(KhataCoreModule(|_| {}));
        })
    };

    let app = Navree::app()
        .module(notices())
        .module(khata())
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, core), Some("navree-plugin-notices"));

    let app = Navree::app()
        .module(khata())
        .module(notices())
        .boot()
        .await
        .expect("boots");
    assert_eq!(acting_for(&app, core), Some("navree-plugin-khata"));
}

#[test]
fn a_second_crates_module_is_listed_under_its_plugin_and_held_to_its_rules() {
    use navree_plugin_khata::KhataModule;
    use navree_plugin_khata_core::{CoreEntry, KhataCoreModule, KhataStoreModule};

    // Listed under the plugin in dependency order, with what it adds and registers.
    let report = Navree::app()
        .module(KhataModule(|m| {
            m.import(KhataCoreModule(|m| {
                m.import(KhataStoreModule).contribute(CoreEntry);
            }));
        }))
        .plugin_report()
        .expect("valid");
    let [plugin] = report.plugins.as_slice() else {
        panic!("one plugin: {report:?}")
    };
    assert_eq!(plugin.info.name, "navree-plugin-khata");
    let ids: Vec<ModuleId> = plugin.modules.iter().map(|module| module.id).collect();
    assert_eq!(
        ids,
        [
            ModuleId::of::<KhataStoreModule>(),
            ModuleId::of::<KhataCoreModule>(),
            ModuleId::of::<KhataModule>(),
        ]
    );
    assert_eq!(plugin.modules[0].commands, ["khata:sync"]);
    assert_eq!(
        plugin.modules[0].contributions,
        [("plugins::navree_plugin_khata_core::CoreEntry", 1)]
    );

    // Held to the rules: configuring another crate's module, or taking a framework command's
    // name, from the second crate is refused naming why it acts for the plugin.
    assert_eq!(
        errors(Navree::app().module(KhataModule(|m| {
            m.import(KhataCoreModule(|m| {
                m.import(Ledger::from_config());
            }));
        }))),
        "plugin `navree-plugin-khata` configures the module `Ledger` in `KhataCoreModule` (only the plugin's modules import it) (for_root or from_config); only a module of the app configures a module of another crate: remove the plugin or update it to a version that imports `Ledger` without configuring it, and configure `Ledger` in a module of the app"
    );
    assert_eq!(
        errors(Navree::app().module(KhataModule(|m| {
            m.import(KhataCoreModule(|m| {
                let check = CommandSpec::new("check", "The plugin's", |_, _| async { Ok(()) });
                m.command(check);
            }));
        }))),
        "plugin `navree-plugin-khata` registers the command `check` in `KhataCoreModule` (only the plugin's modules import it), a command of the framework's own; only a module of the app may take its name: remove the plugin or update it to a version that names its command otherwise"
    );
}

#[tokio::test]
async fn a_framework_module_that_only_a_plugin_imports_acts_for_no_plugin() {
    use navree_kernel::__private::{PlainModule, is_app_module};
    use navree_plugin_khata::KhataModule;

    let app = Navree::app()
        .module(KhataModule(|m| {
            m.import(PlainModule);
        }))
        .boot()
        .await
        .expect("boots");
    let plain = ModuleId::of::<PlainModule>();
    assert_eq!(acting_for(&app, plain), None);
    assert!(is_app_module(app.ctx(), plain));
    let report = app.ctx().plugins();
    let [plugin] = report.plugins.as_slice() else {
        panic!("one plugin")
    };
    let ids: Vec<ModuleId> = plugin.modules.iter().map(|module| module.id).collect();
    assert_eq!(ids, [ModuleId::of::<KhataModule>()]);
}
