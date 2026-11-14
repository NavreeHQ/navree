//! [`Registry`]: every resource and page of the panel, and the links and cards
//! other modules add, built when the panel boots.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use navree_kernel::{AppContext, BoxError, BoxFuture, Injectable, ModuleId};
use navree_ui::html::LocalHref;

use crate::field::{FieldKind, Related};
use crate::handle::{ErasedResource, Handle};
use crate::page::PageRegistration;
use crate::plugin::{LinkRegistration, WidgetRegistration, html_template};
use crate::{HelmLink, HelmOptions, HelmPage, HelmWidget, Resource};

/// Builds a resource's handle.
type Build = fn(
    AppContext,
    ModuleId,
    Arc<HelmOptions>,
) -> BoxFuture<'static, Result<Arc<dyn ErasedResource>, BoxError>>;

/// A resource registered by a module (a contribution of `m.resource::<R>()`).
#[derive(Clone)]
pub(crate) struct ResourceRegistration {
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) module: ModuleId,
    build: Build,
}

impl ResourceRegistration {
    pub(crate) fn of<R: Resource + Injectable>(module: ModuleId) -> Self {
        Self {
            type_id: TypeId::of::<R>(),
            type_name: std::any::type_name::<R>(),
            module,
            build: build::<R>,
        }
    }
}

fn build<R: Resource + Injectable>(
    app: AppContext,
    module: ModuleId,
    options: Arc<HelmOptions>,
) -> BoxFuture<'static, Result<Arc<dyn ErasedResource>, BoxError>> {
    Box::pin(async move {
        let resource = app.resolver_for(module).get::<R>().await?;
        let handle = Handle::<R>::build(resource, module, &options)?;
        Ok(Arc::new(handle) as Arc<dyn ErasedResource>)
    })
}

/// A registered page.
#[derive(Clone, Debug)]
pub(crate) struct PageEntry {
    pub(crate) page: HelmPage,
    pub(crate) module: ModuleId,
}

/// A link another module added to the menu, checked at boot.
#[derive(Clone, Debug)]
pub(crate) struct LinkEntry {
    pub(crate) link: HelmLink,
    pub(crate) module: ModuleId,
}

/// A card another module added to the dashboard, checked at boot.
#[derive(Clone, Debug)]
pub(crate) struct WidgetEntry {
    pub(crate) widget: HelmWidget,
    pub(crate) module: ModuleId,
    /// The name its template is rendered under: always an `.html` template.
    pub(crate) template: String,
}

/// The panel's resources (in menu order) and pages, and the links and cards
/// other modules add.
#[derive(Default)]
pub(crate) struct Registry {
    resources: Vec<Arc<dyn ErasedResource>>,
    by_slug: HashMap<String, usize>,
    by_type: HashMap<TypeId, usize>,
    pages: Vec<PageEntry>,
    links: Vec<LinkEntry>,
    widgets: Vec<WidgetEntry>,
}

impl Registry {
    /// Builds every resource and page the app's modules registered; the
    /// error explains the first problem.
    pub(crate) async fn build(
        app: &AppContext,
        options: Arc<HelmOptions>,
    ) -> Result<Registry, BoxError> {
        let mut seen: HashMap<TypeId, ModuleId> = HashMap::new();
        let mut resources = Vec::new();
        for contributed in app.contributions::<ResourceRegistration>() {
            let registration = &*contributed.value;
            match seen.get(&registration.type_id) {
                Some(module) if *module == registration.module => continue,
                Some(_) => {
                    return Err(format!(
                        "Helm resource `{}` is registered by two modules",
                        registration.type_name
                    )
                    .into());
                }
                None => {
                    seen.insert(registration.type_id, registration.module);
                }
            }
            resources.push(
                (registration.build)(app.clone(), registration.module, options.clone()).await?,
            );
        }
        resources.sort_by(|a, b| {
            let (a, b) = (a.info(), b.info());
            (a.group.as_str(), a.order, a.label).cmp(&(b.group.as_str(), b.order, b.label))
        });
        let mut registry = Registry::default();
        for (index, resource) in resources.iter().enumerate() {
            let info = resource.info();
            if let Some(&other) = registry.by_slug.get(&info.slug) {
                return Err(format!(
                    "Helm resources `{}` and `{}` both use the slug `{}`; set `const SLUG` on one",
                    resources[other].info().type_name,
                    info.type_name,
                    info.slug
                )
                .into());
            }
            registry.by_slug.insert(info.slug.clone(), index);
            registry.by_type.insert(info.type_id, index);
            if !info.icon.is_empty() && !crate::icons::exists(info.icon) {
                tracing::warn!(
                    target: "navree::helm",
                    resource = info.type_name,
                    icon = info.icon,
                    "the panel knows no icon by this name (any Lucide name needs the `helm-all-icons` feature); the menu shows a generic icon"
                );
            }
        }
        registry.resources = resources;
        registry.check_relations()?;

        let mut slugs = HashSet::new();
        for contributed in app.contributions::<PageRegistration>() {
            let PageRegistration { page, module } = (*contributed.value).clone();
            if page.template.is_none() {
                return Err(format!(
                    "Helm page `{}` has no template: add `.template(\"admin/{}\")`",
                    page.slug, page.slug
                )
                .into());
            }
            if page.slug.is_empty() || !slugs.insert(page.slug.clone()) {
                return Err(format!("two Helm pages use the slug `{}`", page.slug).into());
            }
            registry.pages.push(PageEntry { page, module });
        }

        // A link is drawn on every page of the panel: its address must be a path of this
        // app, never another site's.
        for contributed in app.contributions::<LinkRegistration>() {
            let LinkRegistration { link, module } = (*contributed.value).clone();
            if LocalHref::parse(&link.href).is_err() || !link.href.starts_with('/') {
                return Err(format!(
                    "m.helm_nav(\"{}\") in `{}`: the link must be a path on this app, such as /announcements",
                    link.label, contributed.module.name
                )
                .into());
            }
            let entry = LinkEntry { link, module };
            if !entry.link.icon.is_empty() && !crate::icons::exists(&entry.link.icon) {
                tracing::warn!(
                    target: "navree::helm",
                    link = %entry.link.label,
                    module = entry.module.name(),
                    icon = %entry.link.icon,
                    "the panel knows no icon by this name (any Lucide name needs the `helm-all-icons` feature); the menu shows a generic icon"
                );
            }
            registry.links.push(entry);
        }

        let mut keys = HashSet::new();
        for contributed in app.contributions::<WidgetRegistration>() {
            let WidgetRegistration { widget, module } = (*contributed.value).clone();
            let well_formed = !widget.key.is_empty()
                && widget
                    .key
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
            if !well_formed {
                return Err(format!(
                    "Helm widget key `{}` in `{}` must be lower-case letters, digits and -",
                    widget.key, contributed.module.name
                )
                .into());
            }
            let Some(named) = widget.template.as_deref() else {
                return Err(format!(
                    "Helm widget `{}` has no template: add .template(\"admin/widgets/{}\")",
                    widget.key, widget.key
                )
                .into());
            };
            // Only an HTML template escapes what it writes: a card renders nothing else.
            let Some(template) = html_template(named) else {
                return Err(format!(
                    "Helm widget `{}` names the template `{named}`, which is not HTML: name an .html template, such as .template(\"admin/widgets/{}\")",
                    widget.key, widget.key
                )
                .into());
            };
            if !keys.insert(widget.key.clone()) {
                return Err(format!("two Helm widgets use the key `{}`", widget.key).into());
            }
            registry.widgets.push(WidgetEntry {
                widget,
                module,
                template,
            });
        }
        // A stable sort: cards of one order keep the order they were added in.
        registry.widgets.sort_by_key(|entry| entry.widget.order);
        Ok(registry)
    }

    /// Every relationship field and filter must name a registered resource.
    fn check_relations(&self) -> Result<(), BoxError> {
        let missing = |owner: &str, what: &str, related: &Related| -> BoxError {
            format!(
                "Helm resource `{owner}`: {what} refers to `{}`, which is not registered; add `m.resource::<{}>()` to its module",
                related.type_name,
                related
                    .type_name
                    .rsplit("::")
                    .next()
                    .unwrap_or(related.type_name)
            )
            .into()
        };
        for resource in &self.resources {
            let info = resource.info();
            for field in &info.fields {
                if let FieldKind::BelongsTo(related) | FieldKind::HasMany(related) = &field.kind {
                    if !self.by_type.contains_key(&related.type_id) {
                        return Err(missing(
                            info.type_name,
                            &format!("field `{}`", field.name),
                            related,
                        ));
                    }
                    if let FieldKind::HasMany(related) = &field.kind {
                        let target = self.resources[self.by_type[&related.type_id]].info();
                        let refers = target.fields.iter().any(|f| {
                            f.name == field.name && matches!(f.kind, FieldKind::BelongsTo(_))
                        });
                        if !refers {
                            return Err(format!(
                                "Helm resource `{}`: has_many field `{}` needs `{}` to have a `Field::belongs_to` on `{}`",
                                info.type_name, field.name, target.type_name, field.name
                            )
                            .into());
                        }
                    }
                }
            }
            for filter in &info.filters {
                if let Some(related) = &filter.related
                    && !self.by_type.contains_key(&related.type_id)
                {
                    return Err(missing(
                        info.type_name,
                        &format!("filter `{}`", filter.name),
                        related,
                    ));
                }
            }
        }
        Ok(())
    }

    /// The resources, in menu order.
    pub(crate) fn resources(&self) -> &[Arc<dyn ErasedResource>] {
        &self.resources
    }

    /// The resource with `slug`.
    pub(crate) fn by_slug(&self, slug: &str) -> Option<&Arc<dyn ErasedResource>> {
        self.by_slug.get(slug).map(|&i| &self.resources[i])
    }

    /// The resource of type `type_id`.
    pub(crate) fn by_type(&self, type_id: TypeId) -> Option<&Arc<dyn ErasedResource>> {
        self.by_type.get(&type_id).map(|&i| &self.resources[i])
    }

    /// The pages, in registration order.
    pub(crate) fn pages(&self) -> &[PageEntry] {
        &self.pages
    }

    /// The page with `slug`.
    pub(crate) fn page(&self, slug: &str) -> Option<&PageEntry> {
        self.pages.iter().find(|p| p.page.slug == slug)
    }

    /// The links other modules add to the menu, in registration order.
    pub(crate) fn links(&self) -> &[LinkEntry] {
        &self.links
    }

    /// The cards other modules add to the dashboard, by their order, then in
    /// registration order.
    pub(crate) fn widgets(&self) -> &[WidgetEntry] {
        &self.widgets
    }
}
