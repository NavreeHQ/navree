//! Templates a plugin ships: registered under its namespace, beaten by a file
//! in a template folder, publishable, and checked when the app boots.

#[path = "view/support.rs"]
mod support;

use navree_kernel::{App, BootErrors, Module, ModuleBuilder, Navree, PluginInfo, PublishTag};
use navree_view::template::{AutoEscape, Environment, Value};
use navree_view::{
    ModuleBuilderPluginTemplatesExt, ModuleBuilderViewExt, TemplateDir, ViewModule, ViewOptions,
    Views,
};
use serde_json::json;

use crate::navree_plugin_announcements::{
    AnnouncementsModule, Hook, Registers, RegistersAfterImport,
};
use crate::support::{options, views_dir};

/// The announcements plugin: its crate is this module, so the app's modules at the top of the
/// file are in no crate it names.
mod navree_plugin_announcements {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};
    use navree_view::template::Environment;
    use navree_view::{ModuleBuilderPluginTemplatesExt, ModuleBuilderViewExt};

    use crate::AppMail;

    /// The plugin's info.
    fn announcements() -> PluginInfo {
        PluginInfo::new(
            "navree-plugin-announcements",
            module_path!(),
            "announcements",
            "1.0.0",
        )
    }

    /// The plugin: `m.plugin_templates` before `m.plugin`, from the fixtures folder.
    pub struct AnnouncementsModule;
    impl Module for AnnouncementsModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin_templates(navree_macros::embed_templates!(
                "tests/fixtures/plugin-views"
            ))
            .plugin(announcements());
        }
    }

    /// The plugin, registering the template `.0` with `m.templates`.
    pub struct Registers(pub &'static str);
    impl Module for Registers {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(announcements())
                .templates(&[(self.0, "<p>from the plugin</p>")]);
        }
    }

    /// The plugin, importing the app's mail module and registering the template `.0`.
    pub struct RegistersAfterImport(pub &'static str);
    impl Module for RegistersAfterImport {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(announcements())
                .import(AppMail)
                .templates(&[(self.0, "<p>from the plugin</p>")]);
        }
    }

    /// A hook of the plugin that changes the environment as `.0` says.
    pub struct Hook(pub fn(&mut Environment));
    impl Module for Hook {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(announcements()).view_environment(self.0);
        }
    }

    pub const CRATE: &str = module_path!();

    /// A module of the plugin's crate that names no plugin itself.
    pub struct Helper(pub fn(&mut ModuleBuilder));
    impl Module for Helper {
        fn register(&self, m: &mut ModuleBuilder) {
            (self.0)(m);
        }
    }
}

#[tokio::test]
async fn plugin_templates_are_registered_under_the_namespace_and_disk_wins() {
    let notices = json!({ "notices": ["Fee for Ludhiana: ₹1,00,000"] });
    let app = Navree::app()
        .module(ViewModule::for_root(options(&[])))
        .module(AnnouncementsModule)
        .boot()
        .await
        .expect("boots");
    let views = app.ctx().get::<Views>().await.expect("Views");
    assert_eq!(
        views
            .render("announcements/list", &notices)
            .unwrap()
            .trim_end(),
        "<ul><li>Fee for Ludhiana: ₹1,00,000</li></ul>"
    );
    assert!(views.exists("announcements/admin/edit.html"));
    assert!(!views.exists("list.html"), "only under the namespace");
    app.shutdown().await.unwrap();

    let dir = views_dir(&[("announcements/list.html", "Meera Sharma's list")]);
    let app = Navree::app()
        .module(ViewModule::for_root(options(&[dir.path()])))
        .module(AnnouncementsModule)
        .boot()
        .await
        .expect("boots");
    let views = app.ctx().get::<Views>().await.expect("Views");
    assert_eq!(
        views.render("announcements/list", &notices).unwrap(),
        "Meera Sharma's list"
    );
    app.shutdown().await.unwrap();

    let report = Navree::app()
        .module(AnnouncementsModule)
        .plugin_report()
        .expect("valid");
    assert_eq!(
        report.plugins[0].modules[0].publishable,
        [(PublishTag::Views, 2)]
    );
}

#[test]
fn unsafe_template_paths_fail_the_boot() {
    const UNSAFE: &[(&str, &str)] = &[
        ("../escape.html", "x"),
        ("/etc/escape.html", "x"),
        ("admin\\edit.html", "x"),
        ("con.html", "x"),
        ("List.html", "x"),
        ("list.html", "x"),
    ];
    struct Unsafe;
    impl Module for Unsafe {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PluginInfo::new(
                "navree-plugin-announcements",
                env!("CARGO_CRATE_NAME"),
                "announcements",
                "1.0.0",
            ))
            .plugin_templates(TemplateDir::from(UNSAFE));
        }
    }
    let text = Navree::app()
        .module(Unsafe)
        .validate()
        .expect_err("unsafe")
        .to_string();
    for path in [
        "../escape.html",
        "/etc/escape.html",
        "admin\\edit.html",
        "con.html",
        "list.html",
    ] {
        assert!(
            text.contains(&format!(
                "the views file `{path}` is not a safe relative path inside its folder"
            )),
            "{path}: {text}"
        );
    }
}

#[test]
fn a_plugin_cannot_ship_templates_under_the_layouts_namespace() {
    const MAIL: &[(&str, &str)] = &[("mail.html", "<main>{% block body %}{% endblock %}</main>")];
    struct Layouts;
    impl Module for Layouts {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PluginInfo::new(
                "navree-plugin-layouts",
                env!("CARGO_CRATE_NAME"),
                "layouts",
                "1.0.0",
            ))
            .plugin_templates(TemplateDir::from(MAIL));
        }
    }
    let text = Navree::app()
        .module(Layouts)
        .validate()
        .expect_err("reserved")
        .to_string();
    assert!(
        text.contains(
            "plugin `navree-plugin-layouts` uses the namespace `layouts`, which the framework keeps for itself"
        ),
        "{text}"
    );
}

/// `ViewModule::for_root(options)` and `module`, booted.
async fn boot_with(options: ViewOptions, module: impl Module) -> Result<App, BootErrors> {
    Navree::app()
        .module(ViewModule::for_root(options))
        .module(module)
        .boot()
        .await
}

/// What `name` renders as in `app`.
async fn render(app: &App, name: &str) -> String {
    app.ctx()
        .get::<Views>()
        .await
        .unwrap()
        .render(name, json!({}))
        .unwrap()
}

/// A module of the app with a welcome mail.
struct AppMail;
impl Module for AppMail {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("mail/welcome.html", "Welcome from the app")]);
    }
}

/// A module of the app registering the template `name`.
struct AppRegisters(&'static str);
impl Module for AppRegisters {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[(self.0, "<p>from the app</p>")]);
    }
}

#[tokio::test]
async fn a_plugins_template_outside_its_namespace_fails_the_boot() {
    let names = [
        "mail/welcome.html",
        "../x.html",
        "announcements/../mail/welcome.html",
        "announcements.html",
    ];
    for name in names {
        let error = boot_with(options(&[]), Registers(name))
            .await
            .expect_err("fails")
            .to_string();
        assert!(
            error.contains(&format!(
                "plugin `navree-plugin-announcements` registers the template `{name}` in `Registers`, which is not under its namespace: name it `announcements/..` or ship it with m.plugin_templates"
            )),
            "{name}: {error}"
        );
    }

    // Under its namespace it boots.
    let app = boot_with(options(&[]), Registers("announcements/list.html"))
        .await
        .expect("boots");
    assert_eq!(
        render(&app, "announcements/list").await,
        "<p>from the plugin</p>"
    );
    app.shutdown().await.unwrap();

    // The exact name of a template that a module it imports registers is the replacement rule's:
    // its error, not this one. A name that module does not register is this rule's.
    let error = boot_with(options(&[]), RegistersAfterImport("mail/welcome.html"))
        .await
        .expect_err("fails")
        .to_string();
    assert!(
        error.contains(
            "embedded template `mail/welcome.html` of `AppMail` cannot be replaced by `RegistersAfterImport`, a module of the plugin `navree-plugin-announcements`"
        ),
        "{error}"
    );
    assert!(
        !error.contains("which is not under its namespace"),
        "{error}"
    );
    let error = boot_with(options(&[]), RegistersAfterImport("mail/welcome.txt"))
        .await
        .expect_err("fails")
        .to_string();
    assert!(
        error.contains(
            "registers the template `mail/welcome.txt` in `RegistersAfterImport`, which is not under its namespace"
        ),
        "{error}"
    );

    // The same names from a module of the app boot, as in 1.2.
    for name in names {
        boot_with(options(&[]), AppRegisters(name))
            .await
            .expect("boots")
            .shutdown()
            .await
            .unwrap();
    }
}

/// A hook of the app that changes the environment as `.0` says.
struct AppHook(fn(&mut Environment));
impl Module for AppHook {
    fn register(&self, m: &mut ModuleBuilder) {
        m.view_environment(self.0);
    }
}

#[tokio::test]
async fn a_plugins_environment_hook_adds_only_names_under_its_namespace() {
    let dir = views_dir(&[("notes/app.txt", "Hello from the app")]);
    let render_text = |app: &App, source: &'static str| {
        let ctx = app.ctx().clone();
        async move {
            ctx.get::<Views>()
                .await
                .unwrap()
                .render_source("probe.txt", source, json!({ "name": "agra", "cents": 500 }))
                .unwrap()
        }
    };

    // A hook that adds a filter named under the namespace boots, and the filter works.
    let app = boot_with(
        options(&[dir.path()]),
        Hook(|env| env.add_filter("announcements_badge", |text: &str| format!("[{text}]"))),
    )
    .await
    .expect("boots");
    assert_eq!(
        render_text(&app, "{{ name|announcements_badge }}").await,
        "[agra]"
    );
    app.shutdown().await.unwrap();

    // Each of these fails, naming the change.
    type Hook = fn(&mut Environment);
    let cases: [(Hook, &str); 6] = [
        (
            |env| env.add_filter("money", |cents: i64| format!("${cents}")),
            "replaces the filter `money`",
        ),
        (
            |env| env.add_filter("shout", |text: &str| text.to_uppercase()),
            "adds the filter `shout`, which does not start with `announcements_`",
        ),
        (
            |env| {
                let _ = env.add_template("mail/welcome.html", "<p>from the plugin</p>");
            },
            "adds the template `mail/welcome.html`",
        ),
        (
            |env| env.remove_template("notes/app.txt"),
            "removes the template `notes/app.txt`",
        ),
        (
            |env| env.set_auto_escape_callback(|_| AutoEscape::None),
            "changes how templates escape",
        ),
        (
            |env| env.set_loader(|_| Ok(None)),
            "changes the template loader",
        ),
    ];
    for (change, words) in cases {
        let error = boot_with(options(&[dir.path()]), Hook(change))
            .await
            .expect_err("fails")
            .to_string();
        assert!(
            error.contains(&format!(
                "plugin `navree-plugin-announcements`: m.view_environment in `Hook` {words}; a plugin's environment hook only adds filters, tests, functions and globals named `announcements_..`"
            )),
            "{words}: {error}"
        );
        // The same change from a module of the app boots and acts, as in 1.2.
        boot_with(options(&[dir.path()]), AppHook(change))
            .await
            .expect("boots")
            .shutdown()
            .await
            .unwrap();
    }
    let app = boot_with(
        options(&[dir.path()]),
        AppHook(|env| env.add_filter("shout", |text: &str| text.to_uppercase())),
    )
    .await
    .expect("boots");
    assert_eq!(render_text(&app, "{{ name|shout }}").await, "AGRA");
    app.shutdown().await.unwrap();
    let app = boot_with(
        options(&[dir.path()]),
        AppHook(|env| env.add_filter("money", |cents: i64| format!("${cents}"))),
    )
    .await
    .expect("boots");
    assert_eq!(render_text(&app, "{{ cents|money }}").await, "$500");
    app.shutdown().await.unwrap();
}

/// A module of the app that adds the filter `shout`.
struct AppShout;
impl Module for AppShout {
    fn register(&self, m: &mut ModuleBuilder) {
        m.view_environment(|env| env.add_filter("shout", |text: &str| text.to_uppercase()));
    }
}

#[tokio::test]
async fn a_plugins_environment_hook_is_checked_whatever_the_order_of_the_modules() {
    let shout: fn(&mut Environment) =
        |env| env.add_filter("shout", |text: &str| format!("{text}!"));
    // The app's hook ran first, so the plugin's replaces a filter; the other way round it adds
    // one with no prefix: either way it fails, and the app's own hook is not checked.
    for apps_first in [true, false] {
        let mut app = Navree::app().module(ViewModule::for_root(options(&[])));
        if apps_first {
            app = app.module(AppShout).module(Hook(shout));
        } else {
            app = app.module(Hook(shout)).module(AppShout);
        }
        let error = app.boot().await.expect_err("fails").to_string();
        assert!(
            error.contains("plugin `navree-plugin-announcements`: m.view_environment in `Hook` "),
            "{apps_first}: {error}"
        );
        assert!(!error.contains("in `AppShout`"), "{apps_first}: {error}");
    }
}

/// A module of the app that sets the global `company`, plain text.
struct AppCompany;
impl Module for AppCompany {
    fn register(&self, m: &mut ModuleBuilder) {
        m.view_environment(|env| env.add_global("company", "Sharma & Sons"));
    }
}

#[tokio::test]
async fn a_plugins_hook_putting_an_app_global_back_marked_safe_fails_the_boot() {
    // The same text, marked safe: equal to the plain one, but it prints as it is.
    let put_back: fn(&mut Environment) = |env| {
        env.add_global(
            "company",
            Value::from_safe_string("Sharma & Sons".to_owned()),
        )
    };
    let print_company = |app: &App| {
        let ctx = app.ctx().clone();
        async move {
            ctx.get::<Views>()
                .await
                .unwrap()
                .render_source("probe.html", "{{ company }}", json!({}))
                .unwrap()
        }
    };

    let error = Navree::app()
        .module(ViewModule::for_root(options(&[])))
        .module(AppCompany)
        .module(Hook(put_back))
        .boot()
        .await
        .expect_err("fails")
        .to_string();
    assert!(
        error.contains(
            "plugin `navree-plugin-announcements`: m.view_environment in `Hook` replaces the global `company`; a plugin's environment hook only adds filters, tests, functions and globals named `announcements_..`"
        ),
        "{error}"
    );

    // The app's own global alone prints escaped; the same put-back from a module of the app
    // boots and acts, as in 1.2.
    let app = Navree::app()
        .module(ViewModule::for_root(options(&[])))
        .module(AppCompany)
        .boot()
        .await
        .expect("boots");
    assert_eq!(print_company(&app).await, "Sharma &amp; Sons");
    app.shutdown().await.unwrap();
    let app = Navree::app()
        .module(ViewModule::for_root(options(&[])))
        .module(AppCompany)
        .module(AppHook(put_back))
        .boot()
        .await
        .expect("boots");
    assert_eq!(print_company(&app).await, "Sharma & Sons");
    app.shutdown().await.unwrap();

    // A global of its own, under its namespace, boots.
    let app = boot_with(
        options(&[]),
        Hook(|env| env.add_global("announcements_company", "Meera Sharma Ltd")),
    )
    .await
    .expect("boots");
    app.shutdown().await.unwrap();
}

mod announcements_core {
    use navree_kernel::{Module, ModuleBuilder};
    use navree_view::ModuleBuilderViewExt;

    pub struct SecondCrateViews;

    impl Module for SecondCrateViews {
        fn register(&self, m: &mut ModuleBuilder) {
            m.templates(&[("mail/outside.html", "<p>outside</p>")])
                .view_environment(|env| {
                    env.add_filter("shout", |text: &str| text.to_uppercase());
                });
        }
    }
}

mod navree_plugin_importer {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    const IMPORTER: PluginInfo = PluginInfo::new(
        "navree-plugin-importer",
        module_path!(),
        "importer",
        "1.0.0",
    );

    pub struct ImporterPlugin;

    impl Module for ImporterPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(IMPORTER)
                .import(super::announcements_core::SecondCrateViews);
        }
    }
}

#[tokio::test]
async fn the_refusal_says_why_the_module_acts_for_the_plugin() {
    // 1. Module of the plugin's own crate (`Helper`).
    let krate = navree_plugin_announcements::CRATE;
    let in_crate = Navree::app()
        .module(ViewModule::for_root(options(&[])))
        .module(AnnouncementsModule)
        .module(navree_plugin_announcements::Helper(|m| {
            m.templates(&[("mail/outside.html", "<p>outside</p>")])
                .view_environment(|env| {
                    env.add_filter("shout", |text: &str| text.to_uppercase());
                });
        }))
        .boot()
        .await
        .expect_err("fails")
        .to_string();
    assert!(
        in_crate.contains(&format!(
            "registers the template `mail/outside.html` in `Helper` (a module of the plugin's crate `{krate}`), which is not under its namespace"
        )),
        "{in_crate}"
    );
    assert!(
        in_crate.contains(&format!(
            "plugin `navree-plugin-announcements`: m.view_environment in `Helper` (a module of the plugin's crate `{krate}`) adds the filter `shout`"
        )),
        "{in_crate}"
    );

    // 2. Module of a second crate imported only by the plugin (`SecondCrateViews`).
    let by_import = Navree::app()
        .module(ViewModule::for_root(options(&[])))
        .module(navree_plugin_importer::ImporterPlugin)
        .boot()
        .await
        .expect_err("fails")
        .to_string();
    assert!(
        by_import.contains(
            "registers the template `mail/outside.html` in `SecondCrateViews` (only the plugin's modules import it), which is not under its namespace"
        ),
        "{by_import}"
    );
    assert!(
        by_import.contains(
            "plugin `navree-plugin-importer`: m.view_environment in `SecondCrateViews` (only the plugin's modules import it) adds the filter `shout`"
        ),
        "{by_import}"
    );
}
