/**
 * In-memory database for the mock API. State survives page reloads within the
 * tab (sessionStorage) so the demo behaves like a real backend while you click
 * around; a new tab or `window.__navreeMock.reset()` starts from the seed again.
 */
import { createSeed, type MockDatabase } from "./seed";

const DB_KEY = "navree-mock:db:v1";
const SESSION_KEY = "navree-mock:session";
const THROTTLE_KEY = "navree-mock:throttle";

function storage(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.sessionStorage;
  } catch {
    return null;
  }
}

function load(): MockDatabase {
  const raw = storage()?.getItem(DB_KEY);
  if (raw) {
    try {
      return JSON.parse(raw) as MockDatabase;
    } catch {
      // fall through to a fresh seed
    }
  }
  return createSeed();
}

export let db: MockDatabase = load();

let saveTimer: ReturnType<typeof setTimeout> | null = null;

/** Persist after a mutation (debounced). */
export function persist(): void {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    storage()?.setItem(DB_KEY, JSON.stringify(db));
  }, 50);
}

export function resetDatabase(): void {
  db = createSeed();
  storage()?.removeItem(DB_KEY);
  storage()?.removeItem(SESSION_KEY);
  storage()?.removeItem(THROTTLE_KEY);
}

// ---- Session (stands in for the BFF's httpOnly token cookies) ----------------

export function sessionUserId(): string | null {
  return storage()?.getItem(SESSION_KEY) ?? null;
}

export function setSessionUser(id: string | null): void {
  if (id === null) storage()?.removeItem(SESSION_KEY);
  else storage()?.setItem(SESSION_KEY, id);
}

// ---- Login throttle ------------------------------------------------------------

export function failedAttempts(email: string): number {
  const map = JSON.parse(storage()?.getItem(THROTTLE_KEY) ?? "{}") as Record<string, number>;
  return map[email.toLowerCase()] ?? 0;
}

export function recordFailedAttempt(email: string, reset = false): void {
  const map = JSON.parse(storage()?.getItem(THROTTLE_KEY) ?? "{}") as Record<string, number>;
  if (reset) delete map[email.toLowerCase()];
  else map[email.toLowerCase()] = (map[email.toLowerCase()] ?? 0) + 1;
  storage()?.setItem(THROTTLE_KEY, JSON.stringify(map));
}
