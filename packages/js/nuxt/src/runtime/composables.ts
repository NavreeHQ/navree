/** The module's auto-imported composables. */
import type { NavreeClient } from "@navree/client";
import { useNuxtApp } from "#imports";
import type { NavreeAuth } from "../vue/auth.js";
import type { Workspace } from "../vue/workspace.js";
import type { NavreeApp } from "./plugin.js";

/** Everything the module provides: `auth`, `workspace`, `client`, `fetch`. */
export function useNavree(): NavreeApp {
  return useNuxtApp().$navree;
}

/** Sign-in state: `user`, `status`, `signIn`, `verifyTwoFactor`, `signOut`, `load`. */
export function useNavreeAuth<User = unknown>(): NavreeAuth<User> {
  return useNavree().auth as NavreeAuth<User>;
}

/** The current workspace and `setWorkspace` (stored in the `navree_workspace` cookie). */
export function useWorkspace(): Workspace {
  return useNavree().workspace;
}

/** The typed client (`paths` from `navree openapi:client`), same origin through the BFF. */
export function useNavreeClient<Paths extends object>(): NavreeClient<Paths> {
  return useNavree().client as unknown as NavreeClient<Paths>;
}
