//! `ViewModule`: `for_root`, `from_config`, global export of `Views` and
//! templates contributed with `ModuleBuilderViewExt::templates`.

use std::path::PathBuf;
use std::sync::Arc;

use navree_config::{ConfigModule, ConfigSources};
use navree_kernel::{
    App, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, ProviderKey, Resolver,
};
use navree_view::{ModuleBuilderViewExt, ViewModule, ViewOptions, Views};
use serde_json::json;

use crate::support::{options, views_dir};

/// A provider in another module that injects `Views` without importing
/// `ViewModule` (it is global).
struct Greeter {
    views: Arc<Views>,
}

impl Injectable for Greeter {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Views>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Greeter {
                views: r.get::<Views>().await?,
            })
        })
    }
}

struct GreeterModule;

impl Module for GreeterModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.provider::<Greeter>().exports::<Greeter>();
    }
}

/// A library module shipping default templates.
struct LibraryModule;

impl Module for LibraryModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[
            ("mail/welcome.html", "library welcome {{ name }}"),
            ("mail/goodbye.txt", "library goodbye"),
        ]);
    }
}

/// An app module that imports the library and overrides one template.
struct AppModule;

impl Module for AppModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(LibraryModule)
            .templates(&[("mail/welcome.html", "app welcome {{ name }}")]);
    }
}

/// A module adding a function and replacing the built-in `money` filter.
struct ShoutModule;

impl Module for ShoutModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("mail/shout.txt", "{{ shout(name) }} {{ 5|money('EUR') }}")])
            .view_environment(|env| {
                env.add_function("shout", |name: &str| format!("{}!", name.to_uppercase()));
                env.add_filter("money", |cents: i64, currency: &str| {
                    format!("{cents} {currency}")
                });
            });
    }
}

#[tokio::test]
async fn modules_add_functions_and_filters_to_the_environment() {
    let app = Navree::app()
        .module(ViewModule::for_root(options(&[])))
        .module(ShoutModule)
        .boot()
        .await
        .expect("boot");

    assert_eq!(
        views(&app)
            .await
            .render("mail/shout", json!({ "name": "Ada" }))
            .expect("render"),
        "ADA! 5 EUR"
    );
    app.shutdown().await.expect("shutdown");
}

async fn views(app: &App) -> Arc<Views> {
    app.ctx().get::<Views>().await.expect("Views")
}

#[tokio::test]
async fn for_root_exports_views_to_every_module() {
    let dir = views_dir(&[("mail/welcome.html", "Hello {{ name }}")]);
    let app = Navree::app()
        .module(ViewModule::for_root(options(&[dir.path()])))
        .module(GreeterModule)
        .boot()
        .await
        .expect("boot");

    let greeter = app.ctx().get::<Greeter>().await.expect("Greeter");

    assert_eq!(
        greeter
            .views
            .render("mail/welcome", json!({ "name": "Ada" }))
            .expect("render"),
        "Hello Ada"
    );
    app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn modules_contribute_embedded_templates() {
    let app = Navree::app()
        .module(ViewModule::for_root(options(&[])))
        .module(LibraryModule)
        .boot()
        .await
        .expect("boot");

    let views = views(&app).await;

    assert_eq!(
        views
            .render("mail/welcome", json!({ "name": "Ada" }))
            .expect("render"),
        "library welcome Ada"
    );
    app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn importing_module_overrides_imported_modules_templates() {
    let app = Navree::app()
        .module(ViewModule::for_root(options(&[])))
        .module(AppModule)
        .boot()
        .await
        .expect("boot");

    let views = views(&app).await;

    assert_eq!(
        views
            .render("mail/welcome", json!({ "name": "Ada" }))
            .expect("welcome"),
        "app welcome Ada"
    );
    assert_eq!(
        views.render("mail/goodbye", json!({})).expect("goodbye"),
        "library goodbye"
    );
    app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn files_override_contributed_templates() {
    let dir = views_dir(&[("mail/welcome.html", "file welcome")]);
    let app = Navree::app()
        .module(ViewModule::for_root(options(&[dir.path()])))
        .module(LibraryModule)
        .boot()
        .await
        .expect("boot");

    let views = views(&app).await;

    assert_eq!(
        views.render("mail/welcome", json!({})).expect("render"),
        "file welcome"
    );
    app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn template_syntax_error_fails_boot_naming_template() {
    let dir = views_dir(&[("mail/broken.html", "{% for %}")]);

    let errors = Navree::app()
        .module(ViewModule::for_root(options(&[dir.path()])))
        .boot()
        .await
        .expect_err("boot must fail");

    let message = errors.to_string();
    assert!(message.contains("mail/broken.html"), "{message}");
}

#[tokio::test]
async fn from_config_reads_view_section() {
    let dir = views_dir(&[("mail/welcome.txt", "configured {{ name }}")]);
    let app = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().set("view", json!({ "dirs": [dir.path()], "reload": true })),
        ))
        .module(ViewModule::from_config())
        .boot()
        .await
        .expect("boot");

    let views = views(&app).await;

    assert_eq!(
        views
            .render("mail/welcome", json!({ "name": "Ada" }))
            .expect("render"),
        "configured Ada"
    );
    assert!(format!("{views:?}").contains("reload: true"), "{views:?}");
    app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn plain_view_module_reads_config_and_accepts_comma_separated_dirs() {
    let first = views_dir(&[("a.txt", "first a")]);
    let second = views_dir(&[("a.txt", "second a"), ("b.txt", "second b")]);
    let dirs = format!("{},{}", first.path().display(), second.path().display());
    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_VIEW__DIRS", dirs.as_str()),
                ("APP_VIEW__RELOAD", "false"),
            ],
        )))
        .module(ViewModule)
        .boot()
        .await
        .expect("boot");

    let views = views(&app).await;

    assert_eq!(views.render("a", json!({})).expect("a"), "first a");
    assert_eq!(views.render("b", json!({})).expect("b"), "second b");
    app.shutdown().await.expect("shutdown");
}

#[test]
fn view_options_defaults() {
    let defaults = ViewOptions::default();

    assert_eq!(defaults.dirs, vec![PathBuf::from("resources/views")]);
    assert_eq!(defaults.reload, cfg!(debug_assertions));
}

#[tokio::test]
async fn for_root_accepts_a_directory() {
    let dir = views_dir(&[("hello.txt", "hello")]);
    let app = Navree::app()
        .module(ViewModule::for_root(dir.path()))
        .boot()
        .await
        .expect("boot");

    let views = views(&app).await;

    assert_eq!(views.render("hello", json!({})).expect("render"), "hello");
    app.shutdown().await.expect("shutdown");
}
