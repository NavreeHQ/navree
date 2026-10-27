import { useAbility } from "@casl/vue";
import { useEventListener } from "@vueuse/core";
import type { AppAbility } from "@/lib/casl/ability";
import { GO_KEYS } from "@/lib/navigation";
import { type CreateKind, useUiStore } from "@/stores/ui";

const CREATE_KEYS: Record<string, { kind: CreateKind; subject: "Deal" | "Company" | "Contact" | "Activity" }> = {
  d: { kind: "deal", subject: "Deal" },
  c: { kind: "company", subject: "Company" },
  p: { kind: "contact", subject: "Contact" },
  t: { kind: "task", subject: "Activity" },
};

function typing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) || target.getAttribute("role") === "combobox";
}

/**
 * The app's keyboard shortcuts: Ctrl K or ⌘ K (and /) for the palette, Ctrl B
 * or ⌘ B (and `[`) for the sidebar, `?` for the list, `g` then a letter to go
 * somewhere, `c` then a letter to create something. Single keys never fire
 * while typing or while a dialog is open.
 */
export function useShortcuts() {
  const ui = useUiStore();
  const ability = useAbility<AppAbility>();
  let leader: "g" | "c" | null = null;

  /** The desktop sidebar collapses to its icon rail; on phones the navigation sheet opens or closes. */
  function toggleSidebar() {
    if (window.matchMedia("(min-width: 768px)").matches) ui.sidebarCollapsed = !ui.sidebarCollapsed;
    else ui.mobileNavOpen = !ui.mobileNavOpen;
  }
  let leaderTimer: ReturnType<typeof setTimeout> | undefined;

  useEventListener(window, "keydown", (event: KeyboardEvent) => {
    const key = event.key.toLowerCase();
    if (key === "k" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      ui.paletteOpen = !ui.paletteOpen;
      return;
    }
    if (key === "b" && (event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey) {
      event.preventDefault();
      toggleSidebar();
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey || event.defaultPrevented) return;
    if (typing(event.target) || document.querySelector("[role=dialog], [role=alertdialog]")) return;

    if (leader) {
      const which = leader;
      leader = null;
      clearTimeout(leaderTimer);
      if (which === "g" && GO_KEYS[key]) {
        event.preventDefault();
        void navigateTo(GO_KEYS[key]);
      } else if (which === "c" && CREATE_KEYS[key] && ability.can("create", CREATE_KEYS[key].subject)) {
        event.preventDefault();
        ui.create(CREATE_KEYS[key].kind);
      }
      return;
    }
    if (key === "g" || key === "c") {
      leader = key;
      leaderTimer = setTimeout(() => (leader = null), 1200);
    } else if (key === "/") {
      event.preventDefault();
      ui.paletteOpen = true;
    } else if (event.key === "?") {
      event.preventDefault();
      ui.shortcutsOpen = true;
    } else if (key === "[") {
      toggleSidebar();
    }
  });
}
