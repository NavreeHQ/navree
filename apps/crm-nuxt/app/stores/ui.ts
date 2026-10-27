import { useLocalStorage } from "@vueuse/core";
import { defineStore } from "pinia";

export type CreateKind = "deal" | "company" | "contact" | "task";
export type RealtimeState = "off" | "connecting" | "live" | "unavailable";

/**
 * App-wide UI state: the sidebar, the command palette, the shortcuts sheet,
 * the create dialogs the palette and the `c` shortcuts open, the state of the
 * real-time connection (shown in the top bar), the record a detail screen
 * shows (the last breadcrumb) and the deals that just changed somewhere else
 * (their cards light up for a moment).
 */
export const useUiStore = defineStore("ui", () => {
  // A per-device preference: kept in localStorage, never on the server.
  const sidebarCollapsed = useLocalStorage("navree-crm:sidebar-collapsed", false);
  const mobileNavOpen = ref(false);
  const paletteOpen = ref(false);
  const shortcutsOpen = ref(false);
  const creating = ref<CreateKind | null>(null);
  const realtime = ref<RealtimeState>("off");
  const flashing = ref<ReadonlySet<string>>(new Set());
  /** The record on screen, by path, so a breadcrumb never shows the previous screen's name. */
  const record = ref<{ path: string; title: string } | null>(null);

  function create(kind: CreateKind) {
    paletteOpen.value = false;
    mobileNavOpen.value = false;
    creating.value = kind;
  }

  /** Marks a deal as just changed by someone else, for 1.6 s. */
  function flash(dealId: string) {
    flashing.value = new Set([...flashing.value, dealId]);
    setTimeout(() => {
      const next = new Set(flashing.value);
      next.delete(dealId);
      flashing.value = next;
    }, 1600);
  }

  return {
    sidebarCollapsed,
    mobileNavOpen,
    paletteOpen,
    shortcutsOpen,
    creating,
    realtime,
    flashing,
    record,
    create,
    flash,
  };
});
