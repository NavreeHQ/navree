import { useBff } from "../../../lib/instance";

/**
 * The code step of a two-factor sign-in: `{code}` or `{recovery_code}`. The BFF
 * adds the challenge token from its httpOnly cookie and, on success, stores the
 * tokens like login and returns only `{user}`.
 */
export default defineEventHandler((event) => useBff().verifyTwoFactor(toWebRequest(event)));
