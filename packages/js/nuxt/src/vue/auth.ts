/**
 * Sign-in state for a Vue app behind the backend-for-frontend: the user,
 * sign-in with two-factor, sign-out. Every call goes to the app's own
 * `/api/auth/*` routes; the tokens stay in httpOnly cookies.
 */
import { NavreeError } from "@navree/client";
import { computed, readonly, ref, type ComputedRef, type DeepReadonly, type Ref } from "vue";

export type AuthStatus = "unknown" | "signed-in" | "signed-out" | "two-factor";

export type SignInResult<User> = { twoFactor: false; user: User } | { twoFactor: true; expiresIn: number };

export interface NavreeAuthOptions {
  /** Sends the requests (default `fetch` on the app's own origin). */
  fetch?: (path: string, init?: RequestInit) => Promise<Response>;
  /** The BFF's auth routes (default `/api/auth`). */
  authPrefix?: string;
  /** Called after sign-out and when the session turns out to be over. */
  onSignedOut?: () => void;
}

export interface NavreeAuth<User> {
  user: DeepReadonly<Ref<User | null>>;
  status: DeepReadonly<Ref<AuthStatus>>;
  signedIn: ComputedRef<boolean>;
  /** Loads the user (`GET /api/auth/me`); `null` when signed out. */
  load(): Promise<User | null>;
  /** Signs in; a two-factor account answers `{twoFactor: true}` and waits for `verifyTwoFactor`. */
  signIn(credentials: Record<string, unknown>): Promise<SignInResult<User>>;
  /** Completes a two-factor sign-in with `{code}` or `{recovery_code}`. */
  verifyTwoFactor(proof: { code: string } | { recovery_code: string }): Promise<User>;
  /** Signs out (`{all: true}` ends every session of the user). */
  signOut(options?: { all?: boolean }): Promise<void>;
}

async function body(response: Response): Promise<unknown> {
  const text = await response.text();
  if (!text) return undefined;
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

export function createNavreeAuth<User = unknown>(options: NavreeAuthOptions = {}): NavreeAuth<User> {
  const send = options.fetch ?? ((path: string, init?: RequestInit) => globalThis.fetch(path, { credentials: "same-origin", ...init }));
  const prefix = (options.authPrefix ?? "/api/auth").replace(/\/+$/, "");
  const user = ref<User | null>(null) as Ref<User | null>;
  const status = ref<AuthStatus>("unknown");

  const post = (path: string, payload?: unknown) =>
    send(`${prefix}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json", accept: "application/json" },
      body: payload === undefined ? undefined : JSON.stringify(payload),
    });

  function signedOut(): void {
    const wasSignedIn = status.value === "signed-in";
    user.value = null;
    status.value = "signed-out";
    if (wasSignedIn) options.onSignedOut?.();
  }

  return {
    user: readonly(user) as DeepReadonly<Ref<User | null>>,
    status: readonly(status),
    signedIn: computed(() => status.value === "signed-in"),

    async load() {
      const response = await send(`${prefix}/me`, { headers: { accept: "application/json" } });
      if (response.status === 401) {
        await response.arrayBuffer().catch(() => undefined);
        signedOut();
        return null;
      }
      const data = await body(response);
      if (!response.ok) throw NavreeError.fromResponse(response, data);
      user.value = data as User;
      status.value = "signed-in";
      return user.value;
    },

    async signIn(credentials) {
      const response = await post("/login", credentials);
      const data = (await body(response)) as { user?: User; two_factor_required?: boolean; expires_in?: number };
      if (!response.ok) throw NavreeError.fromResponse(response, data);
      if (data?.two_factor_required) {
        status.value = "two-factor";
        return { twoFactor: true, expiresIn: data.expires_in ?? 300 };
      }
      user.value = (data?.user ?? null) as User | null;
      status.value = "signed-in";
      return { twoFactor: false, user: user.value as User };
    },

    async verifyTwoFactor(proof) {
      const response = await post("/2fa/verify", proof);
      const data = (await body(response)) as { user?: User };
      if (!response.ok) throw NavreeError.fromResponse(response, data);
      user.value = (data?.user ?? null) as User | null;
      status.value = "signed-in";
      return user.value as User;
    },

    async signOut(signOutOptions = {}) {
      try {
        await post("/logout", signOutOptions.all ? { all: true } : {});
      } finally {
        const wasSignedIn = status.value === "signed-in";
        user.value = null;
        status.value = "signed-out";
        if (wasSignedIn) options.onSignedOut?.();
      }
    },
  };
}
