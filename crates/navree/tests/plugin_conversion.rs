//! What an app sees when the framework's own packages name themselves plugins.

use std::fs;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use navree::BoxFuture;
use navree::audit::{AuditModule, AuditOptions};
use navree::config::ConfigSources;
use navree::contracts::AuditSink;
use navree::crypto::generate_key;
use navree::flags::{FlagStoreKind, Flags, FlagsModule, FlagsOptions, flag};
use navree::i18n::{I18nModule, Locale};
use navree::kernel::__private::{acts_for, is_app_module};
use navree::kernel::{KernelError, ModuleId};
use navree::plugin::Permission;
use navree::plugin::prelude::*;
use navree::prelude::*;
use navree::realtime::{RealtimeModule, RealtimeOptions};
use navree::redis::{RedisModule, RedisOptions};
use navree::social::{
    AuthorizeUrl, ProviderConfig, ProviderToken, Social, SocialError, SocialModule, SocialOptions,
    SocialProvider, SocialUser,
};
use navree::testing::TestApp;
use navree::ui::ShellNavItem;
use serde_json::{Value, json};

/// Every warning or error logged on this thread while the guard lives (the tests that use it
/// run on a current-thread runtime, so the boot logs on this thread).
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

impl Warnings {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(self.clone())
    }

    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct Message(String);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl tracing::Subscriber for Warnings {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() <= tracing::Level::WARN
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() <= tracing::Level::WARN {
            let mut message = Message(String::new());
            event.record(&mut message);
            self.0.lock().unwrap().push(message.0);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// An exposed flag so `GET /api/flags` returns its value.
#[flag(name = "new-pipeline", default = true, expose)]
pub struct NewPipeline;

/// A custom social provider registered via `SocialModule::for_feature`.
#[injectable]
pub struct ZohoProvider;

impl SocialProvider for ZohoProvider {
    fn key(&self) -> &'static str {
        "zoho"
    }

    fn authorize_url(&self, _config: &ProviderConfig, url: AuthorizeUrl) -> AuthorizeUrl {
        url.endpoint("https://accounts.zoho.in/oauth/v2/auth")
            .scopes(["AaaServer.profile.Read"])
    }

    fn user<'a>(
        &'a self,
        _config: &'a ProviderConfig,
        token: ProviderToken,
    ) -> BoxFuture<'a, std::result::Result<SocialUser, SocialError>> {
        Box::pin(async move {
            Ok(SocialUser::new("zoho", "harpreet-1")
                .name("Harpreet Sandhu")
                .email(Some("harpreet@ludhiana.example"), true)
                .token(token))
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_that_uses_flags_and_social_boots_with_no_warning_and_lists_them_first_party() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let (social, fake) = Social::fake();
    fake.on_code(
        "github",
        "code-meera",
        SocialUser::new("github", "meera-1")
            .name("Meera Sharma")
            .email(Some("meera@ludhiana.example"), true),
    );

    let app = TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config(
            "social.redirect_base",
            "https://kesar.ludhiana.example/auth/callback",
        )
        .config("social.providers.github.client_id", "gh-client")
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(AuthModule::from_config())
        .module(FlagsModule::for_feature::<NewPipeline>())
        .module(FlagsModule::from_config())
        .module(SocialModule::from_config())
        .override_value(Arc::new(social))
        .build()
        .await;

    let logged = warnings.all();
    assert!(logged.is_empty(), "{logged:?}");

    let report = app.ctx().plugins();
    for (package, namespace, module_name) in [
        ("navree-flags", "flags", "FlagsModule"),
        ("navree-social", "social", "SocialModule"),
    ] {
        let plugin = report
            .plugins
            .iter()
            .find(|plugin| plugin.info.name == package)
            .unwrap_or_else(|| panic!("missing `{package}` in {report}"));
        assert!(plugin.first_party, "{plugin:?}");
        assert_eq!(plugin.info.namespace, namespace);
        assert!(
            plugin
                .modules
                .iter()
                .any(|module| module.id.name() == module_name),
            "{plugin:?}"
        );
    }

    let text = report.to_string();
    assert!(
        text.contains("flags") && text.contains("navree-flags"),
        "{text}"
    );
    assert!(
        text.contains("social") && text.contains("navree-social"),
        "{text}"
    );

    // Both packages answer on their documented routes as before.
    app.client()
        .get("/api/flags")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("flags.new-pipeline", json!(true));

    let redirect = app
        .client()
        .get("/api/auth/social/github/redirect")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(redirect["state"].is_string(), "{redirect}");

    app.shutdown().await.unwrap();
}

/// An app module that imports both feature helpers.
struct DealsAndAccounts;

impl Module for DealsAndAccounts {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(FlagsModule::for_feature::<NewPipeline>())
            .import(SocialModule::for_feature::<ZohoProvider>());
    }
}

#[tokio::test]
async fn a_module_that_imports_for_feature_stays_the_apps_own() {
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config(
            "social.redirect_base",
            "https://kesar.ludhiana.example/auth/callback",
        )
        .config(
            "social.providers.zoho",
            json!({
                "type": "custom",
                "name": "Zoho",
                "client_id": "1000.ABC",
                "token_url": "https://accounts.zoho.in/oauth/v2/token",
            }),
        )
        .module(CryptoModule::from_config())
        .module(HttpClientModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(AuthModule::from_config())
        .module(FlagsModule::from_config())
        .module(SocialModule::from_config())
        .module(DealsAndAccounts)
        .build()
        .await;

    let ctx = app.ctx();
    let app_module = ModuleId::of::<DealsAndAccounts>();
    let flag_feature = FlagsModule::for_feature::<NewPipeline>().id();
    let social_feature = SocialModule::for_feature::<ZohoProvider>().id();

    assert!(is_app_module(ctx, app_module));
    assert!(acts_for(ctx, app_module).is_none());
    assert_eq!(
        acts_for(ctx, flag_feature).map(|info| info.name),
        Some("navree-flags")
    );
    assert_eq!(
        acts_for(ctx, social_feature).map(|info| info.name),
        Some("navree-social")
    );

    app.shutdown().await.unwrap();
}

/// An app module that describes `flags.manage` itself, and another that adds a `/flags` link.
struct AppFlagsPermissions;

impl Module for AppFlagsPermissions {
    fn register(&self, m: &mut ModuleBuilder) {
        m.permissions(&[Permission::new("flags.manage", "Manage feature flags")]);
    }
}

struct AppFlagsNavigation;

impl Module for AppFlagsNavigation {
    fn register(&self, m: &mut ModuleBuilder) {
        m.nav_item(ShellNavItem::new("Flags", "/flags").permission("flags.manage"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_that_keeps_its_own_flags_configuration_and_names_boots_as_before() {
    let warnings = Warnings::default();
    let _guard = warnings.install();

    let sources = ConfigSources::new()
        .set("http.per_ip_rate_limit", Value::Null)
        .set(
            "flags",
            json!({
                "store": "memory",
                "cache_ttl_secs": 15,
                "routes": true,
                "admin_routes": true,
            }),
        );

    let builder = || {
        Navree::app()
            .module(ConfigModule::for_root(sources.clone()))
            .module(HttpModule::from_config())
            .module(DatabaseModule::from_config())
            .module(UiModule::from_config())
            .module(AccessModule::for_root(AccessOptions::default()))
            .module(FlagsModule::for_feature::<NewPipeline>())
            .module(FlagsModule::from_config())
            .module(AppFlagsPermissions)
            .module(AppFlagsNavigation)
    };

    let app = builder().boot().await.expect("boots cleanly");
    let logged = warnings.all();
    assert!(logged.is_empty(), "{logged:?}");

    let flags = app.ctx().get::<Flags>().await.expect("Flags");
    assert!(
        flags
            .active::<NewPipeline>(navree::flags::FlagScope::everyone())
            .await
            .unwrap()
    );
    app.shutdown().await.unwrap();

    builder()
        .run_with_args(vec!["flags:list".to_owned()])
        .await
        .expect("flags:list runs as before");
}

static CWD_LOCK: Mutex<()> = Mutex::new(());

struct RestoreCurrentDir;

impl Drop for RestoreCurrentDir {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"));
    }
}

fn in_temp_app_folder(f: impl FnOnce(&Path)) {
    let _lock = CWD_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let folder = tempfile::tempdir().expect("a temporary app folder");
    let _restore = RestoreCurrentDir;
    std::env::set_current_dir(folder.path()).expect("current directory");
    f(folder.path());
}

#[test]
fn config_publish_plugin_writes_the_section_of_each_converted_package() {
    in_temp_app_folder(|root| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let run = |namespace: &str| {
            runtime.block_on(
                Navree::app()
                    .module(ConfigModule::for_root(ConfigSources::new()))
                    .module(CryptoModule::from_config())
                    .module(HttpModule::from_config())
                    .module(DatabaseModule::from_config())
                    .module(AuthModule::from_config())
                    .module(FlagsModule::from_config())
                    .module(SocialModule::from_config())
                    .run_with_args(vec![
                        "config:publish".to_owned(),
                        "--plugin".to_owned(),
                        namespace.to_owned(),
                    ]),
            )
        };

        run("flags").expect("config:publish --plugin flags");
        run("social").expect("config:publish --plugin social");

        let flags_file = root.join("config/flags.toml");
        let social_file = root.join("config/social.toml");
        assert!(flags_file.is_file());
        assert!(social_file.is_file());
        assert!(
            fs::read_to_string(&flags_file)
                .unwrap()
                .contains("cache_ttl_secs")
        );
        assert!(
            fs::read_to_string(&social_file)
                .unwrap()
                .contains("redirect_base")
        );

        // A second run keeps an edited file.
        fs::write(&flags_file, "# Harpreet Sandhu's settings\n").unwrap();
        run("flags").expect("second config:publish --plugin flags");
        assert_eq!(
            fs::read_to_string(&flags_file).unwrap(),
            "# Harpreet Sandhu's settings\n"
        );
    });
}

mod community_plugins {
    use navree::plugin::PluginInfo;
    use navree::prelude::*;

    pub struct CommunityFlags;

    impl Module for CommunityFlags {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PluginInfo::new(
                "navree-plugin-flags",
                module_path!(),
                "flags",
                "0.1.0",
            ));
        }
    }

    pub struct CommunityLedger;

    impl Module for CommunityLedger {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PluginInfo::new(
                "navree-plugin-ledger",
                module_path!(),
                "ledger",
                "0.1.0",
            ));
        }
    }
}

#[tokio::test]
async fn a_community_plugin_with_the_namespace_flags_fails_the_boot() {
    let errors = Navree::app()
        .module(community_plugins::CommunityFlags)
        .validate()
        .expect_err("reserved namespace must fail");
    assert!(
        errors.0.iter().any(|error| matches!(
            error,
            KernelError::NamespaceReserved {
                plugin: "navree-plugin-flags",
                namespace: "flags",
            }
        )),
        "{errors}"
    );

    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(FlagsModule::for_root(FlagsOptions {
            store: FlagStoreKind::Memory,
            routes: false,
            admin_routes: false,
            ..FlagsOptions::default()
        }))
        .module(community_plugins::CommunityLedger)
        .build()
        .await;
    let plugins = app.ctx().plugins().plugins;
    assert!(
        plugins
            .iter()
            .any(|p| p.info.name == "navree-flags" && p.first_party)
    );
    assert!(
        plugins
            .iter()
            .any(|p| p.info.name == "navree-plugin-ledger" && !p.first_party)
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_that_configures_a_package_in_code_boots_and_lists_it_first_party() {
    let warnings = Warnings::default();
    let _guard = warnings.install();

    let mut social_options = SocialOptions::default();
    social_options.redirect_base = "https://kesar.ludhiana.example/auth/callback".to_owned();
    social_options.routes = false;

    let app = TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(FlagsModule::for_root(FlagsOptions {
            store: FlagStoreKind::Memory,
            routes: false,
            admin_routes: false,
            ..FlagsOptions::default()
        }))
        .module(SocialModule::for_root(social_options))
        .build()
        .await;

    let logged = warnings.all();
    assert!(logged.is_empty(), "{logged:?}");

    let report = app.ctx().plugins();
    for (name, namespace) in [("navree-flags", "flags"), ("navree-social", "social")] {
        let plugin = report
            .plugins
            .iter()
            .find(|plugin| plugin.info.name == name)
            .unwrap_or_else(|| panic!("missing `{name}` in {report}"));
        assert!(plugin.first_party, "{plugin:?}");
        assert_eq!(plugin.info.namespace, namespace);
    }

    app.shutdown().await.unwrap();
}

/// A module of this test crate whose `id()` names a type inside `navree_flags`.
struct DisguisedAsFlags;

impl Module for DisguisedAsFlags {
    fn id(&self) -> ModuleId {
        ModuleId::of::<FlagsOptions>()
    }

    fn register(&self, _m: &mut ModuleBuilder) {}
}

/// Another module of this test crate whose `id()` names `navree_flags`, imported only by a
/// community plugin and registering a framework command.
struct DisguisedHelper;

impl Module for DisguisedHelper {
    fn id(&self) -> ModuleId {
        ModuleId::of::<Flags>()
    }

    fn register(&self, m: &mut ModuleBuilder) {
        m.command(navree::kernel::CommandSpec::new(
            "check",
            "Taken command name",
            |_, _| async { Ok(()) },
        ));
    }
}

mod disguised_importer {
    use navree::plugin::PluginInfo;
    use navree::prelude::*;

    use super::DisguisedHelper;

    pub struct KhataPlugin;

    impl Module for KhataPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PluginInfo::new(
                "navree-plugin-khata",
                module_path!(),
                "khata",
                "0.1.0",
            ))
            .import(DisguisedHelper);
        }
    }
}

#[tokio::test]
async fn a_module_of_another_crate_whose_id_names_a_first_party_crate_does_not_act_for_that_plugin()
{
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(FlagsModule::for_root(FlagsOptions {
            store: FlagStoreKind::Memory,
            routes: false,
            admin_routes: false,
            ..FlagsOptions::default()
        }))
        .module(DisguisedAsFlags)
        .build()
        .await;

    let ctx = app.ctx();
    let disguised = DisguisedAsFlags.id();
    assert!(is_app_module(ctx, disguised));
    assert!(acts_for(ctx, disguised).is_none());

    let flags = ctx
        .plugins()
        .plugins
        .into_iter()
        .find(|plugin| plugin.info.name == "navree-flags")
        .expect("navree-flags");
    assert_eq!(flags.modules.len(), 1);
    assert_eq!(flags.modules[0].id, ModuleId::of::<FlagsModule>());
    app.shutdown().await.unwrap();

    // Imported only by a community plugin beside `FlagsModule`, it is never shielded as
    // `navree-flags` by its id: it acts for the community plugin and is held to its rules.
    let errors = Navree::app()
        .module(FlagsModule::for_root(FlagsOptions {
            store: FlagStoreKind::Memory,
            routes: false,
            admin_routes: false,
            ..FlagsOptions::default()
        }))
        .module(disguised_importer::KhataPlugin)
        .validate()
        .expect_err("framework command name in community helper must be refused");
    assert!(
        errors.0.iter().any(|error| matches!(
            error,
            KernelError::PluginCommand {
                plugin: "navree-plugin-khata",
                command: "check",
                ..
            }
        )),
        "{errors}"
    );
}

#[injectable]
struct GreetingController;

#[controller("/greeting")]
impl GreetingController {
    #[get("/")]
    #[public]
    async fn show(&self, locale: Locale) -> String {
        locale.to_string()
    }
}

#[module(controllers = [GreetingController])]
struct GreetingModule;

#[tokio::test(flavor = "current_thread")]
async fn the_localization_packages_app_wide_layer_is_not_refused_once_it_names_itself_a_plugin() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let locales_dir = tempfile::tempdir().expect("locales dir");
    let shown = locales_dir.path().to_string_lossy().replace('\\', "/");

    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("i18n.dir", shown.as_str())
        .config("i18n.supported", json!(["en", "pa-IN"]))
        .module(I18nModule::from_config())
        .module(GreetingModule)
        .build()
        .await;

    let logged = warnings.all();
    assert!(logged.is_empty(), "{logged:?}");

    let plugins = app.ctx().plugins().plugins;
    let i18n = plugins
        .iter()
        .find(|plugin| plugin.info.name == "navree-i18n")
        .expect("navree-i18n listed");
    assert!(i18n.first_party);

    let reply = app
        .client()
        .get("/api/greeting")
        .header("accept-language", "pa-IN")
        .send()
        .await
        .assert_status(200);
    assert_eq!(reply.header("content-language"), Some("pa-IN"));
    assert_eq!(reply.text(), "pa-IN");

    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn the_audit_packages_sink_and_the_real_time_packages_backplane_are_not_refused_once_they_name_themselves_plugins()
 {
    let warnings = Warnings::default();
    let _guard = warnings.install();

    let app = TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(DatabaseModule::from_config())
        .module(AuditModule::for_root(AuditOptions {
            routes: false,
            ..AuditOptions::default()
        }))
        .module(RedisModule::for_root(RedisOptions::default()))
        .module(RealtimeModule::for_root(RealtimeOptions::default()))
        .build()
        .await;

    let logged = warnings.all();
    assert!(logged.is_empty(), "{logged:?}");

    let report = app.ctx().plugins();
    for expected in ["navree-audit", "navree-redis", "navree-realtime"] {
        let plugin = report
            .plugins
            .iter()
            .find(|plugin| plugin.info.name == expected)
            .unwrap_or_else(|| panic!("missing `{expected}` in {report}"));
        assert!(plugin.first_party, "{plugin:?}");
    }
    assert!(app.ctx().try_get::<dyn AuditSink>().await.is_some());

    app.shutdown().await.unwrap();
}
