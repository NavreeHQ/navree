import { navree } from "@/lib/navree";

/**
 * Sign-in, two-factor, sign-out and refresh are answered here (the tokens stay in httpOnly
 * cookies); every other `/api/*` call is forwarded to Navree with the bearer token.
 */
export const dynamic = "force-dynamic";
export const { GET, POST, PUT, PATCH, DELETE, HEAD } = navree.handlers;
