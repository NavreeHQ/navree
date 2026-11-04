import { NavreeError } from "@navree/client";
import { useMutation, useQuery, useQueryClient } from "@tanstack/vue-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/api/keys";
import type { ChangePasswordInput, User } from "@/lib/api/types";

/** `GET /api/auth/me`. Documented as `UserResource`; crm-api's user resource extension adds `roles`. */
export const fetchCurrentUser = () => unwrap(api.GET("/api/auth/me")) as Promise<User>;
export const SESSION_STALE_TIME = 5 * 60_000;

/**
 * The function that loads the signed-in user into the cache. Sign-in state is the `@navree/nuxt`
 * module's (`useNavreeAuth`): until it knows there is a session, the load goes through it, so it
 * can load the user's abilities. Once it is signed in, later loads use the typed client, where a
 * 401 sends the app to `/login?next=`. A visitor without a session is a 401 either way.
 *
 * Call it where a Nuxt app is in scope (a component's setup, a route middleware); the function it
 * returns can run anywhere.
 */
export function currentUserLoader(): () => Promise<User> {
  const auth = useNavreeAuth<User>();
  return async () => {
    if (auth.status.value === "signed-in") return fetchCurrentUser();
    const user = await auth.load();
    if (!user) throw new NavreeError({ statusCode: 401, message: "Unauthorized", error: "Unauthorized" });
    return user;
  };
}

/** The signed-in user (through the BFF); a 401 sends the app to /login. */
export function useCurrentUser() {
  return useQuery({ queryKey: keys.auth.user, queryFn: currentUserLoader(), retry: false, staleTime: SESSION_STALE_TIME });
}

/** Starts the session in the cache: drops whatever the previous user saw. The module loads the new user's abilities. */
export function useStartSession() {
  const queryClient = useQueryClient();
  return (user: User) => {
    queryClient.removeQueries({ predicate: (q) => q.queryKey[0] !== "auth" });
    queryClient.setQueryData(keys.auth.user, user);
  };
}

/**
 * Signs out through the module: it revokes the refresh token at Navree and clears the cookies
 * (`all` signs out every device), and drops the user's abilities. Whatever the user saw leaves the
 * cache even when the request fails.
 */
export function useSignOut() {
  const auth = useNavreeAuth<User>();
  const queryClient = useQueryClient();
  return async (options: { all?: boolean } = {}) => {
    try {
      await auth.signOut(options.all ? { all: true } : {});
    } finally {
      queryClient.clear();
    }
  };
}

/** `PATCH /api/auth/me/password`: the BFF swaps the new token pair into the cookies. */
export function useChangePassword() {
  return useMutation({
    mutationFn: (body: ChangePasswordInput) => unwrap(api.PATCH("/api/auth/me/password", { body })),
  });
}
