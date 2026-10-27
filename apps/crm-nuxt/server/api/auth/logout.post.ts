import { useBff } from "../../lib/instance";

/** Revokes the refresh token at Navree (`{all: true}` signs out every device) and clears the cookies. */
export default defineEventHandler((event) => useBff().logout(toWebRequest(event)));
