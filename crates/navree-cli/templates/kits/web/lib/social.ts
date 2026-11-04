/**
 * The routes only this kit has: social sign-in (`/auth/social/<provider>` and
 * `/auth/callback/<provider>`) and the link in the verification mail. They talk to the API
 * through the app's backend-for-frontend (`navree.bff().api(...)`) and end like a password
 * sign-in: the tokens go into httpOnly cookies (the same names as the BFF's), a two-factor account
 * continues at `/two-factor`, and the browser lands on a page of this app.
 *
 * Each takes the BFF as its first argument, so tests can give them one with a scripted API.
 */
import { bffOptionsFromEnv, type Bff } from "@navree/next/server";

/** The social sign-in state, between the redirect to the provider and the callback. */
export const SOCIAL_STATE_COOKIE = "navree_social";
/** Only the callback routes receive the state cookie. */
const CALLBACK_PATH = "/auth/callback";
/** Only the two-factor code step receives the challenge cookie (the BFF's `POST /api/auth/2fa/verify`). */
const CHALLENGE_PATH = "/api/auth/2fa";
/** How long the refresh token's cookie lives; Navree's refresh tokens last 30 days. */
const REFRESH_MAX_AGE = 60 * 60 * 24 * 30;

interface SocialLogin {
  access_token?: string;
  refresh_token?: string;
  expires_in?: number;
  two_factor_required?: boolean;
  challenge_token?: string;
  return_to?: string;
}

/** `Secure` follows the BFF: on in production, and `NAVREE_COOKIE_SECURE=0` turns it off. */
function secureCookies(): boolean {
  return bffOptionsFromEnv().secureCookies ?? process.env.NODE_ENV === "production";
}

function cookie(name: string, value: string, maxAge: number, path = "/"): string {
  const parts = [`${name}=${encodeURIComponent(value)}`, `Path=${path}`, "HttpOnly", "SameSite=Lax", `Max-Age=${maxAge}`];
  if (secureCookies()) parts.push("Secure");
  return parts.join("; ");
}

function readCookie(request: Request, name: string): string | undefined {
  for (const part of (request.headers.get("cookie") ?? "").split(";")) {
    const eq = part.indexOf("=");
    if (eq !== -1 && part.slice(0, eq).trim() === name) {
      const value = part.slice(eq + 1).trim();
      try {
        return decodeURIComponent(value);
      } catch {
        return value;
      }
    }
  }
  return undefined;
}

/** A `303 See Other` to `location` on this origin, with `headers` (cookies). */
function redirect(location: string, headers = new Headers()): Response {
  headers.set("location", location);
  return new Response(null, { status: 303, headers });
}

/** `next` when it is a path on this origin, else `/` (never an open redirect). */
function localPath(next: string | null | undefined): string {
  return next && next.startsWith("/") && !next.startsWith("//") && !next.startsWith("/\\") ? next : "/";
}

/** Reads a body that is not wanted, so the connection is released. */
async function discard(response: Response): Promise<void> {
  await response.arrayBuffer().catch(() => undefined);
}

/**
 * `GET /auth/social/{provider}?next=/w/acme`: starts a social sign-in. Navree answers with the
 * provider's URL and a sealed state; the state waits in an httpOnly cookie that only the
 * callback route receives, and the browser goes to the provider.
 */
export async function socialRedirect(bff: Bff, request: Request, provider: string): Promise<Response> {
  const next = localPath(new URL(request.url).searchParams.get("next"));
  let upstream: Response;
  try {
    upstream = await bff.api(
      `/api/auth/social/${encodeURIComponent(provider)}/redirect?return_to=${encodeURIComponent(next)}`,
    );
  } catch {
    return redirect("/sign-in?error=api_unreachable");
  }
  if (!upstream.ok) {
    await discard(upstream);
    return redirect("/sign-in?error=social_unavailable");
  }
  const { url, state } = (await upstream.json().catch(() => ({}))) as { url?: string; state?: string };
  if (!url || !state) return redirect("/sign-in?error=social_unavailable");
  const headers = new Headers();
  headers.append("set-cookie", cookie(SOCIAL_STATE_COOKIE, state, 600, CALLBACK_PATH));
  return redirect(url, headers);
}

/**
 * `GET /auth/callback/{provider}?code&state`: the provider sends the browser back here. The state
 * must match the cookie; Navree exchanges the code and answers like a password sign-in (tokens into
 * the cookies, then the page the sign-in started from) or with a two-factor challenge (the
 * `/two-factor` page finishes it).
 */
export async function socialCallback(bff: Bff, request: Request, provider: string): Promise<Response> {
  const params = new URL(request.url).searchParams;
  const code = params.get("code");
  const state = params.get("state");
  const expected = readCookie(request, SOCIAL_STATE_COOKIE);
  const headers = new Headers();
  headers.append("set-cookie", cookie(SOCIAL_STATE_COOKIE, "", 0, CALLBACK_PATH));
  if (!code || !state || !expected || state !== expected) return redirect("/sign-in?error=social_state", headers);

  let upstream: Response;
  try {
    upstream = await bff.api(`/api/auth/social/${encodeURIComponent(provider)}/callback`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code, state }),
    });
  } catch {
    return redirect("/sign-in?error=api_unreachable", headers);
  }
  if (!upstream.ok) {
    await discard(upstream);
    return redirect("/sign-in?error=social_failed", headers);
  }
  const data = (await upstream.json().catch(() => ({}))) as SocialLogin;
  const next = localPath(data.return_to);
  const names = bff.cookieNames;
  if (data.two_factor_required === true && typeof data.challenge_token === "string") {
    const maxAge = Math.max(1, Math.floor(data.expires_in ?? 300));
    headers.append("set-cookie", cookie(names.challenge, data.challenge_token, maxAge, CHALLENGE_PATH));
    return redirect(`/two-factor?next=${encodeURIComponent(next)}`, headers);
  }
  if (typeof data.access_token !== "string" || typeof data.refresh_token !== "string") {
    return redirect("/sign-in?error=social_failed", headers);
  }
  headers.append("set-cookie", cookie(names.access, data.access_token, Math.max(1, Math.floor(data.expires_in ?? 900))));
  headers.append("set-cookie", cookie(names.refresh, data.refresh_token, REFRESH_MAX_AGE));
  return redirect(next, headers);
}

/**
 * `GET /api/auth/email/verify/{id}/{hash}?expires&signature`: the link in the verification mail.
 * Navree checks the signature and marks the address verified; the browser lands on
 * `/verify-email?status=verified` (or `invalid`).
 */
export async function verifyEmail(bff: Bff, request: Request): Promise<Response> {
  const url = new URL(request.url);
  try {
    const upstream = await bff.api(`${url.pathname}${url.search}`);
    await discard(upstream);
    return redirect(`/verify-email?status=${upstream.ok ? "verified" : "invalid"}`);
  } catch {
    return redirect("/verify-email?status=unreachable");
  }
}
