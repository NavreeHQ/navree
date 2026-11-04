import { createNavree } from "@navree/next/server";

/**
 * The app's Navree: the backend-for-frontend behind `app/api`, `app/admin` and
 * `app/broadcasting`, and `auth()` and `client()` for server components. Settings
 * come from the `NAVREE_*` variables when the first request arrives, so
 * `NAVREE_API_URL` can change without a rebuild (see `.env.example`).
 */
export const navree = createNavree();
