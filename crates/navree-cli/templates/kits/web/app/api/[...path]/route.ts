import { navree } from "@/lib/navree";

/**
 * Sign-in, two-factor, sign-out and refresh are answered here, with the tokens kept in httpOnly
 * cookies; every other `/api/*` call goes to the Navree API with the bearer token from the cookie
 * (and the browser client's `X-Workspace`).
 */
export const dynamic = "force-dynamic";
export const { GET, POST, PUT, PATCH, DELETE, HEAD } = navree.handlers;
