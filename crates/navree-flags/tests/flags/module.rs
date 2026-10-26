//! `FlagsModule`: configuration, registration and boot errors.

use navree_config::{Config, ConfigSources};
use navree_flags::{
    FlagRegistry, FlagScope, FlagStoreKind, Flags, FlagsModule, FlagsOptions, MissingScope,
};
use navree_macros::flag;
use navree_testing::TestApp;

use crate::support::{DealExport, NewPipeline};

#[test]
fn options_come_from_the_flags_section_and_the_environment() {
    let config = Config::load(&ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_FLAGS__STORE", "memory"),
            ("APP_FLAGS__CACHE_TTL_SECS", "10"),
            ("APP_FLAGS__ROUTES", "false"),
            ("APP_FLAGS__MISSING_SCOPE", "off"),
        ],
    ))
    .unwrap();
    let options: FlagsOptions = config.typed().unwrap();
    assert_eq!(
        options,
        FlagsOptions {
            store: FlagStoreKind::Memory,
            cache_ttl_secs: 10,
            routes: false,
            admin_routes: true,
            missing_scope: MissingScope::Off,
        }
    );
    let defaults: FlagsOptions = Config::load(&ConfigSources::new())
        .unwrap()
        .typed()
        .unwrap();
    assert_eq!(defaults, FlagsOptions::default());
}

#[tokio::test]
async fn for_root_takes_the_options_in_code() {
    let app = TestApp::builder()
        .module(FlagsModule::for_feature::<DealExport>())
        .module(FlagsModule::for_root(FlagsOptions {
            store: FlagStoreKind::Memory,
            routes: false,
            admin_routes: false,
            ..FlagsOptions::default()
        }))
        .build()
        .await;
    let flags = app.get::<Flags>().await;
    assert!(
        flags
            .active::<DealExport>(FlagScope::everyone())
            .await
            .unwrap()
    );
    assert_eq!(app.get::<FlagsOptions>().await.store, FlagStoreKind::Memory);
}

mod other {
    use navree_macros::flag;

    /// Another flag that took the name `new-pipeline`.
    #[flag(name = "new-pipeline")]
    pub struct Board;
}

#[tokio::test]
async fn two_flags_with_one_name_fail_the_boot() {
    let error = TestApp::builder()
        .config("flags.store", "memory")
        .config("flags.routes", false)
        .config("flags.admin_routes", false)
        .module(FlagsModule::for_feature::<NewPipeline>())
        .module(FlagsModule::for_feature::<other::Board>())
        .module(FlagsModule::from_config())
        .try_build()
        .await
        .expect_err("the boot fails");
    let message = error.to_string();
    assert!(
        message.contains("two flags are named `new-pipeline`")
            && message.contains("NewPipeline")
            && message.contains("other::Board"),
        "{message}"
    );
}

/// The registry lists the flags of every module, each once.
#[tokio::test]
async fn registrations_from_several_modules_are_collected() {
    #[flag(expose)]
    struct Extra;

    let app = TestApp::builder()
        .config("flags.store", "memory")
        .config("flags.routes", false)
        .config("flags.admin_routes", false)
        .module(FlagsModule::for_feature::<NewPipeline>())
        .module(FlagsModule::for_feature::<Extra>())
        .module(FlagsModule::for_feature::<NewPipeline>())
        .module(FlagsModule::from_config())
        .build()
        .await;
    let registry = app.get::<FlagRegistry>().await;
    let names: Vec<&str> = registry.iter().map(|d| d.name).collect();
    assert_eq!(names, ["extra", "new-pipeline"]);
    assert!(registry.get("extra").unwrap().expose);
}

#[tokio::test]
async fn the_module_builder_extension_registers_flags() {
    use navree_flags::ModuleBuilderFlagsExt;
    use navree_kernel::{Module, ModuleBuilder};

    struct Owner;
    impl Module for Owner {
        fn register(&self, m: &mut ModuleBuilder) {
            m.flag::<DealExport>();
        }
    }

    let app = TestApp::builder()
        .config("flags.store", "memory")
        .config("flags.routes", false)
        .config("flags.admin_routes", false)
        .module(Owner)
        .module(FlagsModule)
        .build()
        .await;
    assert!(app.get::<FlagRegistry>().await.contains("deal-export"));
}
