/**
 * The workspace a person last opened, so `/` and `/billing/return` take them back to it. It is kept
 * in the browser only, not in the `navree_workspace` cookie of `@navree/next`: this app names the
 * workspace in the URL (`/w/<slug>/`) and sends it as `X-Workspace` on each call, and a cookie would
 * also put it on the calls that are made outside any workspace (the user, their workspaces).
 */

const KEY = "navree.workspace";

export function rememberWorkspace(slug: string): void {
  try {
    window.localStorage.setItem(KEY, slug);
  } catch {
    // private mode: nothing to remember
  }
}

export function lastWorkspace(): string | null {
  try {
    return window.localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

/** `/w/<slug>` plus `path`. */
export function workspacePath(slug: string, path = ""): string {
  return `/w/${encodeURIComponent(slug)}${path}`;
}
