/** The workspace a person last opened, so `/` and `/billing/return` take them back to it. */

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
