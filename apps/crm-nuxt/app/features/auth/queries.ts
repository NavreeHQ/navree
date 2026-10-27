import { useMutation, useQuery, useQueryClient } from "@tanstack/vue-query";
import { api, unwrap } from "@/lib/api/client";
import { ApiError } from "@/lib/api/errors";
import { keys } from "@/lib/api/keys";
import type {
  BffLoginResponse,
  BffLoginResult,
  BffTwoFactorInput,
  CaslRule,
  ChangePasswordInput,
  LoginInput,
  User,
} from "@/lib/api/types";

/** `GET /api/auth/me`. Documented as `UserResource`; crm-api's user resource extension adds `roles`. */
export const fetchCurrentUser = () => unwrap(api.GET("/api/auth/me")) as Promise<User>;
/** `GET /api/me/abilities`: the CASL rules for the signed-in user. */
export const fetchAbilityRules = () => unwrap<CaslRule[]>(api.GET("/api/me/abilities"));
export const SESSION_STALE_TIME = 5 * 60_000;

/** The signed-in user (through the BFF); a 401 sends the app to /login. */
export function useCurrentUser() {
  return useQuery({ queryKey: keys.auth.user, queryFn: fetchCurrentUser, retry: false, staleTime: SESSION_STALE_TIME });
}

/** The CASL rules for the signed-in user. */
export function useAbilityRules(enabled: MaybeRefOrGetter<boolean>) {
  return useQuery({
    queryKey: keys.auth.abilities,
    queryFn: fetchAbilityRules,
    enabled: computed(() => toValue(enabled)),
    staleTime: SESSION_STALE_TIME,
  });
}

/** True when the password step needs a second factor before there is a session. */
export function needsTwoFactor(result: BffLoginResult): result is Exclude<BffLoginResult, BffLoginResponse> {
  return "two_factor_required" in result && result.two_factor_required === true;
}

/** Starts the session in the cache: drops whatever the previous user saw, then loads their abilities. */
function useSignedIn() {
  const queryClient = useQueryClient();
  return (user: User) => {
    queryClient.removeQueries({ predicate: (q) => q.queryKey[0] !== "auth" });
    queryClient.setQueryData(keys.auth.user, user);
    void queryClient.invalidateQueries({ queryKey: keys.auth.abilities });
  };
}

/**
 * Signs in through the BFF: it keeps the access and refresh tokens in httpOnly
 * cookies and returns only `{user}`. Two-factor accounts get
 * `{two_factor_required: true}` instead and finish with {@link useVerifyTwoFactor}.
 */
export function useLogin() {
  const signedIn = useSignedIn();
  return useMutation({
    mutationFn: async (input: LoginInput) =>
      (await unwrap(api.POST("/api/auth/login", { body: input }))) as unknown as BffLoginResult,
    onSuccess: (result) => {
      if (!needsTwoFactor(result)) signedIn(result.user);
    },
    meta: { silent: true },
  });
}

/** The code step of a two-factor sign-in; the BFF adds the challenge token from its cookie. */
export function useVerifyTwoFactor() {
  const signedIn = useSignedIn();
  return useMutation({
    mutationFn: async (input: BffTwoFactorInput) => {
      const response = await api.rawFetch("/api/auth/2fa/verify", {
        method: "POST",
        headers: { "Content-Type": "application/json", Accept: "application/json" },
        body: JSON.stringify(input),
      });
      const body: unknown = await response.json().catch(() => null);
      if (!response.ok) throw ApiError.fromResponse(response, body);
      return body as BffLoginResponse;
    },
    onSuccess: ({ user }) => signedIn(user),
    meta: { silent: true },
  });
}

/** Revokes the refresh token at Navree and clears the cookies (`all` signs out every device). */
export function useLogout() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (options: { all?: boolean } = {}) =>
      unwrap(api.POST("/api/auth/logout", { body: options.all ? { all: true } : {} })),
    onSettled: () => queryClient.clear(),
    meta: { silent: true },
  });
}

/** `PATCH /api/auth/me/password`: the BFF swaps the new token pair into the cookies. */
export function useChangePassword() {
  return useMutation({
    mutationFn: (body: ChangePasswordInput) => unwrap(api.PATCH("/api/auth/me/password", { body })),
  });
}
