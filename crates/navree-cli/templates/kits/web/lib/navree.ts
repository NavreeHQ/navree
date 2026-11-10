import { createNavree } from "@navree/next/server";

/**
 * The app's Navree: the backend-for-frontend behind `app/api`, `app/admin`, `app/auth` and
 * `app/broadcasting`, and `auth()` and `client()` for server components. Settings come from the
 * `NAVREE_*` variables when the first request arrives, so `NAVREE_API_URL` can change without a
 * rebuild (see `.env.example`).
 *
 * `pages` are the app's own pages that the backend-for-frontend's redirects end on: social
 * sign-in, the link in the verification mail and the admin panel (`/admin`, served by the API),
 * whose page requests go back to the sign-in page when the session ends.
 */
export const navree = createNavree({
  pages: { signIn: "/sign-in", twoFactor: "/two-factor", verifyEmail: "/verify-email" },
  redirectNavigations: true,
});
