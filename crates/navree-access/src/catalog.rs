//! The boot checks of the permissions modules describe with
//! `m.permissions` (navree-contracts' `permission_catalog`): their names, a
//! community plugin's namespace, the names of its subjects and their
//! permission prefixes, and a name two modules describe. Describing
//! a permission grants it to nobody; these checks grant nothing either.
//! They run where the app has AccessModule: without it nothing checks the
//! catalog and nothing reads the descriptions, while a workspace role still
//! holds what `workspace.roles` lists. Then the boot asks each policy of a
//! community plugin what it writes, as each principal it knows, and, once
//! every controller is built, reads what the app's routes require under a
//! community plugin's namespace.

use std::collections::BTreeSet;

use navree_config::Config;
use navree_contracts::permission_catalog;
use navree_http::{HttpOptions, RouteMeta, route_table};
use navree_kernel::{AppContext, ModuleId, PluginReport};
use uuid::Uuid;

use crate::context::AbilityCtx;
use crate::engine::{ALL, Rules};
use crate::error::AccessError;
use crate::options::AccessOptions;
use crate::subject::{PluginPolicy, SubjectRegistration, Subjects};

/// The workspace roles every app has, whether or not its configuration names
/// them.
const BUILT_IN_ROLES: [&str; 4] = ["owner", "admin", "member", "viewer"];

/// The sections of the app's configuration whose keys name roles.
const ROLE_SECTIONS: [&str; 2] = ["workspace.roles.", "workspace.organizations.roles."];

/// Checks the permission catalog of `ctx` at boot, and what the policies of
/// `subjects` that act for community plugins write, in this order, and
/// returns the first error (a community plugin is one the kernel does not
/// find first-party):
///
/// 1. every described name is two or more lower-case words joined by `.`;
/// 2. a community plugin's names start with its namespace and `.`;
/// 3. a subject that a module acting for a community plugin registers is
///    named under the plugin's namespace (the namespace, or the namespace,
///    `.` and a name), and no other module registers a subject of another
///    crate under that namespace; a subject that a module acting for a
///    community plugin registers, whatever crate its type is in, and the
///    plugin's own subjects (types of its crate), wherever registered, take
///    permission prefixes inside its namespace, and its namespace is neither
///    the first word of a permission prefix another module registers for a
///    subject of another crate nor `all`;
/// 4. a name two modules describe is an error when either module belongs to
///    a community plugin, and otherwise a warning at target `navree::access`;
///    a name under a community plugin's namespace that a module not acting
///    for the plugin describes is an error, since the plugin's policies
///    decide the names under its namespace;
/// 5. a community plugin's policy is called as a guest, as a signed-in user
///    with no role, as a user with each of `roles` alone, and as a user with
///    no role who holds every permission the plugin's modules describe: a
///    rule on any subject but the plugin's own (the names under its
///    namespace), `all` included, is an error.
pub(crate) fn check(
    ctx: &AppContext,
    subjects: &Subjects,
    roles: &[String],
) -> Result<(), AccessError> {
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

    let registrations = ctx.contributions::<SubjectRegistration>();
    for contributed in &registrations {
        let registration = &contributed.value;
        let (subject, module) = (registration.subject, contributed.module.name);
        if let Some(plugin) = community_plugin(&plugins, contributed.module.id) {
            // A plugin's subjects are the names under its namespace, so none of
            // them is a subject the app checks, registered or not.
            let namespace = plugin.info.namespace;
            if !is_under(subject, namespace) {
                return Err(AccessError::Config(format!(
                    "plugin `{}` registers the subject `{subject}` in `{module}`, a name outside its namespace `{namespace}`; a plugin's subjects are named under its namespace: remove the plugin or update it to a version that names them under `{namespace}.`, and to register `{subject}`, register it from a module of the app",
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
                    "plugin `{}` uses the namespace `{namespace}`, under which `{module}` registers the subject `{subject}`; the names under a plugin's namespace are its subjects: remove the plugin, or name the app's subject outside `{namespace}.`",
                    plugin.info.name
                )));
            }
        }
    }
    for plugin in plugins.iter().filter(|plugin| !plugin.first_party) {
        let namespace = plugin.info.namespace;
        // A subject whose type lives in the plugin's own crate is its own.
        let own = format!("{}::", plugin.info.crate_name);
        for contributed in &registrations {
            let registration = &contributed.value;
            // What a module acting for the plugin registers is the plugin's, whatever crate
            // its type is in (a `-core` crate, a `Box` of its own type), and so is its own
            // subject, whichever module registers it.
            let of_the_plugin = community_plugin(&plugins, contributed.module.id)
                .is_some_and(|acting| acting.info.name == plugin.info.name)
                || registration.type_name.starts_with(&own);
            if of_the_plugin {
                // A prefix outside its namespace would take over the meaning of permissions
                // the app grants under that prefix.
                if !is_under(registration.prefix, namespace) {
                    let prefix = registration.prefix;
                    return Err(AccessError::Config(format!(
                        "plugin `{}` registers the subject `{}` with the permission prefix `{prefix}`, outside its namespace `{namespace}`; the app's permissions under `{prefix}` would check the plugin's subject: remove the plugin or update it to a version that keeps its permission prefixes under `{namespace}`",
                        plugin.info.name, registration.subject
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

    // A permission that a module not acting for a community plugin describes under the
    // plugin's namespace would be one of the plugin's subjects.
    for (module, permission) in &catalog {
        let acting = community_plugin(&plugins, module.id);
        for plugin in plugins.iter().filter(|plugin| !plugin.first_party) {
            let own = acting.is_some_and(|acting| acting.info.name == plugin.info.name);
            if !own && is_under(permission.name, plugin.info.namespace) {
                let declared = format!(
                    "the permission `{}` that `{}` describes",
                    permission.name, module.name
                );
                return Err(namespace_holds(plugin, &declared));
            }
        }
    }

    for registered in &subjects.policies {
        let Some(policy) = &registered.plugin else {
            continue;
        };
        // What the plugin's modules describe, sorted.
        let described: BTreeSet<String> = catalog
            .iter()
            .filter(|(module, _)| listed_under(&plugins, policy.plugin, module.id))
            .map(|(_, permission)| permission.name.to_owned())
            .collect();
        for probe in probes(subjects, roles, described.into_iter().collect()) {
            let mut rules = Rules::new();
            (registered.define)(&probe, &mut rules);
            if let Some(subject) = rules.rules().iter().find_map(|rule| policy.outside(rule)) {
                return Err(outside_its_subjects(policy, subject));
            }
        }
    }
    Ok(())
}

/// The roles the boot asks a community plugin's policy as, sorted: the
/// built-in workspace roles, every role the app's configuration names under
/// `workspace.roles` and `workspace.organizations.roles` (when the app has
/// one), and the super-admin role of `options`.
pub(crate) async fn probe_roles(ctx: &AppContext, options: &AccessOptions) -> Vec<String> {
    let mut roles: BTreeSet<String> = BUILT_IN_ROLES.into_iter().map(str::to_owned).collect();
    if let Some(config) = ctx.try_get::<Config>().await {
        // `keys` marks nothing as read.
        for key in config.keys() {
            let role = ROLE_SECTIONS
                .into_iter()
                .find_map(|section| key.strip_prefix(section))
                .and_then(|rest| rest.split('.').next())
                .filter(|role| !role.is_empty());
            if let Some(role) = role {
                roles.insert(role.to_owned());
            }
        }
    }
    roles.extend(options.super_admin_role.clone());
    roles.into_iter().collect()
}

/// The principals the boot asks a community plugin's policy as: a guest, a
/// signed-in user with no role, a user with each of `roles` alone, and a user
/// with no role who holds `permissions`. Each user has the nil id, and each
/// principal sees every registered subject's permission prefix.
fn probes(subjects: &Subjects, roles: &[String], permissions: Vec<String>) -> Vec<AbilityCtx> {
    let user = Uuid::nil();
    let mut principals = vec![
        AbilityCtx::guest(),
        AbilityCtx::for_user(user, Vec::new(), Vec::new()),
    ];
    for role in roles {
        principals.push(AbilityCtx::for_user(user, vec![role.clone()], Vec::new()));
    }
    principals.push(AbilityCtx::for_user(user, Vec::new(), permissions));
    principals
        .into_iter()
        .map(|principal| principal.with_subjects(subjects.prefixes.clone()))
        .collect()
}

/// Whether the `plugins` listing lists `module` under the plugin named
/// `plugin`.
fn listed_under(plugins: &[PluginReport], plugin: &str, module: ModuleId) -> bool {
    plugins.iter().any(|report| {
        report.info.name == plugin && report.modules.iter().any(|member| member.id == module)
    })
}

/// The refusal of a community plugin's policy that writes a rule on
/// `subject`, which is not one of the plugin's own.
fn outside_its_subjects(policy: &PluginPolicy, subject: &str) -> AccessError {
    let refusal = format!(
        "plugin `{}` registers the policy `{}` in `{}`, which writes a rule on `{subject}`; a plugin's policy writes rules on its own subjects only: remove the plugin or update it to a version whose policy keeps to them",
        policy.plugin, policy.policy, policy.module
    );
    AccessError::Config(if subject == ALL {
        refusal
    } else {
        format!(
            "{refusal}, and to apply the policy to `{subject}`, register it from a module of the app (AccessModule::for_policy)"
        )
    })
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

/// Checks, once every controller is built, that no route of a module that
/// does not act for a community plugin requires a permission, or checks a
/// subject, under the plugin's namespace: a permission prefix no subject
/// registers stands for itself, and the plugin's policies decide the names
/// under its namespace, so they would decide who may open the route. An app
/// with no community plugin, or without HttpModule, has nothing to check.
/// Mounts are not read.
pub(crate) async fn check_routes(ctx: &AppContext) -> Result<(), AccessError> {
    let plugins = ctx.plugins().plugins;
    if plugins.iter().all(|plugin| plugin.first_party) {
        return Ok(());
    }
    // Without HttpModule the app has no routes.
    if ctx.try_get::<HttpOptions>().await.is_none() {
        return Ok(());
    }
    let routes = route_table(ctx)
        .await
        .map_err(|error| AccessError::Config(error.to_string()))?;
    routes_under_namespaces(&plugins, &routes)
}

/// The first route of `routes` whose module does not act for a community
/// plugin of `plugins` and that requires a permission, or checks a subject,
/// under that plugin's namespace.
fn routes_under_namespaces(
    plugins: &[PluginReport],
    routes: &[RouteMeta],
) -> Result<(), AccessError> {
    for route in routes {
        let acting = route
            .module_id
            .and_then(|module| community_plugin(plugins, module));
        let module = route.module.unwrap_or("?");
        let (method, path) = (&route.method, &route.full_path);
        for plugin in plugins.iter().filter(|plugin| !plugin.first_party) {
            if acting.is_some_and(|acting| acting.info.name == plugin.info.name) {
                continue;
            }
            let namespace = plugin.info.namespace;
            let label = format!("the route `{method} {path}` of `{module}`");
            for permission in &route.access.permissions {
                if is_under(permission, namespace) {
                    let declared = format!("the permission `{permission}` that {label} requires");
                    return Err(namespace_holds(plugin, &declared));
                }
            }
            if let Some(can) = &route.access.can
                && is_under(&can.subject, namespace)
            {
                let subject = &can.subject;
                let declared = format!("the subject `{subject}` that {label} checks");
                return Err(namespace_holds(plugin, &declared));
            }
        }
    }
    Ok(())
}

/// The refusal of the community plugin `plugin`, whose namespace is the
/// first word of `declared` (a permission or a subject, with who declares
/// it): the plugin's policies decide the names under its namespace.
fn namespace_holds(plugin: &PluginReport, declared: &str) -> AccessError {
    AccessError::Config(format!(
        "plugin `{}` uses the namespace `{}`, the first word of {declared}; a plugin's policies decide the names under its namespace, so they would decide who may use it: remove the plugin, or name it under another word in the app",
        plugin.info.name, plugin.info.namespace
    ))
}

/// The refusal of a community plugin whose namespace is the permission
/// prefix of `subject`, which `module` registers.
fn namespace_taken(plugin: &PluginReport, subject: &str, module: &str) -> AccessError {
    AccessError::Config(format!(
        "plugin `{}` uses the namespace `{}`, which is the permission prefix of `{subject}` (`{module}`); a plugin's policies decide the names under its namespace: remove the plugin, or give the app's subject a permission prefix under another word",
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
