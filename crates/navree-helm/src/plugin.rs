//! [`HelmLink`] and [`HelmWidget`]: a link any module adds to the panel's menu and a card it
//! adds to the panel's dashboard, with [`ModuleBuilderHelmPanelExt`]. Both are checked when
//! the panel boots and drawn, for each request, only for a user who passes their check.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use navree_http::AppError;
use navree_kernel::{ModuleBuilder, ModuleId};
use serde_json::Value;

use crate::PageContext;
use crate::page::DataFn;

/// A link a module adds to the panel's menu: a page of the app that panel users go to.
/// Register it with [`m.helm_nav(..)`](ModuleBuilderHelmPanelExt::helm_nav).
///
/// It joins the menu's group of the same label, as a [`HelmPage`](crate::HelmPage) does,
/// after the panel's resources and pages, and **Go to** lists it too. Without
/// [`can`](HelmLink::can) every user allowed into the panel sees it; with it, only those
/// who pass the check. Decided on the server for every request.
///
/// Hiding a link is not access control: the page it leads to declares its own access.
///
/// ```
/// use navree_helm::HelmLink;
///
/// let _link = HelmLink::new("Announcements", "/announcements")
///     .group("Workspace")
///     .icon("megaphone")
///     .can("read", "announcements.Announcement");
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct HelmLink {
    pub(crate) label: String,
    pub(crate) href: String,
    pub(crate) group: Option<String>,
    pub(crate) icon: String,
    pub(crate) can: Option<(String, String)>,
}

impl HelmLink {
    /// A link labelled `label` to `href`, a path on this app (`/announcements`). An address
    /// that is not one (another site, a relative path, an empty one) stops the boot.
    pub fn new(label: &str, href: &str) -> HelmLink {
        HelmLink {
            label: label.to_owned(),
            href: href.to_owned(),
            group: None,
            icon: String::new(),
            can: None,
        }
    }

    /// The menu group (default: **Pages**, as [`HelmPage::group`](crate::HelmPage::group)).
    pub fn group(mut self, group: &str) -> HelmLink {
        self.group = Some(group.to_owned());
        self
    }

    /// A Lucide icon name, as [`HelmPage::icon`](crate::HelmPage::icon). One the panel does
    /// not know is warned about at boot and drawn as a generic icon.
    pub fn icon(mut self, icon: &str) -> HelmLink {
        self.icon = icon.to_owned();
        self
    }

    /// Shows the link only to users who may do `action` on `subject` (a type-level
    /// check), as [`HelmPage::can`](crate::HelmPage::can); every panel user otherwise.
    pub fn can(mut self, action: &str, subject: &str) -> HelmLink {
        self.can = Some((action.to_owned(), subject.to_owned()));
        self
    }
}

/// A card a module adds to the panel's dashboard, after the metric cards: a template of the
/// app's, given data the module computes. Register it with
/// [`m.helm_widget(..)`](ModuleBuilderHelmPanelExt::helm_widget).
///
/// The template (a navree-view template; the app needs `ViewModule`) gets `title`, `user`
/// (`{id, name, email}`), `path` (the panel's path) and `data` (what
/// [`data`](HelmWidget::data) returns, else `null`). It is always an HTML template, so
/// navree-view escapes what it writes: see [`template`](HelmWidget::template). Cards are
/// drawn while `helm.dashboard` is on, by [`order`](HelmWidget::order), then in the order
/// they were added. Without [`can`](HelmWidget::can) every panel user sees the card; with
/// it, only those who pass the check, and the data function runs only for them. A card
/// whose data function or template fails is left out and logged, and the dashboard is drawn
/// without it.
///
/// ```
/// use navree_helm::HelmWidget;
///
/// let _card = HelmWidget::new("open-deals", "Open deals")
///     .template("admin/widgets/open-deals")
///     .data(|_ctx| async move { Ok(serde_json::json!({ "open": 12 })) })
///     .can("read", "Deal")
///     .order(10);
/// ```
#[derive(Clone)]
#[non_exhaustive]
pub struct HelmWidget {
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) template: Option<String>,
    pub(crate) data: Option<DataFn>,
    pub(crate) can: Option<(String, String)>,
    pub(crate) order: i32,
}

impl HelmWidget {
    /// A card titled `title`. `key` names it: lower-case letters, digits and `-`, unique in
    /// the app; another key stops the boot.
    pub fn new(key: &str, title: &str) -> HelmWidget {
        HelmWidget {
            key: key.to_owned(),
            title: title.to_owned(),
            template: None,
            data: None,
            can: None,
            order: 0,
        }
    }

    /// The card's template (a navree-view template), given `title`, `user`, `path` and
    /// `data`. It is an HTML template, so navree-view escapes what it writes: a name without
    /// an extension stands for that name with `.html` added (`admin/widgets/open-deals` is
    /// `admin/widgets/open-deals.html`, never another file of that name), and a name may
    /// end in `.html`. Any other name (a `.txt` or `.md` template prints values as they are)
    /// stops the boot, and so does a card without a template.
    pub fn template(mut self, name: &str) -> HelmWidget {
        self.template = Some(name.to_owned());
        self
    }

    /// Provides the template's `data`: `f(ctx)` returns a JSON value. It runs each time the
    /// dashboard is drawn for a user who may see the card, and for nobody else.
    pub fn data<F, Fut>(mut self, f: F) -> HelmWidget
    where
        F: Fn(PageContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, AppError>> + Send + 'static,
    {
        self.data = Some(Arc::new(move |ctx| Box::pin(f(ctx))));
        self
    }

    /// Shows the card only to users who may do `action` on `subject` (a type-level check),
    /// as [`HelmPage::can`](crate::HelmPage::can); every panel user otherwise.
    pub fn can(mut self, action: &str, subject: &str) -> HelmWidget {
        self.can = Some((action.to_owned(), subject.to_owned()));
        self
    }

    /// Lower comes first (default 0); ties keep the order the cards were added in.
    pub fn order(mut self, order: i32) -> HelmWidget {
        self.order = order;
        self
    }
}

impl fmt::Debug for HelmWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HelmWidget")
            .field("key", &self.key)
            .field("title", &self.title)
            .field("template", &self.template)
            .field("can", &self.can)
            .field("order", &self.order)
            .finish_non_exhaustive()
    }
}

/// Links and dashboard cards on a [`ModuleBuilder`]: what any module, a plugin's too, adds
/// to the admin panel. In an app without `HelmModule` they add nothing.
///
/// ```
/// use navree_helm::{HelmLink, HelmWidget, ModuleBuilderHelmPanelExt};
/// use navree_kernel::{Module, ModuleBuilder};
///
/// struct AnnouncementsModule;
///
/// impl Module for AnnouncementsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.helm_nav(HelmLink::new("Announcements", "/announcements").icon("megaphone"))
///             .helm_widget(
///                 HelmWidget::new("announcements", "Latest announcements")
///                     .template("admin/widgets/announcements")
///                     .can("read", "announcements.Announcement"),
///             );
///     }
/// }
/// ```
pub trait ModuleBuilderHelmPanelExt {
    /// Adds a link to the panel's menu.
    fn helm_nav(&mut self, link: HelmLink) -> &mut Self;

    /// Adds a card to the panel's dashboard.
    fn helm_widget(&mut self, widget: HelmWidget) -> &mut Self;
}

impl ModuleBuilderHelmPanelExt for ModuleBuilder {
    fn helm_nav(&mut self, link: HelmLink) -> &mut Self {
        let module = self.module_id();
        self.contribute(LinkRegistration { link, module })
    }

    fn helm_widget(&mut self, widget: HelmWidget) -> &mut Self {
        let module = self.module_id();
        self.contribute(WidgetRegistration { widget, module })
    }
}

/// A link registered by a module (a contribution of `m.helm_nav(..)`).
#[derive(Clone, Debug)]
pub(crate) struct LinkRegistration {
    pub(crate) link: HelmLink,
    pub(crate) module: ModuleId,
}

/// A card registered by a module (a contribution of `m.helm_widget(..)`).
#[derive(Clone, Debug)]
pub(crate) struct WidgetRegistration {
    pub(crate) widget: HelmWidget,
    pub(crate) module: ModuleId,
}

/// The `can` checks declared by every Helm link, card and custom page in `ctx`,
/// for the boot check against plugin namespaces.
pub(crate) async fn declared_access(
    ctx: navree_kernel::AppContext,
) -> Result<Vec<navree_http::__private::DeclaredAccess>, navree_kernel::BoxError> {
    let mut declared = Vec::new();
    for contributed in ctx.contributions::<LinkRegistration>() {
        if let Some((action, subject)) = &contributed.value.link.can {
            declared.push(
                navree_http::__private::DeclaredAccess::value_can(
                    "Helm link",
                    &contributed.value.link.label,
                    action,
                    subject,
                )
                .in_module(contributed.module.id, contributed.module.name),
            );
        }
    }
    for contributed in ctx.contributions::<WidgetRegistration>() {
        if let Some((action, subject)) = &contributed.value.widget.can {
            declared.push(
                navree_http::__private::DeclaredAccess::value_can(
                    "Helm card",
                    &contributed.value.widget.key,
                    action,
                    subject,
                )
                .in_module(contributed.module.id, contributed.module.name),
            );
        }
    }
    for contributed in ctx.contributions::<crate::page::PageRegistration>() {
        if let Some((action, subject)) = &contributed.value.page.can {
            declared.push(
                navree_http::__private::DeclaredAccess::value_can(
                    "Helm page",
                    &contributed.value.page.slug,
                    action,
                    subject,
                )
                .in_module(contributed.module.id, contributed.module.name),
            );
        }
    }
    Ok(declared)
}

/// The name a card's template `name` is rendered under, one navree-view always escapes as
/// HTML: `name` when its last segment ends in `.html`, `name` with `.html` added when its
/// last segment has no extension (so navree-view never picks a `.txt` file of that name),
/// and `None` for any other name, which the boot refuses.
pub(crate) fn html_template(name: &str) -> Option<String> {
    let file = name.rsplit('/').next().unwrap_or(name);
    match file.strip_suffix(".html") {
        Some(stem) if !stem.is_empty() => Some(name.to_owned()),
        _ if !file.is_empty() && !file.contains('.') => Some(format!("{name}.html")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_keeps_its_key_as_given_and_its_order() {
        let card = HelmWidget::new("Pipeline", "Open pipeline").order(-2);
        assert_eq!(card.key, "Pipeline");
        assert_eq!(card.order, -2);
        assert!(card.template.is_none() && card.data.is_none());
        let link = HelmLink::new("Status", "/status").can("read", "Deal");
        assert_eq!(link.can, Some(("read".into(), "Deal".into())));
        assert_eq!(link.group, None);
    }

    #[test]
    fn a_card_renders_only_an_html_template() {
        assert_eq!(
            html_template("admin/widgets/pipeline").as_deref(),
            Some("admin/widgets/pipeline.html")
        );
        assert_eq!(
            html_template("admin/widgets.v2/pipeline").as_deref(),
            Some("admin/widgets.v2/pipeline.html")
        );
        assert_eq!(
            html_template("admin/widgets/pipeline.html").as_deref(),
            Some("admin/widgets/pipeline.html")
        );
        for name in [
            "admin/widgets/latest.txt",
            "admin/widgets/latest.md",
            "admin/widgets/latest.json",
            "admin/widgets/latest.txt.j2",
            "admin/widgets/latest.j2",
            "admin/widgets/latest.svg",
            "admin/widgets/latest.HTML",
            "admin/widgets/latest.html.",
            "admin/widgets/.html",
            "admin/widgets/",
            "",
        ] {
            assert_eq!(html_template(name), None, "{name:?}");
        }
    }
}
