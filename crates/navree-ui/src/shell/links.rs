//! The links modules add to the app's sidebar with `m.nav_item`: checked once at boot into
//! [`Links`], in the order the sidebar draws them.

use std::borrow::Cow;

use navree_contracts::permission_catalog;
use navree_kernel::AppContext;

use crate::html::LocalHref;
use crate::icons::Icon;
use crate::shell::ShellNavItem;

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
