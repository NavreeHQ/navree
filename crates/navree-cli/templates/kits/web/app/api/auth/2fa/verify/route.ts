import { bff } from "@/lib/bff/server";

/**
 * The second sign-in step for accounts with two-factor authentication: `{code}` or
 * `{recovery_code}`. The BFF adds the challenge token from its httpOnly cookie; on success it
 * stores the tokens like the sign-in and returns only `{user}`.
 */
export function POST(request: Request): Promise<Response> {
  return bff.verifyTwoFactor(request);
}

export const dynamic = "force-dynamic";
