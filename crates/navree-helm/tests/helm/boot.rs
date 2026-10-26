//! Mistakes in options, resources and pages fail the boot with a message
//! that names them.

use navree_access::AccessModule;
use navree_helm::{
    Action, Field, HelmModule, HelmOptions, HelmPage, Metric, ModuleBuilderHelmExt, Resource,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::TestApp;

use crate::support::*;

/// The error of booting an app with `module` next to the CRM.
async fn boot_error(module: impl Module) -> String {
    match builder().module(module).try_build().await {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    }
}

macro_rules! resource {
    ($name:ident, $slug:literal, fields: $fields:expr $(, $($item:tt)*)?) => {
        struct $name;

        impl Injectable for $name {
            fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
                Box::pin(async { Ok($name) })
            }
        }

        impl Resource for $name {
            type Entity = deal::Entity;
            const LABEL: &'static str = "Broken";
            const SLUG: &'static str = $slug;
            fn fields(&self) -> Vec<Field> {
                $fields
            }
            $($($item)*)?
        }
    };
}

struct With<R>(std::marker::PhantomData<R>);

impl<R: Resource + Injectable> Module for With<R> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.resource::<R>();
    }
}

fn with<R: Resource + Injectable>() -> With<R> {
    With(std::marker::PhantomData)
}

resource!(UnknownColumn, "unknown-column", fields: vec![Field::text("colour")]);
resource!(BadPattern, "bad-pattern", fields: vec![Field::text("title").pattern("(")]);
resource!(WrongModel, "wrong-model", fields: vec![Field::computed("x", |c: &company::Model| c.name.clone())]);
resource!(NotRegistered, "not-registered", fields: vec![Field::belongs_to::<OtherResource>("company_id")]);
resource!(OtherResource, "other", fields: vec![Field::text("title")]);
resource!(Duplicate, "deals", fields: vec![Field::text("title")]);
resource!(
    NoHandler,
    "no-handler",
    fields: vec![Field::text("title")],
    fn actions(&self) -> Vec<Action<deal::Entity>> {
        vec![Action::new("noop", "Do nothing")]
    }
);
resource!(
    NoBuckets,
    "no-buckets",
    fields: vec![Field::text("title")],
    fn metrics(&self) -> Vec<Metric<deal::Entity>> {
        vec![Metric::trend("Deals").count()]
    }
);
resource!(
    BadTitle,
    "bad-title",
    fields: vec![Field::text("title")],
    const TITLE: &'static str = "name";
);

#[tokio::test]
async fn resource_mistakes_name_the_resource_and_the_problem() {
    let cases: Vec<(String, &str)> = vec![
        (
            boot_error(with::<UnknownColumn>()).await,
            "field `colour` is not a column of `deals`",
        ),
        (
            boot_error(with::<BadPattern>()).await,
            "the pattern of field `title` is invalid",
        ),
        (
            boot_error(with::<WrongModel>()).await,
            "computed field `x` takes",
        ),
        (
            boot_error(with::<NotRegistered>()).await,
            "which is not registered; add `m.resource::<OtherResource>()`",
        ),
        (
            boot_error(with::<Duplicate>()).await,
            "both use the slug `deals`",
        ),
        (
            boot_error(with::<NoHandler>()).await,
            "action `noop` has no handler",
        ),
        (boot_error(with::<NoBuckets>()).await, "needs `.by_day(..)`"),
        (
            boot_error(with::<BadTitle>()).await,
            "TITLE `name` is not a column of `deals`",
        ),
    ];
    for (error, expected) in cases {
        assert!(
            error.contains(expected),
            "expected `{expected}` in:\n{error}"
        );
    }
}

struct PageWithoutTemplate;

impl Module for PageWithoutTemplate {
    fn register(&self, m: &mut ModuleBuilder) {
        m.helm_page(HelmPage::new("report", "Report"));
    }
}

struct PageWithTemplate;

impl Module for PageWithTemplate {
    fn register(&self, m: &mut ModuleBuilder) {
        m.helm_page(HelmPage::new("report", "Report").template("admin/report"));
    }
}

#[tokio::test]
async fn page_mistakes_fail_the_boot() {
    let error = boot_error(PageWithoutTemplate).await;
    assert!(
        error.contains("Helm page `report` has no template"),
        "{error}"
    );
    let error = boot_error(PageWithTemplate).await;
    assert!(error.contains("so the app needs ViewModule"), "{error}");
}

#[tokio::test]
async fn invalid_options_fail_the_boot() {
    let error = match builder().config("helm.accent", "green").try_build().await {
        Ok(_) => panic!("booted"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("helm.accent must be a hex color"), "{error}");

    let options = HelmOptions {
        per_page: 0,
        ..HelmOptions::default()
    };
    let error = match TestApp::builder()
        .temp_database()
        .module(navree_db::DatabaseModule)
        .module(AccessModule::for_root(options_access()))
        .module(HelmModule::for_root(options))
        .try_build()
        .await
    {
        Ok(_) => panic!("booted"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("helm.per_page must be between 1 and 100"),
        "{error}"
    );
}

fn options_access() -> navree_access::AccessOptions {
    options()
}

#[tokio::test]
async fn for_root_takes_options_without_config() {
    let app = TestApp::builder()
        .temp_database()
        .module(navree_db::DatabaseModule)
        .module(AccessModule::for_root(options()))
        .module(CrmModule)
        .module(HelmModule::for_root(HelmOptions {
            path: "/ops".into(),
            title: "Ops".into(),
            ..HelmOptions::default()
        }))
        .build()
        .await;
    let (_, rep) = user(&app, "rep").await;
    rep.get("/ops/api/resources")
        .send()
        .await
        .assert_status(200);
    let options = app.get::<HelmOptions>().await;
    assert_eq!(options.title, "Ops");
}
