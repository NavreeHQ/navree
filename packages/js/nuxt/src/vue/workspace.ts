/**
 * The current workspace, kept in the `navree_workspace` cookie (in Nuxt,
 * `useCookie`): the typed client sends it as `X-Workspace`, the BFF falls
 * back to it, and server-side rendering reads it.
 */
import { computed, type ComputedRef, type Ref } from "vue";

export const WORKSPACE_COOKIE = "navree_workspace";

export interface Workspace {
  workspace: ComputedRef<string | undefined>;
  /** Switches workspace (or clears it with `undefined`), then calls `onChange`. */
  setWorkspace(workspace: string | undefined): void;
}

/** Workspace state over a cookie ref (`useCookie(WORKSPACE_COOKIE)` in Nuxt). */
export function createWorkspace(cookie: Ref<string | null | undefined>, onChange?: (workspace: string | undefined) => void): Workspace {
  return {
    workspace: computed(() => cookie.value || undefined),
    setWorkspace(workspace) {
      cookie.value = workspace ?? null;
      onChange?.(workspace);
    },
  };
}
