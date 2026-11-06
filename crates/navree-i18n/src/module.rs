//! [`I18nModule`].

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use navree_config::ModuleBuilderConfigExt;
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::{GlobalLayer, LayerSlot, ModuleBuilderHttpExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Lifecycle, Module, ModuleBuilder, ModuleId,
    ProviderKey, Resolver,
};
use navree_notify::RecipientScope;
use navree_validation::MessageTranslator;
use navree_view::ModuleBuilderViewExt;

use crate::catalog;
use crate::commands::{self, commands};
use crate::controller::I18nController;
use crate::error::map_i18n_error;
use crate::locale::{clear_global, set_global};
use crate::migration::migrations;
use crate::negotiate;
use crate::notify::UserLocaleScope;
use crate::validation::Translator;
use crate::views::{self, Slot};
use crate::{I18n, I18nError, I18nOptions};

/// How often `i18n.reload` looks for changed files.
const RELOAD_EVERY: Duration = Duration::from_millis(500);

/// Localization.
///
/// Global; provides and exports [`I18n`] and [`I18nOptions`], and
/// contributes:
///
/// - the request layer (slot `Context`) that negotiates each request's
///   locale from the sources of `i18n.negotiation`, makes it current for
///   the handler (the [`Locale`](crate::Locale) extractor, [`t!`](crate::t))
///   and adds `Content-Language` to the response;
/// - a `dyn MessageTranslator`: navree-validation's messages in the
///   request's locale;
/// - a `dyn RecipientScope`: notifications rendered in the recipient's
///   saved locale;
/// - the `t` and `locale` template functions, and `money` and `date`
///   filters that format for the current locale (navree-view);
/// - the `i18n` migrations (`locale_preferences`, used with
///   `DatabaseModule`);
/// - `GET {api_prefix}/i18n/locales` and, with `DatabaseModule` and an
///   authenticator, `PUT {api_prefix}/me/locale` (unless `i18n.routes` is
///   false);
/// - the commands `i18n:check`, `i18n:missing` and `i18n:publish`.
///
/// Put it before `ValidationModule`, `ViewModule` and `MailModule`. The
/// app must include `ConfigModule` (for
/// [`from_config`](I18nModule::from_config) and the plain module).
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_http::HttpModule;
/// use navree_i18n::I18nModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(I18nModule::from_config()) // section `i18n`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct I18nModule;

impl I18nModule {
    /// The module with explicit options (configure it once, in the app
    /// root), for example with Fluent files embedded by
    /// [`embed_locales!`](crate::embed_locales).
    pub fn for_root(options: I18nOptions) -> impl Module {
        ConfiguredI18nModule {
            options: Some(options),
        }
    }

    /// The module configured from the `i18n` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredI18nModule { options: None }
    }
}

impl Module for I18nModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `I18nModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredI18nModule {
    /// `None`: read from configuration.
    options: Option<I18nOptions>,
}

impl Module for ConfiguredI18nModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<I18nModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<I18nOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<I18nOptions>::new(move |_| {
            let options = options.clone();
            async move { Ok(Arc::new(options)) }
        })),
        None => m.config::<I18nOptions>(),
    };
    // The app's I18n, for the request layer and the template functions,
    // which are registered before it exists.
    let slot: Slot = Arc::new(OnceLock::new());
    let built = slot.clone();
    m.exports::<I18nOptions>()
        .factory(
            Factory::<I18n>::new(move |r: Resolver| {
                let slot = built.clone();
                async move {
                    let options = r.get::<I18nOptions>().await?;
                    let app = r.app();
                    let lenient = app
                        .command()
                        .is_some_and(|command| commands::NAMES.contains(&command.name));
                    let db = app.try_get::<Db>().await;
                    // Reads files: keep it off the async workers.
                    let i18n =
                        tokio::task::spawn_blocking(move || I18n::load(options, db, lenient))
                            .await??;
                    let i18n = Arc::new(i18n);
                    let _ = slot.set(i18n.clone());
                    Ok(i18n)
                }
            })
            .deps(vec![ProviderKey::of::<I18nOptions>()]),
        )
        .exports::<I18n>()
        .lifecycle::<I18n>()
        .factory(
            Factory::<dyn MessageTranslator>::new(|r: Resolver| async move {
                let i18n = r.get::<I18n>().await?;
                Ok(Arc::new(Translator { i18n }) as Arc<dyn MessageTranslator>)
            })
            .deps(vec![ProviderKey::of::<I18n>()]),
        )
        .exports::<dyn MessageTranslator>()
        .factory(
            Factory::<dyn RecipientScope>::new(|r: Resolver| async move {
                let i18n = r.get::<I18n>().await?;
                Ok(Arc::new(UserLocaleScope { i18n }) as Arc<dyn RecipientScope>)
            })
            .deps(vec![ProviderKey::of::<I18n>()]),
        )
        .exports::<dyn RecipientScope>()
        .controller::<I18nController>()
        .migrations(migrations())
        .map_error::<I18nError>(map_i18n_error);
    let for_layer = slot.clone();
    m.http_layer(GlobalLayer::new(
        LayerSlot::Context,
        "i18n.locale",
        move |_ctx, router| {
            let slot = for_layer.clone();
            Ok(router.layer(navree_http::middleware::from_fn_with_state(
                slot,
                negotiate::layer,
            )))
        },
    ));
    m.view_environment(move |env| views::configure(env, &slot));
    for command in commands() {
        m.command(command);
    }
    #[cfg(feature = "ui")]
    m.factory(Factory::<dyn navree_ui::Localizer>::new(|_| async {
        Ok(Arc::new(crate::ui::UiLocalizer) as Arc<dyn navree_ui::Localizer>)
    }))
    .exports::<dyn navree_ui::Localizer>();
}

impl Lifecycle for I18n {
    /// Makes this app's locales the ones code outside any request uses,
    /// and watches the files when `i18n.reload` is on.
    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            set_global(&self.shared);
            let options = self.options();
            if options.reload && options.files.embedded().is_none() {
                watch(self, &ctx);
            }
            Ok(())
        })
    }

    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            clear_global(&self.shared);
            Ok(())
        })
    }
}

/// Reloads the files when they change (`i18n.reload`), until shutdown.
fn watch(i18n: &I18n, ctx: &AppContext) {
    let shared = Arc::downgrade(&i18n.shared);
    let dir = i18n.options().dir.clone();
    let shutdown = ctx.shutdown_token();
    ctx.tracker().spawn(async move {
        let mut last = catalog::fingerprint(&dir);
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                () = tokio::time::sleep(RELOAD_EVERY) => {}
            }
            let Some(shared) = shared.upgrade() else {
                break;
            };
            let now = catalog::fingerprint(&dir);
            if now == last {
                continue;
            }
            last = now;
            match (I18n { shared }).reload() {
                Ok(()) => tracing::info!(target: "navree::i18n", "Fluent files reloaded"),
                Err(error) => tracing::warn!(
                    target: "navree::i18n",
                    %error,
                    "Fluent files changed but could not be loaded; keeping the previous messages"
                ),
            }
        }
    });
}
