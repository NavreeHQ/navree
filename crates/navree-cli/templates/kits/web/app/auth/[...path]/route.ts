import { navree } from "@/lib/navree";

/**
 * Social sign-in: `/auth/social/<provider>` starts it (a "Continue with ..." button links there)
 * and the provider sends the browser back to `/auth/callback/<provider>` (`social.redirect_base` in
 * the API's config/app.toml is that route without the provider). Both end like a password sign-in:
 * the tokens go into httpOnly cookies, a two-factor account continues at `/two-factor`, and a
 * failure lands on `/sign-in?error=...`.
 */
export const dynamic = "force-dynamic";
export const { GET } = navree.handlers;
