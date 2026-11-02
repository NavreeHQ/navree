//! [`Core`]: the panel's shared state (options, registry, templates,
//! assets and the metric cache), built once when the panel boots.

use std::sync::Arc;
use std::time::Duration;

use navree_crypto::sha256_hex;
use navree_kernel::{AppContext, BoxError};
use navree_validation::Messages;
use navree_view::Views;

use crate::field::FieldKind;
use crate::metric::MetricResult;
use crate::registry::Registry;
use crate::workspace::{StaffCheck, StaffPolicy};
use crate::{HelmOptions, save, templates};

/// The panel's stylesheet.
pub(crate) const CSS: &str = include_str!("assets/helm.css");

/// The panel's script (no inline scripts anywhere: the pages' CSP allows
/// only this file).
pub(crate) const JS: &str = include_str!("assets/helm.js");

/// The panel's static files, with a version for immutable caching.
pub(crate) struct Assets {
    /// `theme.css`: the accent color from `helm.accent`.
    pub(crate) theme: String,
    /// A short digest of every asset, in their URLs (`?v=..`).
    pub(crate) version: String,
}

impl Assets {
    fn new(options: &HelmOptions) -> Self {
        let theme = theme_css(&options.accent);
        let version = sha256_hex(format!("{CSS}{JS}{theme}").as_bytes())[..12].to_owned();
        Self { theme, version }
    }
}

/// The custom properties of the accent color.
fn theme_css(accent: &str) -> String {
    let (r, g, b) = rgb(accent).unwrap_or((0x3d, 0xff, 0x8f));
    // Text on the accent: dark ink on light accents, white on dark ones.
    let luminance = 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
    let ink = if luminance > 0.35 {
        "#03130a"
    } else {
        "#ffffff"
    };
    format!(
        ":root{{--accent:{accent};--accent-rgb:{r},{g},{b};--accent-ink:{ink};}}\n",
        accent = accent.trim()
    )
}

/// The relative luminance of an sRGB channel.
fn channel(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// `#rgb` or `#rrggbb` as its channels.
fn rgb(color: &str) -> Option<(u8, u8, u8)> {
    let hex = color.trim().strip_prefix('#')?;
    let expand = |s: &str| u8::from_str_radix(&s.repeat(if s.len() == 1 { 2 } else { 1 }), 16).ok();
    match hex.len() {
        3 => Some((
            expand(&hex[0..1])?,
            expand(&hex[1..2])?,
            expand(&hex[2..3])?,
        )),
        6 => Some((
            expand(&hex[0..2])?,
            expand(&hex[2..4])?,
            expand(&hex[4..6])?,
        )),
        _ => None,
    }
}

/// The panel's shared state.
pub(crate) struct Core {
    pub(crate) options: Arc<HelmOptions>,
    pub(crate) registry: Registry,
    pub(crate) messages: Messages,
    /// The panel's own templates.
    pub(crate) views: Views,
    /// The app's templates (`ViewModule`), for view fields and pages.
    pub(crate) app_views: Option<Arc<Views>>,
    pub(crate) metrics: Option<navree_bounded::Cache<String, Arc<MetricResult>>>,
    pub(crate) assets: Assets,
    /// Who may work in a workspace they are not in, if the app says.
    pub(crate) staff: Option<StaffCheck>,
}

impl Core {
    /// Builds the registry and everything else; fails the boot with the
    /// first problem in a resource or page.
    pub(crate) async fn new(app: AppContext, options: Arc<HelmOptions>) -> Result<Core, BoxError> {
        let registry = Registry::build(&app, options.clone()).await?;
        let app_views = app.try_get::<Views>().await;
        if app_views.is_none() {
            let view_field = registry.resources().iter().find_map(|r| {
                r.info()
                    .fields
                    .iter()
                    .find(|f| matches!(f.kind, FieldKind::View { .. }))
                    .map(|f| (r.info().type_name, f.name.clone()))
            });
            if let Some((resource, field)) = view_field {
                return Err(format!(
                    "Helm resource `{resource}`: view field `{field}` renders an app template, so the app needs ViewModule"
                )
                .into());
            }
            if let Some(page) = registry.pages().first() {
                return Err(format!(
                    "Helm page `{}` renders an app template, so the app needs ViewModule",
                    page.page.slug
                )
                .into());
            }
        }
        let metrics = (options.metrics_cache_secs > 0).then(|| {
            navree_bounded::Cache::builder(10_000)
                .time_to_live(Duration::from_secs(options.metrics_cache_secs))
                .build()
        });
        let mut policies = app.contributions::<StaffPolicy>().into_iter();
        let staff = policies.next().map(|policy| policy.value.0.clone());
        if policies.next().is_some() {
            return Err("Helm: `helm_staff_workspaces` is registered more than once".into());
        }
        Ok(Core {
            staff,
            messages: save::messages(&app).await,
            views: templates::views()?,
            assets: Assets::new(&options),
            registry,
            app_views,
            metrics,
            options,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_follows_the_accent() {
        let theme = theme_css("#3dff8f");
        assert!(theme.contains("--accent:#3dff8f"), "{theme}");
        assert!(theme.contains("--accent-rgb:61,255,143"), "{theme}");
        assert!(theme.contains("--accent-ink:#03130a"), "{theme}");
        assert!(theme_css("#123").contains("--accent-ink:#ffffff"));
        assert_eq!(rgb("#fff"), Some((255, 255, 255)));
        assert_eq!(rgb("nope"), None);
    }
}
