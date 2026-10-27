import type { Deal } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";

/**
 * When a `cannot move` rule is what stops the current user moving `deal` (reps
 * cannot reopen won or lost deals), the reason to show: the rule's own `reason`
 * if the API sent one. `null` when the deal is simply someone else's.
 */
export function moveBlockedReason(ability: AppAbility, deal: Deal): string | null {
  const rule = ability.relevantRuleFor("move", deal);
  if (!rule?.inverted) return null;
  return rule.reason ?? "Only a manager can reopen a won or lost deal.";
}

/** Why the current user can't move `deal`, for the lock on its card. */
export function lockReason(ability: AppAbility, deal: Deal): string {
  return moveBlockedReason(ability, deal) ?? `Owned by ${deal.owner.name}; you can't move it`;
}
