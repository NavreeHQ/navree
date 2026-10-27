"use client";

/**
 * The current workspace in the browser. It lives in the `navree_workspace`
 * cookie (not a secret, so not httpOnly): the typed client sends it as
 * `X-Workspace`, the BFF falls back to it, and server components read it.
 */
import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";

export const WORKSPACE_COOKIE = "navree_workspace";

/** The workspace cookie in the browser (`undefined` on the server or when none is chosen). */
export function readWorkspaceCookie(name = WORKSPACE_COOKIE): string | undefined {
  if (typeof document === "undefined") return undefined;
  for (const part of document.cookie.split(";")) {
    const [key, ...rest] = part.trim().split("=");
    if (key === name) return decodeURIComponent(rest.join("=")) || undefined;
  }
  return undefined;
}

/** Writes (or, with `undefined`, clears) the workspace cookie for a year. */
export function writeWorkspaceCookie(workspace: string | undefined, name = WORKSPACE_COOKIE): void {
  if (typeof document === "undefined") return;
  const secure = typeof location !== "undefined" && location.protocol === "https:" ? "; Secure" : "";
  document.cookie = workspace
    ? `${name}=${encodeURIComponent(workspace)}; Path=/; SameSite=Lax; Max-Age=31536000${secure}`
    : `${name}=; Path=/; SameSite=Lax; Max-Age=0${secure}`;
}

interface WorkspaceContextValue {
  workspace: string | undefined;
  setWorkspace: (workspace: string | undefined) => void;
}

const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);

export interface WorkspaceProviderProps {
  /** The workspace the server rendered with (from `auth()` or the cookie). */
  initial?: string;
  /** Called after a switch: clear cached queries, refresh abilities, `router.refresh()`. */
  onChange?: (workspace: string | undefined) => void;
  cookieName?: string;
  children: ReactNode;
}

export function WorkspaceProvider({ initial, onChange, cookieName = WORKSPACE_COOKIE, children }: WorkspaceProviderProps) {
  const [workspace, setState] = useState<string | undefined>(() => initial ?? readWorkspaceCookie(cookieName));
  const setWorkspace = useCallback(
    (next: string | undefined) => {
      writeWorkspaceCookie(next, cookieName);
      setState(next);
      onChange?.(next);
    },
    [cookieName, onChange],
  );
  const value = useMemo(() => ({ workspace, setWorkspace }), [workspace, setWorkspace]);
  return <WorkspaceContext.Provider value={value}>{children}</WorkspaceContext.Provider>;
}

/** The current workspace and a switch. */
export function useWorkspace(): WorkspaceContextValue {
  const value = useContext(WorkspaceContext);
  if (!value) throw new Error("useWorkspace() needs a <WorkspaceProvider> above it");
  return value;
}
