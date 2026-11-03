/**
 * A component's own parts, from the `data-slot` tree of one of its examples.
 * Shared by `<UiAnatomy>` and the docs' Markdown copies, so both draw the same.
 *
 * Plain TypeScript with no imports: Node runs it with the types stripped.
 */

export type Slot = { slot: string; tag: string; count: number; children: Slot[] };

/** Layout wrappers an example puts around things: never a component's part. */
const LAYOUT = new Set(["row", "stack", "grid", "container"]);

/**
 * The root parts of other components. Inside a component they are content
 * someone put there (a button in a card's footer), so they are drawn as one
 * box, without their own insides.
 */
const COMPONENT_ROOTS = new Set([
  "alert", "attachment", "avatar", "badge", "bars", "breadcrumb", "bubble", "button", "button-group",
  "card", "checkbox", "choice", "code", "combobox", "command-dialog", "empty", "field", "heading", "input",
  "input-group", "item", "kbd", "label", "marker", "message", "multiselect", "native-select", "pagination",
  "progress", "radio-group", "select", "separator", "skeleton", "spinner", "stat", "switch", "table-container",
  "text", "textarea", "toggle",
]);

/** Components whose outer element, with a label, has another slot. */
const OWN_ROOT: Record<string, string> = { checkbox: "choice", switch: "choice", toast: "toaster" };

function trim(slots: Slot[]): Slot[] {
  const out: Slot[] = [];
  for (const s of slots) {
    if (LAYOUT.has(s.slot)) {
      out.push(...trim(s.children));
    } else if (COMPONENT_ROOTS.has(s.slot)) {
      out.push({ ...s, children: [] });
    } else {
      out.push({ ...s, children: trim(s.children) });
    }
  }
  // Wrappers lifted out can leave siblings of one kind side by side: one box.
  const merged: Slot[] = [];
  for (const s of out) {
    const last = merged[merged.length - 1];
    if (last && last.slot === s.slot) last.count += s.count;
    else merged.push({ ...s });
  }
  return merged;
}

/** The parts of component `id`, drawn from `slots`. */
export function ownSlots(slots: Slot[], id: string): Slot[] {
  if (id === "layout") return slots;
  let parts = slots;
  while (parts.length > 0 && parts.every((p) => LAYOUT.has(p.slot))) parts = parts.flatMap((p) => p.children);
  // An example that puts the component in something else (a field, so it has
  // a label; a row beside its caption): start from the component itself.
  const root = OWN_ROOT[id] ?? id;
  // A popover draws its own button.
  const mine = (p: Slot) => p.slot === root || p.slot.startsWith(`${root}-`) || ((id === "popover" || id === "dropdown-menu") && p.slot === "button");
  if (id !== "field" && parts.some(mine)) {
    parts = parts.filter(mine);
  } else if (id !== "field") {
    const own = find(parts, root);
    if (own) parts = [own];
  }
  // Several of the component side by side: one is enough to show its parts.
  const tops: Slot[] = [];
  for (const p of parts) {
    if (tops[tops.length - 1]?.slot !== p.slot) tops.push({ ...p, count: 1, children: trim(p.children) });
  }
  return tops;
}

function find(slots: Slot[], name: string): Slot | null {
  for (const s of slots) {
    if (s.slot === name) return s;
    const deeper = find(s.children, name);
    if (deeper) return deeper;
  }
  return null;
}
