//! Who may own a record: owner pickers are for users who can
//! `manage` the record type; everyone else creates records they own, and
//! cannot hand them to someone else (403).

use navree::access::{Ability, MANAGE};
use navree::http::AppError;
use uuid::Uuid;

/// The `owner_id` a write sets on a record of `subject` (`"Deal"`, ...).
///
/// - `current`: the record's owner (`None` when creating);
/// - `requested`: the `owner_id` in the request body, if any;
/// - `me`: the signed-in user.
///
/// Nothing requested keeps the current owner (or makes the creator the
/// owner); requesting the owner the record would have anyway is fine;
/// any other owner needs `manage` on `subject` (managers, admins), else a
/// 403. Check that the requested user exists separately (a 400).
///
/// ```ignore
/// let owner_id = owner_for(&ability, DEAL, me, None, input.owner_id)?;
/// ```
pub fn owner_for(
    ability: &Ability,
    subject: &str,
    me: Uuid,
    current: Option<Uuid>,
    requested: Option<Uuid>,
) -> Result<Uuid, AppError> {
    let unchanged = current.unwrap_or(me);
    match requested {
        None => Ok(unchanged),
        Some(owner) if owner == unchanged => Ok(owner),
        Some(owner) if ability.can(MANAGE, subject, None) => Ok(owner),
        Some(_) => Err(AppError::forbidden().with_detail(format!(
            "Only users who can manage {subject} records may change their owner"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use navree::access::{ALL, Ability, AbilityCtx, MANAGE, Rules};
    use uuid::Uuid;

    use super::*;
    use crate::abilities::define;
    use crate::permissions;
    use crate::roles::{ADMIN, MANAGER, REP};
    use crate::subjects::{COMPANY, DEAL};

    const ME: Uuid = Uuid::from_u128(1);
    const OTHER: Uuid = Uuid::from_u128(2);
    const THIRD: Uuid = Uuid::from_u128(3);

    fn ability(role: &str) -> Ability {
        let prefixes: BTreeMap<String, String> = [("companies", "Company"), ("deals", "Deal")]
            .into_iter()
            .map(|(p, s)| (p.to_owned(), s.to_owned()))
            .collect();
        let permissions = if role == MANAGER {
            permissions::MANAGER.map(str::to_owned).to_vec()
        } else {
            Vec::new()
        };
        let ctx = AbilityCtx::for_user(ME, vec![role.to_owned()], permissions)
            .with_subjects(Arc::new(prefixes));
        let mut rules = Rules::new();
        define(&ctx, &mut rules);
        if role == ADMIN {
            rules.can(MANAGE, ALL);
        }
        rules.build()
    }

    #[test]
    fn new_records_belong_to_their_creator_by_default() {
        for role in [ADMIN, MANAGER, REP] {
            assert_eq!(
                owner_for(&ability(role), COMPANY, ME, None, None).ok(),
                Some(ME)
            );
        }
    }

    #[test]
    fn only_managers_of_the_type_assign_records_to_others() {
        assert_eq!(
            owner_for(&ability(REP), DEAL, ME, None, Some(ME)).ok(),
            Some(ME)
        );
        let denied = owner_for(&ability(REP), DEAL, ME, None, Some(OTHER)).expect_err("403");
        assert_eq!(denied.status.as_u16(), 403);
        assert_eq!(
            owner_for(&ability(MANAGER), DEAL, ME, None, Some(OTHER)).ok(),
            Some(OTHER)
        );
        assert_eq!(
            owner_for(&ability(ADMIN), COMPANY, ME, None, Some(OTHER)).ok(),
            Some(OTHER)
        );
    }

    #[test]
    fn updates_keep_the_owner_unless_a_manager_reassigns() {
        let rep = ability(REP);
        // Not sent, or sent unchanged: the current owner stays.
        assert_eq!(owner_for(&rep, DEAL, ME, Some(ME), None).ok(), Some(ME));
        assert_eq!(
            owner_for(&rep, DEAL, ME, Some(OTHER), None).ok(),
            Some(OTHER)
        );
        assert_eq!(
            owner_for(&rep, DEAL, ME, Some(OTHER), Some(OTHER)).ok(),
            Some(OTHER)
        );
        // Reassigning (even to themselves) needs `manage`.
        assert!(owner_for(&rep, DEAL, ME, Some(ME), Some(OTHER)).is_err());
        assert!(owner_for(&rep, DEAL, ME, Some(OTHER), Some(ME)).is_err());
        assert_eq!(
            owner_for(&ability(MANAGER), DEAL, ME, Some(OTHER), Some(THIRD)).ok(),
            Some(THIRD)
        );
    }
}
