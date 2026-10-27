import { useBff } from "../../lib/instance";

/** Rotates the token cookies with the refresh cookie. The browser never sees either token. */
export default defineEventHandler((event) => useBff().refreshSession(toWebRequest(event)));
