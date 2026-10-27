import { useBff } from "../../lib/instance";

/** Private-channel authorization for pusher-js: forwarded to Navree with the bearer token. */
export default defineEventHandler((event) => useBff().proxy(toWebRequest(event), "/broadcasting/auth"));
