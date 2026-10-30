//! Who may do what in the CRM: the abilities function the
//! access module calls for every principal, compiled into CASL rules served
//! at `GET /api/me/abilities` and enforced by `#[can]` routes and
//! `Accessible` list filters.
//!
//! | Role | Rules |
//! |---|---|
//! | `admin` | super-admin: `can manage all`, added last by the access module (so it beats every `cannot`) |
//! | `manager` | its permissions (`companies.manage`, `contacts.manage`, `contacts.export`, `deals.manage`, `activities.manage`, `users.read`), then the base rules |
//! | `rep` | the base rules |
//! | no CRM role, guests | nothing |
//!
//! The base rules: read Company, Contact, Deal, Activity and User; create
//! Company, Contact, Deal and Activity; update and delete the Companies,
//! Contacts and Activities they own (`owner_id`); complete Activities
//! assigned to them (`assignee_id`) or owned by them; update, move and
//! delete the Deals they own; for users who cannot `manage` deals,
//! `cannot move Deal` when `stage` is `won` or `lost` (reopening a closed
//! deal takes a manager, or a rep could reopen it and then delete it);
//! and, last (the last matching rule wins), `cannot delete Deal` when
//! `stage` is `won` or `lost`.

use navree::access::{AbilityCtx, AccessOptions, CREATE, DELETE, READ, Rules, UPDATE, field};

use crate::actions::{COMPLETE, MOVE};
use crate::enums::Stage;
use crate::permissions::DEALS_MANAGE;
use crate::roles::{ADMIN, MANAGER, REP};
use crate::subjects::{ACTIVITY, COMPANY, CONTACT, DEAL, USER};

/// Why won and lost deals cannot be deleted (the CASL rule's `reason`).
pub const CLOSED_DEAL_REASON: &str = "Won and lost deals are kept for reporting.";

/// Why reps cannot move won and lost deals (the CASL rule's `reason`).
pub const CLOSED_DEAL_MOVE_REASON: &str =
    "Won and lost deals are kept for reporting; only managers reopen them.";

/// The CRM's rules for `ctx`; see the [module docs](self).
pub fn define(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    let Some(user) = &ctx.user else {
        return;
    };
    // The super-admin rule (`manage all`) comes from the access module.
    if user.has_role(ADMIN) || !user.has_any_role(&[MANAGER, REP]) {
        return;
    }
    for subject in [COMPANY, CONTACT, DEAL, ACTIVITY, USER] {
        rules.can(READ, subject);
    }
    for subject in [COMPANY, CONTACT, DEAL, ACTIVITY] {
        rules.can(CREATE, subject);
    }
    let own = || field("owner_id").eq(user.id);
    for subject in [COMPANY, CONTACT, ACTIVITY] {
        rules.can(UPDATE, subject).when(own());
        rules.can(DELETE, subject).when(own());
    }
    rules
        .can(COMPLETE, ACTIVITY)
        .when(field("assignee_id").eq(user.id));
    rules.can(COMPLETE, ACTIVITY).when(own());
    rules.can(UPDATE, DEAL).when(own());
    rules.can(MOVE, DEAL).when(own());
    rules.can(DELETE, DEAL).when(own());
    let closed = || field("stage").is_in([Stage::Won.as_str(), Stage::Lost.as_str()]);
    if !manages_deals(ctx) {
        // The delete rule below looks at the current stage only: moving a
        // closed deal back to an open stage would get around it.
        rules
            .cannot(MOVE, DEAL)
            .when(closed())
            .because(CLOSED_DEAL_MOVE_REASON);
    }
    rules
        .cannot(DELETE, DEAL)
        .when(closed())
        .because(CLOSED_DEAL_REASON);
}

/// Whether `ctx`'s permissions include `manage` on deals (`deals.manage`
/// or `deals.*`), as the `manager` role's do.
fn manages_deals(ctx: &AbilityCtx) -> bool {
    ctx.permissions
        .iter()
        .any(|permission| permission == DEALS_MANAGE || permission == "deals.*")
}

/// The access module's options: [`define`], with `admin` as the
/// super-admin role.
pub fn access_options() -> AccessOptions {
    AccessOptions {
        abilities: define,
        super_admin_role: Some(ADMIN.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use navree::access::{ALL, Ability, AbilityCtx, MANAGE, Rules};
    use serde_json::{Value, json};
    use uuid::Uuid;

    use super::*;
    use crate::permissions;
    use crate::roles::{ADMIN, MANAGER, REP};

    /// The permission prefixes the CRM registers (`for_feature` on every
    /// model, navree-auth's users included).
    fn prefixes() -> Arc<BTreeMap<String, String>> {
        Arc::new(
            [
                ("companies", "Company"),
                ("contacts", "Contact"),
                ("deals", "Deal"),
                ("activities", "Activity"),
                ("users", "User"),
            ]
            .into_iter()
            .map(|(prefix, subject)| (prefix.to_owned(), subject.to_owned()))
            .collect(),
        )
    }

    /// Who is asking in a row of the table.
    #[derive(Clone, Copy, Debug)]
    enum Who {
        Admin,
        Manager,
        Rep,
        /// Signed in, but without a CRM role.
        NoRole,
        Guest,
    }

    const EVERYONE: [Who; 5] = [Who::Admin, Who::Manager, Who::Rep, Who::NoRole, Who::Guest];

    const ME: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0001);
    const SOMEONE_ELSE: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0002);

    /// The context the access module builds for `who` (manager permissions
    /// as the `crm-roles` seeder grants them), and the ability it compiles:
    /// `define`, then, for the super-admin role, `can manage all` last.
    fn ability(who: Who) -> Ability {
        let (roles, permissions): (Vec<&str>, Vec<&str>) = match who {
            Who::Admin => (vec![ADMIN], vec![]),
            Who::Manager => (vec![MANAGER], permissions::MANAGER.to_vec()),
            Who::Rep => (vec![REP], vec![]),
            Who::NoRole => (vec![], vec![]),
            Who::Guest => return build(&AbilityCtx::guest().with_subjects(prefixes())),
        };
        let ctx = AbilityCtx::for_user(
            ME,
            roles.into_iter().map(str::to_owned).collect(),
            permissions.into_iter().map(str::to_owned).collect(),
        )
        .with_subjects(prefixes());
        build(&ctx)
    }

    fn build(ctx: &AbilityCtx) -> Ability {
        let options = access_options();
        let mut rules = Rules::new();
        (options.abilities)(ctx, &mut rules);
        if let Some(role) = &options.super_admin_role
            && ctx.has_role(role)
        {
            rules.can(MANAGE, ALL).because("super admin");
        }
        rules.build()
    }

    /// The record a row checks: `None` for a type-level check.
    #[derive(Clone, Copy, Debug)]
    enum Rec {
        Type,
        Own,
        Other,
        /// Owned by someone else, assigned to me.
        Assigned,
        OwnWon,
        OwnLost,
        OtherWon,
        OwnNegotiation,
    }

    fn record(rec: Rec) -> Option<Value> {
        let row = |owner: Uuid, assignee: Option<Uuid>, stage: &str| {
            json!({
                "id": Uuid::now_v7(),
                "owner_id": owner,
                "assignee_id": assignee,
                "stage": stage,
            })
        };
        Some(match rec {
            Rec::Type => return None,
            Rec::Own => row(ME, None, "lead"),
            Rec::Other => row(SOMEONE_ELSE, None, "lead"),
            Rec::Assigned => row(SOMEONE_ELSE, Some(ME), "lead"),
            Rec::OwnWon => row(ME, None, "won"),
            Rec::OwnLost => row(ME, None, "lost"),
            Rec::OtherWon => row(SOMEONE_ELSE, None, "won"),
            Rec::OwnNegotiation => row(ME, None, "negotiation"),
        })
    }

    /// `(action, subject, record, [admin, manager, rep, no role, guest])`.
    type Row = (&'static str, &'static str, Rec, [bool; 5]);

    const Y: bool = true;
    const N: bool = false;

    /// Admin is super-admin; managers manage every customer
    /// record, export contacts and read users; reps read everything, create
    /// records, change only what they own, complete their own or assigned
    /// activities; nobody but admin deletes a won or lost deal.
    #[rustfmt::skip]
    const TABLE: &[Row] = &[
        // Companies.
        ("read",     "Company",  Rec::Other,    [Y, Y, Y, N, N]),
        ("create",   "Company",  Rec::Type,     [Y, Y, Y, N, N]),
        ("update",   "Company",  Rec::Own,      [Y, Y, Y, N, N]),
        ("update",   "Company",  Rec::Other,    [Y, Y, N, N, N]),
        ("delete",   "Company",  Rec::Own,      [Y, Y, Y, N, N]),
        ("delete",   "Company",  Rec::Other,    [Y, Y, N, N, N]),
        ("manage",   "Company",  Rec::Type,     [Y, Y, N, N, N]),
        // Contacts.
        ("read",     "Contact",  Rec::Other,    [Y, Y, Y, N, N]),
        ("create",   "Contact",  Rec::Type,     [Y, Y, Y, N, N]),
        ("update",   "Contact",  Rec::Own,      [Y, Y, Y, N, N]),
        ("update",   "Contact",  Rec::Other,    [Y, Y, N, N, N]),
        ("delete",   "Contact",  Rec::Own,      [Y, Y, Y, N, N]),
        ("delete",   "Contact",  Rec::Other,    [Y, Y, N, N, N]),
        ("export",   "Contact",  Rec::Type,     [Y, Y, N, N, N]),
        ("manage",   "Contact",  Rec::Type,     [Y, Y, N, N, N]),
        // Deals.
        ("read",     "Deal",     Rec::Other,    [Y, Y, Y, N, N]),
        ("create",   "Deal",     Rec::Type,     [Y, Y, Y, N, N]),
        ("update",   "Deal",     Rec::Own,      [Y, Y, Y, N, N]),
        ("update",   "Deal",     Rec::Other,    [Y, Y, N, N, N]),
        ("update",   "Deal",     Rec::OwnWon,   [Y, Y, Y, N, N]),
        ("move",     "Deal",     Rec::Own,      [Y, Y, Y, N, N]),
        ("move",     "Deal",     Rec::Other,    [Y, Y, N, N, N]),
        // Reopening a closed deal (any move of it) takes a manager.
        ("move",     "Deal",     Rec::OwnWon,   [Y, Y, N, N, N]),
        ("move",     "Deal",     Rec::OwnLost,  [Y, Y, N, N, N]),
        ("move",     "Deal",     Rec::OwnNegotiation, [Y, Y, Y, N, N]),
        ("delete",   "Deal",     Rec::Own,      [Y, Y, Y, N, N]),
        ("delete",   "Deal",     Rec::OwnNegotiation, [Y, Y, Y, N, N]),
        ("delete",   "Deal",     Rec::Other,    [Y, Y, N, N, N]),
        ("delete",   "Deal",     Rec::OwnWon,   [Y, N, N, N, N]),
        ("delete",   "Deal",     Rec::OwnLost,  [Y, N, N, N, N]),
        ("delete",   "Deal",     Rec::OtherWon, [Y, N, N, N, N]),
        ("manage",   "Deal",     Rec::Type,     [Y, Y, N, N, N]),
        // Activities.
        ("read",     "Activity", Rec::Other,    [Y, Y, Y, N, N]),
        ("create",   "Activity", Rec::Type,     [Y, Y, Y, N, N]),
        ("update",   "Activity", Rec::Own,      [Y, Y, Y, N, N]),
        ("update",   "Activity", Rec::Other,    [Y, Y, N, N, N]),
        ("update",   "Activity", Rec::Assigned, [Y, Y, N, N, N]),
        ("delete",   "Activity", Rec::Own,      [Y, Y, Y, N, N]),
        ("delete",   "Activity", Rec::Other,    [Y, Y, N, N, N]),
        ("complete", "Activity", Rec::Own,      [Y, Y, Y, N, N]),
        ("complete", "Activity", Rec::Assigned, [Y, Y, Y, N, N]),
        ("complete", "Activity", Rec::Other,    [Y, Y, N, N, N]),
        ("manage",   "Activity", Rec::Type,     [Y, Y, N, N, N]),
        // Users.
        ("read",     "User",     Rec::Other,    [Y, Y, Y, N, N]),
        ("create",   "User",     Rec::Type,     [Y, N, N, N, N]),
        ("update",   "User",     Rec::Other,    [Y, N, N, N, N]),
        ("delete",   "User",     Rec::Other,    [Y, N, N, N, N]),
        ("manage",   "User",     Rec::Type,     [Y, N, N, N, N]),
        // Everything else (the audit log, role management) is admin only.
        ("read",     "audit",    Rec::Type,     [Y, N, N, N, N]),
        ("manage",   "roles",    Rec::Type,     [Y, N, N, N, N]),
        ("manage",   "all",      Rec::Type,     [Y, N, N, N, N]),
        // Reps get custom actions only on their subjects; `manage` (the
        // manager's permissions) covers every action, custom ones included.
        ("move",     "Company",  Rec::Own,      [Y, Y, N, N, N]),
        ("export",   "Deal",     Rec::Type,     [Y, Y, N, N, N]),
        ("complete", "Deal",     Rec::Own,      [Y, Y, N, N, N]),
    ];

    #[test]
    fn rule_table_for_every_role() {
        let mut failures = Vec::new();
        for who in EVERYONE {
            let ability = ability(who);
            for (action, subject, rec, expected) in TABLE {
                let expected = expected[who as usize];
                let actual = ability.can(action, subject, record(*rec).as_ref());
                if actual != expected {
                    failures.push(format!(
                        "{who:?} {action} {subject} ({rec:?}): expected {expected}, got {actual}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn won_and_lost_deals_are_protected_by_the_last_rule() {
        let ability = ability(Who::Rep);
        let last = ability.rules().last().expect("rules");
        assert!(last.inverted, "{last:?}");
        assert_eq!(
            ability
                .to_casl_json()
                .as_array()
                .and_then(|rules| rules.last())
                .cloned(),
            Some(json!({
                "action": "delete",
                "subject": "Deal",
                "conditions": { "stage": { "$in": ["won", "lost"] } },
                "inverted": true,
                "reason": "Won and lost deals are kept for reporting.",
            }))
        );
        let manager = ability_rules(Who::Manager);
        assert_eq!(
            manager.last().map(|rule| rule["inverted"].clone()),
            Some(json!(true))
        );
    }

    fn ability_rules(who: Who) -> Vec<Value> {
        ability(who)
            .to_casl_json()
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn admin_rules_are_manage_all_only() {
        assert_eq!(
            ability_rules(Who::Admin),
            [json!({ "action": "manage", "subject": "all", "reason": "super admin" })]
        );
    }

    #[test]
    fn guests_and_users_without_a_role_get_no_rules() {
        assert!(ability_rules(Who::Guest).is_empty());
        assert!(ability_rules(Who::NoRole).is_empty());
    }

    #[test]
    fn manager_rules_start_with_the_granted_permissions() {
        let rules = ability_rules(Who::Manager);
        let granted: Vec<(String, String)> = rules
            .iter()
            .take(6)
            .map(|rule| {
                (
                    rule["action"].as_str().unwrap_or_default().to_owned(),
                    rule["subject"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        let mut granted_sorted = granted.clone();
        granted_sorted.sort();
        assert_eq!(
            granted_sorted,
            [
                ("export", "Contact"),
                ("manage", "Activity"),
                ("manage", "Company"),
                ("manage", "Contact"),
                ("manage", "Deal"),
                ("read", "User"),
            ]
            .map(|(a, s)| (a.to_owned(), s.to_owned()))
        );
    }

    #[test]
    fn options_make_admin_the_super_admin() {
        let options = access_options();
        assert_eq!(options.super_admin_role.as_deref(), Some("admin"));
    }
}
