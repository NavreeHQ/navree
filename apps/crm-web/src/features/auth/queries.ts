"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "@/lib/api/client";
import { ApiError } from "@/lib/api/errors";
import { keys } from "@/lib/api/keys";
import type { BffLoginResponse, BffLoginResult, BffTwoFactorInput, CaslRule, LoginInput, User } from "@/lib/api/types";

export function useCurrentUser() {
  return useQuery({
    queryKey: keys.auth.user,
    // Documented as `UserResource`; crm-api's user resource extension adds `roles`, so it is a `User`.
    queryFn: () => unwrap(api.GET("/api/auth/me")) as Promise<User>,
    retry: false,
    staleTime: 5 * 60_000,
  });
}

export function useAbilityRules(enabled: boolean) {
  return useQuery({
    queryKey: keys.auth.abilities,
    queryFn: () => unwrap<CaslRule[]>(api.GET("/api/me/abilities")),
    enabled,
    staleTime: 5 * 60_000,
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
 * cookies and returns only `{user}`. Accounts with two-factor authentication get
 * `{two_factor_required: true}` instead (the BFF keeps the challenge token) and
 * finish with {@link useVerifyTwoFactor}.
 */
export function useLogin() {
  const signedIn = useSignedIn();
  return useMutation({
    mutationFn: async (input: LoginInput) =>
      (await unwrap(api.POST("/api/auth/login", { body: input }))) as unknown as BffLoginResult,
    onSuccess: (result) => {
      if (!needsTwoFactor(result)) signedIn(result.user);
    },
  });
}

/**
 * The code step of a two-factor sign-in: `{code}` from the authenticator app or a
 * `{recovery_code}`. The BFF adds the challenge token from its cookie, stores the
 * tokens and returns `{user}`.
 */
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
  });
}

/** Revokes the refresh token at Navree and clears the cookies (`all` signs out every device). */
export function useLogout() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (options: { all?: boolean } = {}) =>
      unwrap(api.POST("/api/auth/logout", { body: options.all ? { all: true } : {} })),
    onSettled: () => {
      queryClient.clear();
    },
  });
}
