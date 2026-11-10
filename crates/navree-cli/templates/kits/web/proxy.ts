import { navree } from "@/lib/navree";

/**
 * Runs before the admin panel's pages (`/admin`): a browser without a session goes to the sign-in
 * page (`pages.signIn` in `lib/navree.ts`) and comes back, and an expired access token is refreshed
 * before the panel loads. The app's own screens check the session in the browser
 * (`app/(app)/layout.tsx`).
 */
export const proxy = navree.proxy();

export const config = { matcher: ["/admin/:path*"] };
