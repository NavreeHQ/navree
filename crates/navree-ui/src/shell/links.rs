//! The links modules add to the app's sidebar with `m.nav_item`: checked once at boot into
//! [`Links`], in the order the sidebar draws them, and shown per request only to a reader
//! who may open them ([`visible`]), decided on the server for every request.

use std::borrow::Cow;
use std::sync::Arc;

use http::header::{self, HeaderValue};
use http::request::Parts;
use navree_contracts::{permission_catalog, permitted};
use navree_http::response::{IntoResponse, Response};
use navree_http::{CurrentPrincipal, Json, RequestWorkspace};
use navree_kernel::AppContext;
use uuid::Uuid;

use crate::html::LocalHref;
use crate::icons::Icon;
use crate::module::UiGate;
use crate::shell::{NavGroup, NavItem, ShellNavItem};

/// The group of a link that names none, when its module's plugin has no title.
const DEFAULT_GROUP: &str = "More";

/// A link a module added, checked at boot.
pub(crate) struct Link {
    pub(crate) label: Cow<'static, str>,
    pub(crate) href: LocalHref,
    pub(crate) icon: Option<Icon>,
    pub(crate) group: Cow<'static, str>,
    pub(crate) permission: Option<&'static str>,
    pub(crate) order: i32,
}

/// The contributed links, checked, in the order the sidebar draws them.
#[derive(Default)]
pub(crate) struct Links(pub(crate) Vec<Link>);

impl Links {
    /// Whether no module added a link.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Every link the app's modules add, checked at boot: an address that is not a path of
/// this app (`LocalHref::parse` refuses it, or it does not start with `/`) stops the boot,
/// and a permission no module describes with `m.permissions` is warned about. Groups come
/// in the order of their first link (module order, then registration order), an empty
/// group is the default one (the plugin's title, else `More`), and the links of a group
/// are ordered by `order`, ties in module then registration order.
pub(crate) fn check(ctx: &AppContext) -> Result<Links, String> {
    let added = ctx.contributions::<ShellNavItem>();
    if added.is_empty() {
        return Ok(Links::default());
    }
    let catalog = permission_catalog(ctx);
    let mut groups: Vec<(Cow<'static, str>, Vec<Link>)> = Vec::new();
    for contributed in added {
        let module = contributed.module.name;
        let item: &ShellNavItem = &contributed.value;
        let href = LocalHref::parse(item.href)
            .ok()
            .filter(|_| item.href.starts_with('/'))
            .ok_or_else(|| {
                format!(
                    "m.nav_item(\"{}\") in `{module}`: the link must be a path on this app, such as /announcements",
                    item.label
                )
            })?;
        if let Some(permission) = item.permission
            && !catalog
                .iter()
                .any(|(_, declared)| declared.name == permission)
        {
            let text = format!(
                "m.nav_item(\"{}\") in `{module}` asks for the permission `{permission}`, which no module declares with m.permissions",
                item.label
            );
            tracing::warn!(target: "navree::ui", "{text}");
        }
        let group = match &item.group {
            Some(group) if !group.is_empty() => group.clone(),
            _ => Cow::Borrowed(
                ctx.plugin(contributed.module.id)
                    .and_then(|plugin| plugin.title)
                    .unwrap_or(DEFAULT_GROUP),
            ),
        };
        let link = Link {
            label: item.label.clone(),
            href,
            icon: item.icon,
            group: group.clone(),
            permission: item.permission,
            order: item.order,
        };
        match groups.iter_mut().find(|(label, _)| *label == group) {
            Some((_, links)) => links.push(link),
            None => groups.push((group, vec![link])),
        }
    }
    let mut links = Vec::new();
    for (_, mut group) in groups {
        // A stable sort: ties keep module, then registration, order.
        group.sort_by_key(|link| link.order);
        links.extend(group);
    }
    Ok(Links(links))
}

/// The access declarations of every sidebar link added with `m.nav_item`, for
/// the boot check against plugin namespaces.
pub(crate) async fn declared_access(
    ctx: AppContext,
) -> Result<Vec<navree_http::__private::DeclaredAccess>, navree_kernel::BoxError> {
    let mut declared = Vec::new();
    for contributed in ctx.contributions::<ShellNavItem>() {
        if let Some(permission) = contributed.value.permission {
            declared.push(
                navree_http::__private::DeclaredAccess::value_permission(
                    "sidebar link",
                    contributed.value.label.as_ref(),
                    permission,
                )
                .in_module(contributed.module.id, contributed.module.name),
            );
        }
    }
    Ok(declared)
}

/// What [`visible`] decided for a request, kept in that request's extensions only.
#[derive(Clone)]
struct Visible(Arc<[usize]>);

/// Which of `links` the request's reader may open (indexes into `links.0`, in order),
/// decided once per request and kept in the request's extensions.
///
/// A link without a permission is open to every reader, a guest too. A link with one is
/// open only to a signed-in reader whom `navree_contracts::permitted` grants it, app-wide
/// or in the workspace the app's `dyn RequestWorkspace` names for the request: asked once,
/// only when such a link is checked, and never entered. Without that provider the check is
/// app-wide. A check that fails hides the link and logs a warning naming the permission,
/// never the reader.
pub(crate) async fn visible(parts: &mut Parts, links: &Links) -> Arc<[usize]> {
    if let Some(Visible(done)) = parts.extensions.get::<Visible>() {
        return done.clone();
    }
    let ctx = parts.extensions.get::<AppContext>().cloned();
    let reader = CurrentPrincipal::from_parts(parts)
        .ok()
        .map(|current| current.0);
    // The request's workspace: asked once, and only when needed.
    let mut workspace: Option<Option<Uuid>> = None;
    let mut shown = Vec::new();
    for (index, link) in links.0.iter().enumerate() {
        let allowed = match (link.permission, &reader, &ctx) {
            (None, _, _) => true,
            (Some(_), None, _) | (Some(_), _, None) => false,
            (Some(permission), Some(reader), Some(ctx)) => {
                if workspace.is_none() {
                    workspace = Some(match ctx.try_get::<dyn RequestWorkspace>().await {
                        Some(resolver) => resolver.resolve(parts).await,
                        None => None,
                    });
                }
                match permitted(ctx, reader, permission, workspace.flatten()).await {
                    Ok(allowed) => allowed,
                    Err(error) => {
                        tracing::warn!(
                            target: "navree::ui",
                            permission,
                            %error,
                            "a sidebar link's permission could not be checked; the link is left out"
                        );
                        false
                    }
                }
            }
        };
        if allowed {
            shown.push(index);
        }
    }
    let shown: Arc<[usize]> = shown.into();
    parts.extensions.insert(Visible(shown.clone()));
    shown
}

/// The `visible` links of `links` as the sidebar's groups: one group per label, in order,
/// each link with its icon when it has one, with no count and fetched only when clicked.
/// A group none of whose links is visible is not drawn.
pub(crate) fn groups(links: &Links, visible: &[usize]) -> Vec<NavGroup> {
    let mut groups: Vec<(Cow<'static, str>, Vec<NavItem>)> = Vec::new();
    for link in visible.iter().filter_map(|index| links.0.get(*index)) {
        let mut item = NavItem::new(link.label.clone(), link.href.clone().into_href());
        if let Some(icon) = link.icon {
            item = item.icon(icon);
        }
        match groups.last_mut() {
            Some((label, items)) if *label == link.group => items.push(item),
            _ => groups.push((link.group.clone(), vec![item])),
        }
    }
    groups
        .into_iter()
        .map(|(label, items)| items.into_iter().fold(NavGroup::new(label), NavGroup::item))
        .collect()
}

/// What the `AppShell` extractor draws for the request of `parts`: the groups of the links
/// its reader may open. Nothing in an app without `UiModule` or without a link.
pub(crate) async fn drawn(parts: &mut Parts) -> Vec<NavGroup> {
    let Some(ctx) = parts.extensions.get::<AppContext>().cloned() else {
        return Vec::new();
    };
    let Some(gate) = ctx.try_get::<UiGate>().await else {
        return Vec::new();
    };
    if gate.1.is_empty() {
        return Vec::new();
    }
    let shown = visible(parts, &gate.1).await;
    groups(&gate.1, &shown)
}

/// Where the links a caller may open are answered as JSON, under the toolkit's own prefix.
pub(crate) const NAVIGATION_PATH: &str = "/_navree/ui/navigation";

/// The answer of `GET /_navree/ui/navigation`: `{"items": [..]}`, one object per link the
/// caller may open ([`visible`]), in the order the sidebar draws them, with exactly the
/// keys `label`, `href`, `group`, `order` and `icon` (the icon's compile-time shapes, or
/// `null`). Nothing about the other links, so a caller who may open none of the links with
/// a permission gets a guest's bytes. Never kept by a cache: `Cache-Control: private,
/// no-store` and `Vary: Cookie, Authorization`.
pub(crate) async fn answer(parts: &mut Parts, links: &Links) -> Response {
    let shown = visible(parts, links).await;
    let items: Vec<serde_json::Value> = shown
        .iter()
        .filter_map(|index| links.0.get(*index))
        .map(|link| {
            serde_json::json!({
                "label": link.label,
                "href": link.href.as_str(),
                "group": link.group,
                "order": link.order,
                "icon": link.icon.map(|icon| {
                    serde_json::json!({ "view_box": icon.view_box(), "shapes": icon.shapes() })
                }),
            })
        })
        .collect();
    let mut response = Json(serde_json::json!({ "items": items })).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    headers.append(
        header::VARY,
        HeaderValue::from_static("Cookie, Authorization"),
    );
    response
}
