"use client";

import { useQuery } from "@tanstack/react-query";
import { useParams } from "next/navigation";
import { useMemo } from "react";
import { abilityFrom } from "@/lib/abilities";
import { api } from "@/lib/api/client";
import type { CaslRule, DataList, User, Workspace } from "@/lib/api/types";

/** TanStack Query keys: everything a workspace shows is keyed by its slug. */
export const keys = {
  me: ["me"] as const,
  workspaces: ["workspaces"] as const,
  globalAbilities: ["abilities"] as const,
  abilities: (workspace: string) => ["abilities", workspace] as const,
  projects: (workspace: string) => ["projects", workspace] as const,
  project: (workspace: string, id: string) => ["projects", workspace, "one", id] as const,
  members: (workspace: string) => ["members", workspace] as const,
  invitations: (workspace: string) => ["invitations", workspace] as const,
  billing: (workspace: string) => ["billing", workspace] as const,
  invoices: (workspace: string) => ["invoices", workspace] as const,
  settings: (workspace: string) => ["settings", workspace] as const,
  notifications: (workspace: string) => ["notifications", workspace] as const,
  sessions: ["sessions"] as const,
  providers: ["providers"] as const,
};

/** The slug of the `/w/<slug>/` URL: the workspace the screen's calls are made in. */
export function useWorkspaceSlug(): string {
  return useParams<{ workspace: string }>().workspace;
}

/** The signed-in user (`null` for a guest). */
export function useMe() {
  return useQuery({
    queryKey: keys.me,
    queryFn: async () => {
      try {
        return await api.get<User>("/api/auth/me", { workspace: null });
      } catch {
        return null;
      }
    },
  });
}

/** The user's workspaces, each with their role. */
export function useWorkspaces() {
  return useQuery({
    queryKey: keys.workspaces,
    queryFn: async () => (await api.get<DataList<Workspace>>("/api/workspaces", { workspace: null })).data,
  });
}

/** What the user may do in workspace `slug` (their role there). */
export function useWorkspaceAbility(slug: string) {
  const { data } = useQuery({
    queryKey: keys.abilities(slug),
    queryFn: () => api.get<CaslRule[]>("/api/v1/settings/abilities", { workspace: slug }),
  });
  return useMemo(() => abilityFrom(data ?? []), [data]);
}

/** What the user may do outside workspaces (super admins: everything, the admin panel too). */
export function useGlobalAbility() {
  const { data } = useQuery({
    queryKey: keys.globalAbilities,
    queryFn: () => api.get<CaslRule[]>("/api/me/abilities", { workspace: null }),
  });
  return useMemo(() => abilityFrom(data ?? []), [data]);
}
