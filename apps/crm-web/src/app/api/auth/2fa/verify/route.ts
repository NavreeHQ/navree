import { bff } from "@/lib/bff/server";

/**
 * Second sign-in step for accounts with two-factor authentication: `{code}` or
 * `{recovery_code}`. The BFF adds the challenge token from its httpOnly cookie,
 * and on success stores the tokens like login and returns only `{user}`.
 */
export function POST(request: Request): Promise<Response> {
  return bff.verifyTwoFactor(request);
}

export const dynamic = "force-dynamic";
