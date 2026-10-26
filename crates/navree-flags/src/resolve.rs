//! How a flag is resolved for a scope: the first rule that decides wins.
//!
//! 1. Killed: off for everyone.
//! 2. The user's override.
//! 3. The workspace's override.
//! 4. The global state ("everyone").
//! 5. The rollout, when the user (or workspace) falls inside it.
//! 6. The flag's default.
//!
//! A check with neither a user nor a workspace under
//! `missing_scope = "off"` is on only when the flag is on for everyone.

use uuid::Uuid;

use crate::{FlagScope, FlagState, MissingScope, Rollout, RolloutUnit};

/// Everything a decision needs except a workspace's plan.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Facts<'a> {
    /// The flag's name (its rollout bucket depends on it).
    pub(crate) flag: &'a str,
    /// The flag's default.
    pub(crate) default: bool,
    /// The stored global state, if any.
    pub(crate) state: Option<&'a FlagState>,
    /// The override of the scope's user, if any.
    pub(crate) user_override: Option<bool>,
    /// The override of the scope's workspace, if any.
    pub(crate) workspace_override: Option<bool>,
    /// Who is asking.
    pub(crate) scope: &'a FlagScope,
    /// `flags.missing_scope`.
    pub(crate) missing_scope: MissingScope,
}

/// What the rules before the rollout say.
enum Early<'a> {
    Decided(bool),
    Rollout(&'a Rollout),
    Default,
}

impl<'a> Facts<'a> {
    fn early(&self) -> Early<'a> {
        let state = self.state;
        if state.is_some_and(|state| state.killed) {
            return Early::Decided(false);
        }
        if self.scope.is_everyone() && self.missing_scope == MissingScope::Off {
            return Early::Decided(state.and_then(|state| state.everyone) == Some(true));
        }
        if let Some(active) = self.user_override.or(self.workspace_override) {
            return Early::Decided(active);
        }
        if let Some(active) = state.and_then(|state| state.everyone) {
            return Early::Decided(active);
        }
        match state.and_then(|state| state.rollout.as_ref()) {
            Some(rollout) => Early::Rollout(rollout),
            None => Early::Default,
        }
    }

    /// The id the rollout buckets (the user or the workspace), when the
    /// scope has it and it falls inside the percentage.
    fn in_percentage(&self, rollout: &Rollout) -> bool {
        let id = match rollout.of {
            RolloutUnit::Users => self.scope.user_id(),
            RolloutUnit::Workspaces => self.scope.workspace_id(),
        };
        id.is_some_and(|id| rollout.includes_named(self.flag, id))
    }

    /// The workspace whose plan the decision needs: only when every earlier
    /// rule is silent, the rollout is limited to plans and the scope falls
    /// inside its percentage.
    pub(crate) fn plan_needed(&self) -> Option<Uuid> {
        match self.early() {
            Early::Rollout(rollout) if !rollout.plans.is_empty() && self.in_percentage(rollout) => {
                self.scope.workspace_id()
            }
            _ => None,
        }
    }

    /// The decision, given the plan of the scope's workspace (when
    /// [`plan_needed`](Facts::plan_needed) asked for it).
    pub(crate) fn decide(&self, plan: Option<&str>) -> bool {
        match self.early() {
            Early::Decided(active) => active,
            Early::Rollout(rollout) => {
                let on_plan = rollout.plans.is_empty()
                    || (self.scope.workspace_id().is_some()
                        && plan.is_some_and(|plan| rollout.plans.iter().any(|p| p == plan)));
                if self.in_percentage(rollout) && on_plan {
                    true
                } else {
                    self.default
                }
            }
            Early::Default => self.default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLAG: &str = "new-pipeline";

    fn state(everyone: Option<bool>, rollout: Option<Rollout>, killed: bool) -> FlagState {
        FlagState {
            everyone,
            rollout,
            killed,
            ..FlagState::unset(FLAG)
        }
    }

    struct Case {
        default: bool,
        state: Option<FlagState>,
        user: Option<bool>,
        workspace: Option<bool>,
    }

    impl Case {
        fn new() -> Case {
            Case {
                default: false,
                state: None,
                user: None,
                workspace: None,
            }
        }

        fn decide(&self, scope: &FlagScope, missing: MissingScope) -> bool {
            let facts = Facts {
                flag: FLAG,
                default: self.default,
                state: self.state.as_ref(),
                user_override: self.user,
                workspace_override: self.workspace,
                scope,
                missing_scope: missing,
            };
            assert_eq!(facts.plan_needed(), None);
            facts.decide(None)
        }
    }

    /// An id whose bucket for FLAG is below (or not below) `percent`.
    fn id_with_bucket(inside: bool, percent: u8) -> Uuid {
        (0..10_000_u128)
            .map(Uuid::from_u128)
            .find(|id| (crate::bucket(FLAG, *id) < percent) == inside)
            .unwrap()
    }

    #[test]
    fn resolution_matrix() {
        let (user, workspace) = (Uuid::now_v7(), Uuid::now_v7());
        let both = FlagScope::user(user).workspace(workspace);
        let d = MissingScope::Default;

        // 6. The default when nothing else decides.
        assert!(!Case::new().decide(&both, d));
        assert!(
            Case {
                default: true,
                ..Case::new()
            }
            .decide(&both, d)
        );

        // 4. The global state beats the default.
        let on_for_everyone = Case {
            state: Some(state(Some(true), None, false)),
            ..Case::new()
        };
        assert!(on_for_everyone.decide(&both, d));
        let off_for_everyone = Case {
            default: true,
            state: Some(state(Some(false), None, false)),
            ..Case::new()
        };
        assert!(!off_for_everyone.decide(&both, d));

        // 3. The workspace override beats the global state.
        let workspace_off = Case {
            state: Some(state(Some(true), None, false)),
            workspace: Some(false),
            ..Case::new()
        };
        assert!(!workspace_off.decide(&both, d));

        // 2. The user override beats the workspace override.
        let user_on = Case {
            user: Some(true),
            ..workspace_off
        };
        assert!(user_on.decide(&both, d));

        // 1. Killed beats everything, overrides included.
        let killed = Case {
            state: Some(state(Some(true), Some(Rollout::percent(100)), true)),
            ..user_on
        };
        assert!(!killed.decide(&both, d));
        assert!(
            !Case {
                default: true,
                state: Some(state(None, None, true)),
                ..Case::new()
            }
            .decide(&both, d),
            "a killed flag ignores its default too"
        );
    }

    #[test]
    fn the_rollout_decides_only_for_those_inside() {
        let inside = id_with_bucket(true, 30);
        let outside = id_with_bucket(false, 30);
        let case = Case {
            state: Some(state(None, Some(Rollout::percent(30)), false)),
            ..Case::new()
        };
        let d = MissingScope::Default;
        assert!(case.decide(&FlagScope::user(inside), d));
        assert!(!case.decide(&FlagScope::user(outside), d));
        // Outside the rollout, the default decides.
        let default_on = Case {
            default: true,
            ..case
        };
        assert!(default_on.decide(&FlagScope::user(outside), d));

        // A rollout of workspaces needs a workspace in the scope.
        let by_workspace = Case {
            state: Some(state(None, Some(Rollout::percent(30).of_workspaces()), false)),
            ..Case::new()
        };
        assert!(by_workspace.decide(&FlagScope::workspace(inside), d));
        assert!(!by_workspace.decide(&FlagScope::user(inside), d));
        assert!(by_workspace.decide(&FlagScope::user(outside).workspace(inside), d));

        // The everyone state beats the rollout.
        let everyone_off = Case {
            state: Some(state(Some(false), Some(Rollout::percent(100)), false)),
            ..Case::new()
        };
        assert!(!everyone_off.decide(&FlagScope::user(inside), d));
    }

    #[test]
    fn plan_rollouts_ask_for_the_workspace_plan() {
        let workspace = id_with_bucket(true, 50);
        let rollout = Rollout::percent(50).of_workspaces().on_plans(["pro", "business"]);
        let state = state(None, Some(rollout), false);
        let scope = FlagScope::workspace(workspace);
        let facts = Facts {
            flag: FLAG,
            default: false,
            state: Some(&state),
            user_override: None,
            workspace_override: None,
            scope: &scope,
            missing_scope: MissingScope::Default,
        };
        assert_eq!(facts.plan_needed(), Some(workspace));
        assert!(facts.decide(Some("pro")));
        assert!(facts.decide(Some("business")));
        assert!(!facts.decide(Some("free")));
        assert!(!facts.decide(None));

        // Outside the percentage, no plan is looked up.
        let outside = FlagScope::workspace(id_with_bucket(false, 50));
        let facts = Facts {
            scope: &outside,
            ..facts
        };
        assert_eq!(facts.plan_needed(), None);
        assert!(!facts.decide(Some("pro")));

        // A user rollout limited to plans reads the scope's workspace.
        let user = id_with_bucket(true, 100);
        let by_user = FlagState {
            rollout: Some(Rollout::percent(100).on_plans(["pro"])),
            ..FlagState::unset(FLAG)
        };
        let scope = FlagScope::user(user).workspace(workspace);
        let facts = Facts {
            state: Some(&by_user),
            scope: &scope,
            ..facts
        };
        assert_eq!(facts.plan_needed(), Some(workspace));
        assert!(facts.decide(Some("pro")));
        let no_workspace = FlagScope::user(user).scope();
        let facts = Facts {
            scope: &no_workspace,
            ..facts
        };
        assert_eq!(facts.plan_needed(), None);
        assert!(!facts.decide(None));
    }

    #[test]
    fn checks_without_a_scope() {
        let everyone = FlagScope::everyone();
        let unset = Case {
            default: true,
            ..Case::new()
        };
        assert!(unset.decide(&everyone, MissingScope::Default));
        assert!(!unset.decide(&everyone, MissingScope::Off));

        let on = Case {
            state: Some(state(Some(true), None, false)),
            ..Case::new()
        };
        assert!(on.decide(&everyone, MissingScope::Default));
        assert!(on.decide(&everyone, MissingScope::Off));

        let rollout = Case {
            state: Some(state(None, Some(Rollout::percent(100)), false)),
            ..Case::new()
        };
        assert!(
            !rollout.decide(&everyone, MissingScope::Default),
            "nobody to bucket"
        );

        let killed = Case {
            state: Some(state(Some(true), None, true)),
            ..Case::new()
        };
        assert!(!killed.decide(&everyone, MissingScope::Off));

        // A partial scope is not a missing one.
        let user = FlagScope::user(Uuid::now_v7()).scope();
        assert!(unset.decide(&user, MissingScope::Off));
    }
}
