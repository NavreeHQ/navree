import { useBff } from "../../lib/instance";

/** Signs in at Navree, keeps the tokens in httpOnly cookies, returns `{user}` (or a two-factor challenge). */
export default defineEventHandler((event) => useBff().login(toWebRequest(event)));
