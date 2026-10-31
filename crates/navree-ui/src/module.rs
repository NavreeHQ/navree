//! [`UiModule`], [`UiOptions`] and the [`Ui`] service.

use std::sync::Arc;

use http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG};
use http::{HeaderValue, Method};
use navree_config::{Config, ConfigSection};
use navree_http::body::Bytes;
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, Controller, ModuleBuilderHttpExt, Path, RouteDef};
use navree_kernel::{
    BoxError, BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, ModuleId,
    ProviderKey, Resolver,
};
use navree_view::{ModuleBuilderViewExt, Views};
use serde::Deserialize;

use crate::{HtmlPage, Shell, assets, gallery};

/// Options of [`UiModule`], configuration section `ui`:
///
/// ```toml
/// [ui]
/// mount = "/_navree/ui"   # APP_UI__MOUNT: where the assets (and the gallery) are served
/// gallery = false         # APP_UI__GALLERY: the component gallery at {mount}/gallery
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct UiOptions {
    /// The path the assets are served under, as `{mount}/{version}/{file}`.
    /// Default: `/_navree/ui`.
    pub mount: String,
    /// Serve the component gallery at `{mount}/gallery` (and its example
    /// pages under it). Default: on in debug builds, off in release builds.
    pub gallery: bool,
}

impl Default for UiOptions {
    fn default() -> Self {
        Self {
            mount: assets::DEFAULT_MOUNT.to_owned(),
            gallery: cfg!(debug_assertions),
        }
    }
}

impl ConfigSection for UiOptions {
    const KEY: &'static str = "ui";
}

impl UiOptions {
    /// The mount path without a trailing slash, checked.
    fn mount(&self) -> Result<String, String> {
        let mount = self.mount.trim_end_matches('/');
        if !mount.starts_with('/') || mount.contains(['{', '}', '*', '?', '#', ' ']) {
            return Err(format!(
                "ui.mount must be an absolute path such as /_navree/ui (got `{}`)",
                self.mount
            ));
        }
        Ok(mount.to_owned())
    }
}

/// The kit as a service: where its assets are served, and a [`Shell`]
/// pointing at them. Provided and exported by [`UiModule`].
#[derive(Clone, Debug)]
pub struct Ui {
    options: UiOptions,
    mount: String,
    assets_url: String,
}

impl Ui {
    /// The service for `options` (an invalid mount is an error).
    pub fn new(options: UiOptions) -> Result<Ui, BoxError> {
        let mount = options.mount()?;
        Ok(Ui {
            assets_url: assets::url(&mount),
            mount,
            options,
        })
    }

    /// The options.
    pub fn options(&self) -> &UiOptions {
        &self.options
    }

    /// The mount path (no trailing slash).
    pub fn mount(&self) -> &str {
        &self.mount
    }

    /// The versioned assets URL, `{mount}/{version}`: what a [`Shell`]
    /// needs in [`Shell::assets`].
    pub fn assets_url(&self) -> &str {
        &self.assets_url
    }

    /// A [`Shell`] for app `name` with this mount's assets.
    pub fn shell(&self, name: impl Into<String>) -> Shell {
        Shell::new(name).assets(self.assets_url.clone())
    }
}

/// The UI kit module: global; provides and exports [`Ui`], contributes the
/// `ui/*` templates to the app's [`Views`] (the app includes `ViewModule`),
/// and serves the assets at `GET {mount}/{version}/{*file}` (public, cached
/// for a year, immutable). With [`UiOptions::gallery`] it also serves the
/// component gallery at `GET {mount}/gallery`, `{mount}/gallery/dashboard`
/// and `{mount}/gallery/sign-in`.
///
/// - [`UiModule::for_root`] takes [`UiOptions`] in code;
/// - [`UiModule::from_config`] (and a plain `.module(UiModule)` or an import
///   with no configured instance in the app) reads section `ui` at boot, so
///   the app must include `ConfigModule`.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_ui::UiModule;
/// use navree_view::ViewModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(ViewModule::from_config())
///     .module(UiModule::from_config())
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct UiModule;

impl UiModule {
    /// The module configured with `options` (configure it once, in the app
    /// root).
    pub fn for_root(options: UiOptions) -> impl Module {
        ConfiguredUiModule {
            source: Source::Options(Arc::new(options)),
        }
    }

    /// The module configured from section `ui` of the app's `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredUiModule {
            source: Source::Config,
        }
    }
}

impl Module for UiModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, Source::Config);
    }
}

/// `UiModule::for_root(...)` / `from_config()`: same module id, configured.
struct ConfiguredUiModule {
    source: Source,
}

impl Module for ConfiguredUiModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<UiModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.source.clone());
    }
}

/// Where the options come from.
#[derive(Clone)]
enum Source {
    Options(Arc<UiOptions>),
    Config,
}

impl Source {
    async fn options(&self, r: &Resolver) -> Result<UiOptions, BoxError> {
        Ok(match self {
            Source::Options(options) => UiOptions::clone(options),
            Source::Config => r.get::<Config>().await?.typed::<UiOptions>()?,
        })
    }

    fn deps(&self) -> Vec<ProviderKey> {
        match self {
            Source::Options(_) => Vec::new(),
            Source::Config => vec![ProviderKey::of::<Config>()],
        }
    }
}

fn register(m: &mut ModuleBuilder, source: Source) {
    let deps = source.deps();
    m.global()
        .factory(
            Factory::<Ui>::new(move |r| {
                let source = source.clone();
                async move { Ok(Arc::new(Ui::new(source.options(&r).await?)?)) }
            })
            .deps(deps),
        )
        .exports::<Ui>()
        .templates(crate::TEMPLATES)
        .controller::<UiController>();
}

/// The asset (and gallery) routes.
struct UiController {
    ui: Arc<Ui>,
    gallery: Option<Arc<Views>>,
}

impl Injectable for UiController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Ui>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let ui = r.get::<Ui>().await?;
            let gallery = if ui.options().gallery {
                let views = gallery::views().map_err(|error| KernelError::Construction {
                    provider: std::any::type_name::<UiController>().to_owned(),
                    source: Box::new(error),
                })?;
                Some(Arc::new(views))
            } else {
                None
            };
            Ok(Self { ui, gallery })
        })
    }
}

impl Controller for UiController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let mount = self.ui.mount().to_owned();
        let mut routes = vec![
            RouteDef::on(
                Method::GET,
                format!("!{mount}/{{version}}/{{*file}}"),
                |Path((version, file)): Path<(String, String)>| async move {
                    serve_asset(&version, &file)
                },
            )
            .name("ui.assets")
            .public(),
        ];
        if let Some(views) = &self.gallery {
            for page in gallery::PAGES {
                let views = views.clone();
                let ui = self.ui.clone();
                let path = match page.slug {
                    "" => format!("!{mount}/gallery"),
                    slug => format!("!{mount}/gallery/{slug}"),
                };
                routes.push(
                    RouteDef::on(Method::GET, path, move || {
                        let views = views.clone();
                        let ui = ui.clone();
                        async move {
                            let links = gallery::Links::mounted(ui.mount());
                            match gallery::render(&views, page.slug, ui.assets_url(), &links) {
                                Ok(html) => HtmlPage(html).into_response(),
                                Err(error) => AppError::internal(error).into_response(),
                            }
                        }
                    })
                    .name(page.route)
                    .public(),
                );
            }
        }
        routes
    }
}

/// `GET {mount}/{version}/{file}`: the embedded file, cached for a year when
/// the version is current (a stale version still gets the file, uncached).
fn serve_asset(version: &str, file: &str) -> Response {
    let Some(asset) = assets::get(file) else {
        return AppError::not_found().into_response();
    };
    let current = assets::version();
    let cache = if version == current {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(asset.content_type)),
            (CACHE_CONTROL, HeaderValue::from_static(cache)),
            (
                ETAG,
                HeaderValue::from_str(&format!("\"{current}\""))
                    .unwrap_or(HeaderValue::from_static("\"navree-ui\"")),
            ),
        ],
        Bytes::from_static(asset.bytes),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_must_be_absolute_paths() {
        assert!(Ui::new(UiOptions::default()).is_ok());
        let ui = Ui::new(UiOptions {
            mount: "/admin/ui/".to_owned(),
            gallery: false,
        })
        .unwrap();
        assert_eq!(ui.mount(), "/admin/ui");
        assert_eq!(ui.assets_url(), format!("/admin/ui/{}", assets::version()));
        for bad in ["ui", "/ui/{x}", "/ui/*", "/a b"] {
            assert!(
                Ui::new(UiOptions {
                    mount: bad.to_owned(),
                    gallery: false
                })
                .is_err(),
                "{bad}"
            );
        }
    }
}
