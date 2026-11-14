//! Templates a plugin ships: registered under its namespace, beaten by a file
//! in a template folder, publishable, and checked when the app boots.

#[path = "view/support.rs"]
mod support;

use navree_kernel::{Module, ModuleBuilder, Navree, PluginInfo, PublishTag};
use navree_view::{ModuleBuilderPluginViewsExt, TemplateDir, ViewModule, Views};
use serde_json::json;

use crate::support::{options, views_dir};

fn announcements() -> PluginInfo {
    PluginInfo::new(
        "navree-plugin-announcements",
        env!("CARGO_CRATE_NAME"),
        "announcements",
        "1.0.0",
    )
}

/// The plugin: `m.plugin_templates` before `m.plugin`, from the fixtures folder.
struct AnnouncementsModule;
impl Module for AnnouncementsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin_templates(navree_macros::embed_templates!(
            "tests/fixtures/plugin-views"
        ))
        .plugin(announcements());
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
            m.plugin(announcements())
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
