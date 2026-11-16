//! The boot checks of the permissions modules describe with
//! `m.permissions` (navree-contracts' `permission_catalog`): their names, a
//! community plugin's namespace, the names of its subjects and their
//! permission prefixes, and a name two modules describe. Describing
//! a permission grants it to nobody; these checks grant nothing either.
//! They run where the app has AccessModule: without it nothing checks the
//! catalog and nothing reads the descriptions, while a workspace role still
//! holds what `workspace.roles` lists.

use navree_contracts::permission_catalog;
use navree_kernel::{AppContext, ModuleId, PluginReport};

use crate::engine::ALL;
use crate::error::AccessError;
use crate::subject::SubjectRegistration;

/// Checks the permission catalog of `ctx` at boot, in this order, and
/// returns the first error (a community plugin is one the kernel does not
/// find first-party):
///
/// 1. every described name is two or more lower-case words joined by `.`;
/// 2. a community plugin's names start with its namespace and `.`;
/// 3. a subject that a module acting for a community plugin registers is
///    named under the plugin's namespace (the namespace, or the namespace,
///    `.` and a name), and no other module registers a subject of another
///    crate under that namespace; a community plugin's own subjects (types
///    of its crate) take permission prefixes inside its namespace, and its
///    namespace is neither the first word of a permission prefix a subject
///    of another crate registers nor `all`;
/// 4. a name two modules describe is an error when either module belongs to
///    a community plugin, and otherwise a warning at target `navree::access`.
pub(crate) fn check(ctx: &AppContext) -> Result<(), AccessError> {
    let catalog = permission_catalog(ctx);
    let plugins = ctx.plugins().plugins;

    for (module, permission) in &catalog {
        if !is_permission_name(permission.name) {
            return Err(AccessError::Config(format!(
                "permission `{}` declared by `{}` is not two or more lower-case words joined by `.`",
                permission.name, module.name
            )));
        }
    }

    for (module, permission) in &catalog {
        let Some(plugin) = community_plugin(&plugins, module.id) else {
            continue;
        };
        let namespace = plugin.info.namespace;
        if !permission.name.starts_with(&format!("{namespace}.")) {
            return Err(AccessError::Config(format!(
                "plugin `{}` declares the permission `{}`, which does not start with `{namespace}.`",
                plugin.info.name, permission.name
            )));
        }
    }

    let subjects = ctx.contributions::<SubjectRegistration>();
    for contributed in &subjects {
        let registration = &contributed.value;
        let (subject, module) = (registration.subject, contributed.module.name);
        if let Some(plugin) = community_plugin(&plugins, contributed.module.id) {
            // A plugin's subjects are the names under its namespace, so none of
            // them is a subject the app checks, registered or not.
            let namespace = plugin.info.namespace;
            if !is_under(subject, namespace) {
                return Err(AccessError::Config(format!(
                    "plugin `{}` registers the subject `{subject}` in `{module}`, a name outside its namespace; name it `{namespace}.{subject}`",
                    plugin.info.name
                )));
            }
            continue;
        }
        // Under a plugin's namespace: the plugin's subjects only, and the app's
        // registrations of them.
        for plugin in plugins.iter().filter(|plugin| !plugin.first_party) {
            let namespace = plugin.info.namespace;
            let of_the_plugin = registration
                .type_name
                .strip_prefix(plugin.info.crate_name)
                .is_some_and(|rest| rest.starts_with("::"));
            if !of_the_plugin && is_under(subject, namespace) {
                return Err(AccessError::Config(format!(
                    "plugin `{}` uses the namespace `{namespace}`, under which `{module}` registers the subject `{subject}`; set another with plugin_info!(namespace = \"..\")",
                    plugin.info.name
                )));
            }
        }
    }
    for plugin in plugins.iter().filter(|plugin| !plugin.first_party) {
        let namespace = plugin.info.namespace;
        // A subject whose type lives in the plugin's own crate is its own.
        let own = format!("{}::", plugin.info.crate_name);
        for contributed in &subjects {
            let registration = &contributed.value;
            if registration.type_name.starts_with(&own) {
                // Its own subject: a prefix outside its namespace would take over the
                // meaning of permissions the app grants under that prefix.
                let inside = registration.prefix == namespace
                    || registration.prefix.starts_with(&format!("{namespace}."));
                if !inside {
                    return Err(AccessError::Config(format!(
                        "plugin `{}` registers the subject `{}` with the permission prefix `{}`, outside its namespace `{namespace}`",
                        plugin.info.name, registration.subject, registration.prefix
                    )));
                }
            } else if registration.prefix.split('.').next() == Some(namespace) {
                return Err(namespace_taken(
                    plugin,
                    registration.subject,
                    contributed.module.name,
                ));
            }
        }
        // `all`: navree-access's subject for everything.
        if namespace == ALL {
            return Err(namespace_taken(plugin, ALL, "AccessModule"));
        }
    }

    for (index, (first, permission)) in catalog.iter().enumerate() {
        for (second, other) in &catalog[index + 1..] {
            if other.name != permission.name {
                continue;
            }
            let both = format!(
                "permission `{}` is declared by both `{}` and `{}`",
                permission.name, first.name, second.name
            );
            if community_plugin(&plugins, first.id).is_some()
                || community_plugin(&plugins, second.id).is_some()
            {
                return Err(AccessError::Config(both));
            }
            let message = format!("{both}; this becomes an error in 2.0");
            tracing::warn!(target: "navree::access", "{message}");
        }
    }
    Ok(())
}

/// The community plugin `module` acts for, if it acts for one: the plugin the
/// `plugins` listing lists it under, when the kernel does not find that plugin
/// first-party.
pub(crate) fn community_plugin(
    plugins: &[PluginReport],
    module: ModuleId,
) -> Option<&PluginReport> {
    plugins.iter().find(|plugin| {
        !plugin.first_party && plugin.modules.iter().any(|member| member.id == module)
    })
}

/// Whether `name` is under `namespace`: the namespace itself, or the
/// namespace, `.` and more.
pub(crate) fn is_under(name: &str, namespace: &str) -> bool {
    name.strip_prefix(namespace)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// The refusal of a community plugin whose namespace is the permission
/// prefix of `subject`, which `module` registers.
fn namespace_taken(plugin: &PluginReport, subject: &str, module: &str) -> AccessError {
    AccessError::Config(format!(
        "plugin `{}` uses the namespace `{}`, which is the permission prefix of `{subject}` (`{module}`); set another with plugin_info!(namespace = \"..\")",
        plugin.info.name, plugin.info.namespace
    ))
}

/// Whether `name` is two or more words joined by `.`, each a lower-case
/// letter followed by up to 63 lower-case letters, digits or `_`.
fn is_permission_name(name: &str) -> bool {
    let mut words = 0;
    for word in name.split('.') {
        let mut bytes = word.bytes();
        let starts_with_a_letter = matches!(bytes.next(), Some(b'a'..=b'z'));
        if !starts_with_a_letter
            || word.len() > 64
            || !bytes.all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
        {
            return false;
        }
        words += 1;
    }
    words >= 2
}

#[cfg(test)]
mod tests {
    use super::{is_permission_name, is_under};

    #[test]
    fn a_name_is_under_a_namespace_it_is_or_starts_with_and_a_dot() {
        for name in [
            "announcements",
            "announcements.Announcement",
            "announcements.notice_board.Notice",
        ] {
            assert!(is_under(name, "announcements"), "{name:?} is under it");
        }
        for name in [
            "Announcement",
            "announcements_extra.Notice",
            "announcementsNotice",
            "reports.announcements",
            "all",
            "",
        ] {
            assert!(!is_under(name, "announcements"), "{name:?} is not under it");
        }
    }

    #[test]
    fn permission_names_are_two_or_more_lower_case_words() {
        let longest = format!("{}.read", "a".repeat(64));
        for name in [
            "announcements.read",
            "announcements.notice_board.publish",
            "a1.b2",
            longest.as_str(),
        ] {
            assert!(is_permission_name(name), "{name:?} is a permission name");
        }
        let too_long = format!("{}.read", "a".repeat(65));
        for name in [
            "announcements",
            "Announcements.read",
            "announcements..read",
            "announcements.read-all",
            "announcements.*",
            "1notices.read",
            "announcements.read.",
            "announcements. read",
            "",
            too_long.as_str(),
        ] {
            assert!(
                !is_permission_name(name),
                "{name:?} is not a permission name"
            );
        }
    }
}
