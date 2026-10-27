//! The diff of a run: what the source [pulled](crate::Pulled) against what
//! the directory holds, as a [`Plan`] of user and group steps.

use std::collections::{BTreeSet, HashMap, HashSet};

use uuid::Uuid;

use crate::pulled::Pulled;
use crate::types::{ChangeAction, RunSummary};
use crate::user::{DirectoryGroup, DirectoryUser};

/// A user the directory holds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CurrentUser {
    pub(crate) id: Uuid,
    pub(crate) user: DirectoryUser,
    pub(crate) fingerprint: String,
    pub(crate) local_id: Option<String>,
}

/// A group the directory holds, with its members' external ids.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CurrentGroup {
    pub(crate) id: Uuid,
    pub(crate) group: DirectoryGroup,
    pub(crate) fingerprint: String,
    pub(crate) local_id: Option<String>,
}

/// What the directory holds for a connection, by external id.
#[derive(Clone, Debug, Default)]
pub(crate) struct Current {
    pub(crate) users: HashMap<String, CurrentUser>,
    pub(crate) groups: HashMap<String, CurrentGroup>,
}

impl Current {
    pub(crate) fn active_users(&self) -> usize {
        self.users.values().filter(|user| user.user.active).count()
    }
}

/// A change to one user.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct UserStep {
    pub(crate) action: ChangeAction,
    /// The user after the change.
    pub(crate) user: DirectoryUser,
    /// The user before it.
    pub(crate) previous: Option<CurrentUser>,
    /// The fields that change.
    pub(crate) changed: Vec<String>,
}

/// A change to one group (its members as external ids).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GroupStep {
    pub(crate) action: ChangeAction,
    /// The group after the change, with its complete member list.
    pub(crate) group: DirectoryGroup,
    pub(crate) previous: Option<CurrentGroup>,
    pub(crate) changed: Vec<String>,
    pub(crate) added: Vec<String>,
    pub(crate) removed: Vec<String>,
}

/// The steps of a run, in the order they apply: users created, updated
/// and reactivated; groups created and updated; groups deleted; users
/// deactivated (after the groups, so group changes see them as members
/// for the last time).
#[derive(Clone, Debug, Default)]
pub(crate) struct Plan {
    pub(crate) users: Vec<UserStep>,
    pub(crate) groups: Vec<GroupStep>,
    pub(crate) deleted_groups: Vec<GroupStep>,
    pub(crate) deactivations: Vec<UserStep>,
    pub(crate) summary: RunSummary,
}

impl Plan {
    pub(crate) fn len(&self) -> usize {
        self.users.len() + self.groups.len() + self.deleted_groups.len() + self.deactivations.len()
    }
}

/// Diffs `pulled` against `current`.
pub(crate) fn diff(current: &Current, pulled: &Pulled) -> Plan {
    let mut plan = Plan::default();
    let summary = &mut plan.summary;

    // -- users ---------------------------------------------------------------
    let removed: HashSet<&str> = if pulled.complete {
        HashSet::new()
    } else {
        pulled.removed_users.iter().map(String::as_str).collect()
    };
    let mut seen: Vec<&DirectoryUser> = Vec::new();
    let mut seen_ids: HashMap<&str, usize> = HashMap::new();
    for user in &pulled.users {
        if user.external_id.trim().is_empty() || user.user_name.trim().is_empty() {
            summary.skipped += 1;
            continue;
        }
        // The last version of a user the source gives twice wins.
        match seen_ids.get(user.external_id.as_str()) {
            Some(&at) => seen[at] = user,
            None => {
                seen_ids.insert(&user.external_id, seen.len());
                seen.push(user);
            }
        }
    }
    summary.users_seen = seen.len() as u64;
    for user in &seen {
        if removed.contains(user.external_id.as_str()) {
            continue;
        }
        let Some(previous) = current.users.get(&user.external_id) else {
            plan.users.push(UserStep {
                action: ChangeAction::Create,
                user: (*user).clone(),
                previous: None,
                changed: Vec::new(),
            });
            summary.users_created += 1;
            continue;
        };
        if previous.fingerprint == user.fingerprint() {
            summary.users_unchanged += 1;
            continue;
        }
        let changed = user.changed_fields(&previous.user);
        let step = |action| UserStep {
            action,
            user: (*user).clone(),
            previous: Some(previous.clone()),
            changed: changed.clone(),
        };
        match (previous.user.active, user.active) {
            (false, true) => {
                plan.users.push(step(ChangeAction::Reactivate));
                summary.users_reactivated += 1;
            }
            (true, false) => {
                plan.deactivations.push(step(ChangeAction::Deactivate));
                summary.users_deactivated += 1;
            }
            _ => {
                plan.users.push(step(ChangeAction::Update));
                summary.users_updated += 1;
            }
        }
    }
    let gone: Vec<&CurrentUser> = if pulled.complete {
        let mut gone: Vec<&CurrentUser> = current
            .users
            .values()
            .filter(|user| user.user.active && !seen_ids.contains_key(user.user.external_id.as_str()))
            .collect();
        gone.sort_by(|a, b| a.user.external_id.cmp(&b.user.external_id));
        gone
    } else {
        let mut ids: Vec<&str> = removed.iter().copied().collect();
        ids.sort_unstable();
        ids.into_iter()
            .filter_map(|id| current.users.get(id))
            .filter(|user| user.user.active)
            .collect()
    };
    for previous in gone {
        let mut user = previous.user.clone();
        user.active = false;
        plan.deactivations.push(UserStep {
            action: ChangeAction::Deactivate,
            user,
            previous: Some(previous.clone()),
            changed: vec!["active".to_owned()],
        });
        summary.users_deactivated += 1;
    }

    // -- groups --------------------------------------------------------------
    // Members must be users of this pull (snapshot) or of the directory.
    let known = |id: &str| -> bool {
        seen_ids.contains_key(id) || (!pulled.complete && current.users.contains_key(id))
    };
    let removed_groups: HashSet<&str> = if pulled.complete {
        HashSet::new()
    } else {
        pulled.removed_groups.iter().map(String::as_str).collect()
    };
    let mut groups: Vec<&DirectoryGroup> = Vec::new();
    let mut group_ids: HashMap<&str, usize> = HashMap::new();
    for group in &pulled.groups {
        if group.external_id.trim().is_empty() || group.display_name.trim().is_empty() {
            summary.skipped += 1;
            continue;
        }
        match group_ids.get(group.external_id.as_str()) {
            Some(&at) => groups[at] = group,
            None => {
                group_ids.insert(&group.external_id, groups.len());
                groups.push(group);
            }
        }
    }
    summary.groups_seen = groups.len() as u64;

    // Member changes of a delta, by group.
    let mut member_changes: HashMap<&str, (Vec<&str>, Vec<&str>)> = HashMap::new();
    let mut delta_groups: Vec<&str> = Vec::new();
    if !pulled.complete {
        for change in &pulled.member_changes {
            let entry = member_changes.entry(change.group.as_str()).or_insert_with(|| {
                delta_groups.push(change.group.as_str());
                (Vec::new(), Vec::new())
            });
            if change.added {
                entry.0.push(&change.user);
            } else {
                entry.1.push(&change.user);
            }
        }
    }

    let mut unknown_members = 0_u64;
    let mut desired_members =
        |base: BTreeSet<String>, group: &str, listed: Option<&DirectoryGroup>| -> BTreeSet<String> {
            if pulled.complete {
                let listed = listed.map(|g| g.members.as_slice()).unwrap_or_default();
                let mut members = BTreeSet::new();
                for member in listed {
                    if known(member) {
                        members.insert(member.clone());
                    } else {
                        unknown_members += 1;
                    }
                }
                return members;
            }
            let mut members = base;
            if let Some((added, removed)) = member_changes.get(group) {
                for member in added {
                    if known(member) {
                        members.insert((*member).to_owned());
                    } else {
                        unknown_members += 1;
                    }
                }
                for member in removed {
                    members.remove(*member);
                }
            }
            members
        };

    let mut handled: HashSet<&str> = HashSet::new();
    let ordered: Vec<(&str, Option<&DirectoryGroup>)> = groups
        .iter()
        .map(|group| (group.external_id.as_str(), Some(*group)))
        .chain(
            delta_groups
                .iter()
                .filter(|id| !group_ids.contains_key(**id))
                .map(|id| (*id, None)),
        )
        .collect();
    for (id, listed) in ordered {
        if removed_groups.contains(id) || !handled.insert(id) {
            continue;
        }
        let previous = current.groups.get(id);
        let Some(attributes) = listed
            .map(DirectoryGroup::without_members)
            .or_else(|| previous.map(|p| p.group.without_members()))
        else {
            // Member changes of a group neither listed nor known.
            continue;
        };
        let base: BTreeSet<String> = previous
            .map(|p| p.group.members.iter().cloned().collect())
            .unwrap_or_default();
        let members = desired_members(base.clone(), id, listed);
        let added: Vec<String> = members.difference(&base).cloned().collect();
        let gone: Vec<String> = base.difference(&members).cloned().collect();
        let mut group = attributes;
        group.members = members.into_iter().collect();
        match previous {
            None => {
                summary.groups_created += 1;
                summary.memberships_added += added.len() as u64;
                plan.groups.push(GroupStep {
                    action: ChangeAction::Create,
                    group,
                    previous: None,
                    changed: Vec::new(),
                    added,
                    removed: Vec::new(),
                });
            }
            Some(previous) => {
                let mut changed = if previous.fingerprint == group.fingerprint() {
                    Vec::new()
                } else {
                    group.changed_fields(&previous.group)
                };
                if !added.is_empty() || !gone.is_empty() {
                    changed.push("members".to_owned());
                }
                if changed.is_empty() {
                    summary.groups_unchanged += 1;
                    continue;
                }
                summary.groups_updated += 1;
                summary.memberships_added += added.len() as u64;
                summary.memberships_removed += gone.len() as u64;
                plan.groups.push(GroupStep {
                    action: ChangeAction::Update,
                    group,
                    previous: Some(previous.clone()),
                    changed,
                    added,
                    removed: gone,
                });
            }
        }
    }
    summary.unknown_members = unknown_members;

    let deleted: Vec<&CurrentGroup> = if pulled.complete {
        let mut deleted: Vec<&CurrentGroup> = current
            .groups
            .values()
            .filter(|group| !group_ids.contains_key(group.group.external_id.as_str()))
            .collect();
        deleted.sort_by(|a, b| a.group.external_id.cmp(&b.group.external_id));
        deleted
    } else {
        let mut ids: Vec<&str> = removed_groups.iter().copied().collect();
        ids.sort_unstable();
        ids.into_iter().filter_map(|id| current.groups.get(id)).collect()
    };
    for previous in deleted {
        summary.groups_deleted += 1;
        summary.memberships_removed += previous.group.members.len() as u64;
        plan.deleted_groups.push(GroupStep {
            action: ChangeAction::Delete,
            group: previous.group.clone(),
            previous: Some(previous.clone()),
            changed: Vec::new(),
            added: Vec::new(),
            removed: previous.group.members.clone(),
        });
    }
    plan
}

/// Whether applying `plan` would deactivate more users than the limit
/// allows: more than `min_users` users, and more than `percent` percent of
/// the active ones.
pub(crate) fn exceeds_deactivation_limit(
    plan: &Plan,
    current: &Current,
    percent: u8,
    min_users: u32,
) -> bool {
    let deactivations = plan.deactivations.len();
    if deactivations <= min_users as usize || percent >= 100 {
        return false;
    }
    let active = current.active_users().max(1);
    deactivations * 100 > active * usize::from(percent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str) -> DirectoryUser {
        DirectoryUser::new(id, format!("{id}@acme.test"))
    }

    fn held(users: &[DirectoryUser], groups: &[DirectoryGroup]) -> Current {
        let mut current = Current::default();
        for (n, user) in users.iter().enumerate() {
            current.users.insert(
                user.external_id.clone(),
                CurrentUser {
                    id: Uuid::from_u128(n as u128 + 1),
                    user: user.clone(),
                    fingerprint: user.fingerprint(),
                    local_id: None,
                },
            );
        }
        for (n, group) in groups.iter().enumerate() {
            current.groups.insert(
                group.external_id.clone(),
                CurrentGroup {
                    id: Uuid::from_u128(n as u128 + 1000),
                    group: group.clone(),
                    fingerprint: group.fingerprint(),
                    local_id: None,
                },
            );
        }
        current
    }

    fn actions(steps: &[UserStep]) -> Vec<(ChangeAction, &str)> {
        steps
            .iter()
            .map(|step| (step.action, step.user.external_id.as_str()))
            .collect()
    }

    #[test]
    fn a_first_snapshot_creates_everyone() {
        let pulled = Pulled::snapshot(
            vec![user("u1"), user("u2").inactive()],
            vec![DirectoryGroup::new("g1", "Engineering").members(["u1", "u2", "ghost"])],
        );
        let plan = diff(&Current::default(), &pulled);
        assert_eq!(
            actions(&plan.users),
            vec![(ChangeAction::Create, "u1"), (ChangeAction::Create, "u2")]
        );
        assert!(!plan.users[1].user.active);
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].group.members, vec!["u1", "u2"]);
        assert_eq!(plan.groups[0].added, vec!["u1", "u2"]);
        assert_eq!(plan.summary.users_created, 2);
        assert_eq!(plan.summary.groups_created, 1);
        assert_eq!(plan.summary.memberships_added, 2);
        assert_eq!(plan.summary.unknown_members, 1);
        assert_eq!(plan.summary.changes(), 3);
        assert_eq!(plan.len(), 3);
    }

    #[test]
    fn a_second_snapshot_updates_reactivates_deactivates_and_deletes() {
        let ada = user("u1").title("Engineer");
        let grace = user("u2").inactive();
        let alan = user("u3");
        let linus = user("u4");
        let eng = DirectoryGroup::new("g1", "Engineering").members(["u1", "u3"]);
        let ops = DirectoryGroup::new("g2", "Operations").members(["u4"]);
        let current = held(
            &[ada.clone(), grace.clone(), alan.clone(), linus.clone()],
            &[eng.clone(), ops],
        );
        let pulled = Pulled::snapshot(
            vec![
                ada.clone().title("Principal engineer"),
                DirectoryUser {
                    active: true,
                    ..grace.clone()
                },
                alan.clone().inactive(),
            ],
            vec![DirectoryGroup::new("g1", "R&D").members(["u1", "u2"])],
        );
        let plan = diff(&current, &pulled);
        assert_eq!(
            actions(&plan.users),
            vec![(ChangeAction::Update, "u1"), (ChangeAction::Reactivate, "u2")]
        );
        assert_eq!(plan.users[0].changed, vec!["title"]);
        assert_eq!(
            actions(&plan.deactivations),
            vec![(ChangeAction::Deactivate, "u3"), (ChangeAction::Deactivate, "u4")]
        );
        // u4 is missing from the snapshot: deactivated with its last data.
        assert_eq!(plan.deactivations[1].changed, vec!["active"]);
        assert!(!plan.deactivations[1].user.active);
        assert_eq!(plan.groups.len(), 1);
        let group = &plan.groups[0];
        assert_eq!(group.action, ChangeAction::Update);
        assert_eq!(group.changed, vec!["display_name", "members"]);
        assert_eq!(group.added, vec!["u2"]);
        assert_eq!(group.removed, vec!["u3"]);
        assert_eq!(plan.deleted_groups.len(), 1);
        assert_eq!(plan.deleted_groups[0].group.external_id, "g2");
        let summary = plan.summary;
        assert_eq!(summary.users_updated, 1);
        assert_eq!(summary.users_reactivated, 1);
        assert_eq!(summary.users_deactivated, 2);
        assert_eq!(summary.groups_updated, 1);
        assert_eq!(summary.groups_deleted, 1);
        assert_eq!(summary.memberships_added, 1);
        assert_eq!(summary.memberships_removed, 2);
    }

    #[test]
    fn nothing_changes_when_the_directory_is_the_same() {
        let users = vec![user("u1"), user("u2").inactive()];
        let groups = vec![DirectoryGroup::new("g1", "Engineering").members(["u1"])];
        let plan = diff(&held(&users, &groups), &Pulled::snapshot(users, groups));
        assert_eq!(plan.len(), 0);
        assert_eq!(plan.summary.users_unchanged, 2);
        assert_eq!(plan.summary.groups_unchanged, 1);
        // An inactive user missing from a snapshot stays as it is.
        let plan = diff(
            &held(&[user("u1").inactive()], &[]),
            &Pulled::snapshot(Vec::new(), Vec::new()),
        );
        assert_eq!(plan.len(), 0);
    }

    #[test]
    fn a_delta_changes_only_what_it_names() {
        let current = held(
            &[user("u1"), user("u2"), user("u3")],
            &[
                DirectoryGroup::new("g1", "Engineering").members(["u1", "u2"]),
                DirectoryGroup::new("g2", "Sales").members(["u3"]),
                DirectoryGroup::new("g3", "Legacy"),
            ],
        );
        let pulled = Pulled::delta(
            vec![user("u4"), user("u1").title("CTO")],
            vec![DirectoryGroup::new("g4", "Design").members(["ignored"])],
        )
        .removed_user("u2")
        .removed_user("nobody")
        .removed_group("g3")
        .member_added("g1", "u4")
        .member_removed("g1", "u2")
        .member_added("g4", "u1")
        .member_added("g2", "stranger")
        .member_added("unknown-group", "u1");
        let plan = diff(&current, &pulled);
        assert_eq!(
            actions(&plan.users),
            vec![(ChangeAction::Create, "u4"), (ChangeAction::Update, "u1")]
        );
        assert_eq!(actions(&plan.deactivations), vec![(ChangeAction::Deactivate, "u2")]);
        let groups: Vec<(&str, ChangeAction, Vec<String>)> = plan
            .groups
            .iter()
            .map(|g| (g.group.external_id.as_str(), g.action, g.group.members.clone()))
            .collect();
        assert_eq!(
            groups,
            vec![
                ("g4", ChangeAction::Create, vec!["u1".to_owned()]),
                ("g1", ChangeAction::Update, vec!["u1".to_owned(), "u4".to_owned()]),
            ]
        );
        assert_eq!(plan.groups[1].changed, vec!["members"]);
        assert_eq!(plan.deleted_groups.len(), 1);
        assert_eq!(plan.deleted_groups[0].group.external_id, "g3");
        // u3 is not in this delta but stays; g2 did not change.
        assert_eq!(plan.summary.unknown_members, 1);
        assert_eq!(plan.summary.users_deactivated, 1);
    }

    #[test]
    fn records_without_an_id_or_a_name_are_skipped_and_duplicates_collapse() {
        let pulled = Pulled::snapshot(
            vec![
                user(""),
                DirectoryUser::new("u9", " "),
                user("u1").title("first"),
                user("u1").title("second"),
            ],
            vec![DirectoryGroup::new("g1", ""), DirectoryGroup::new("", "x")],
        );
        let plan = diff(&Current::default(), &pulled);
        assert_eq!(plan.summary.skipped, 4);
        assert_eq!(plan.users.len(), 1);
        assert_eq!(plan.users[0].user.title.as_deref(), Some("second"));
        assert_eq!(plan.summary.users_seen, 1);
    }

    #[test]
    fn the_deactivation_limit_needs_both_a_count_and_a_share() {
        let users: Vec<DirectoryUser> = (0..100).map(|n| user(&format!("u{n}"))).collect();
        let current = held(&users, &[]);
        let keep = |n: usize| Pulled::snapshot(users[..n].to_vec(), Vec::new());
        // 30 of 100 gone: over 25 %.
        let plan = diff(&current, &keep(70));
        assert!(exceeds_deactivation_limit(&plan, &current, 25, 10));
        // 20 of 100: under 25 %.
        let plan = diff(&current, &keep(80));
        assert!(!exceeds_deactivation_limit(&plan, &current, 25, 10));
        // Few users: under the minimum count, whatever the share.
        let small = held(&users[..4], &[]);
        let plan = diff(&small, &Pulled::snapshot(Vec::new(), Vec::new()));
        assert_eq!(plan.deactivations.len(), 4);
        assert!(!exceeds_deactivation_limit(&plan, &small, 25, 10));
        assert!(exceeds_deactivation_limit(&plan, &small, 25, 3));
        // 100 % turns the limit off.
        assert!(!exceeds_deactivation_limit(&plan, &small, 100, 0));
    }
}
