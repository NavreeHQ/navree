import { navree } from "@/lib/navree";

/**
 * The Helm admin panel (`/admin`, for super admins), served by the API and forwarded with the
 * same session, so it opens on this origin. `proxy.ts` sends a browser without a session to the
 * sign-in page first.
 */
export const dynamic = "force-dynamic";
export const { GET, POST, PUT, PATCH, DELETE, HEAD } = navree.handlers;
