import type { DiagramSpec } from "./figure";

/**
 * The two ways to stay signed in. Wide: the first group is the token flow of
 * an API client, from the sign-in to the pair of tokens (the accent), a call
 * with the access token, its expiry and the refresh that returns a new pair;
 * an old refresh token used again ends the whole session. The second group
 * is a browser's session cookie. Narrow: the same steps as two columns.
 */
export const tokenAndSession: DiagramSpec = {
  title: "Token and session flow",
  desc: "An API client signs in with POST /api/auth/login and receives two tokens: an access token, a JWT valid for 15 minutes, and a refresh token valid for 30 days. It calls the API with the access token in an Authorization: Bearer header, which the server checks without a database lookup. When the access token expires the API answers 401 Token expired, and the client posts its refresh token to POST /api/auth/refresh. That returns a new pair, and the old refresh token is revoked. If an old refresh token is presented again, the whole session ends and the user signs in again. A browser signs in on a session instead: the sign-in gives the session a new id, an encrypted HttpOnly cookie carries the id, and every state-changing request that carries the cookie must also send the session's CSRF token.",
  legend: [
    { kind: "flow", label: "the normal path" },
    { kind: "async", label: "a refresh token used twice" },
  ],
  wide: {
    width: 592,
    height: 360,
    groups: [
      { id: "tokens", x: 0, y: 0, w: 592, h: 216, label: "Bearer tokens" },
      { id: "browser", x: 0, y: 248, w: 592, h: 112, label: "Browser session" },
    ],
    nodes: [
      { id: "signin", x: 16, y: 40, w: 176, title: "Sign in", sub: "POST /api/auth/login", align: "center" },
      { id: "pair", x: 208, y: 40, w: 176, title: "Access and refresh", sub: "15 minutes, 30 days", align: "center", tone: "accent" },
      { id: "call", x: 400, y: 40, w: 176, title: "Call the API", sub: "Authorization: Bearer", align: "center" },
      { id: "expired", x: 400, y: 144, w: 176, title: "Access token expires", sub: "401 Token expired", align: "center" },
      { id: "refresh", x: 208, y: 144, w: 176, title: "Refresh", sub: "POST /api/auth/refresh", align: "center" },
      { id: "reuse", x: 16, y: 144, w: 176, title: "Token used twice", sub: "the whole session ends", align: "center", tone: "muted" },
      { id: "bsignin", x: 16, y: 288, w: 176, title: "Browser sign-in", sub: "a new id and CSRF token", align: "center" },
      { id: "cookie", x: 208, y: 288, w: 176, title: "Session cookie", sub: "HttpOnly, encrypted id", align: "center", tone: "accent" },
      { id: "csrf", x: 400, y: 288, w: 176, title: "CSRF token", sub: "on every state change", align: "center" },
    ],
    edges: [
      { from: "signin.right", to: "pair.left", step: 0 },
      { from: "pair.right", to: "call.left", step: 1 },
      { from: "call.bottom", to: "expired.top", step: 2 },
      { from: "expired.left", to: "refresh.right", step: 3 },
      { from: "refresh.top", to: "pair.bottom", step: 4, label: "a new pair", labelMono: false, at: [304, 132] },
      { from: "refresh.left", to: "reuse.right", kind: "async" },
      { from: "bsignin.right", to: "cookie.left", step: 5 },
      { from: "cookie.right", to: "csrf.left", step: 6 },
    ],
  },
  narrow: {
    width: 272,
    height: 760,
    groups: [
      { id: "tokens", x: 0, y: 0, w: 272, h: 448, label: "Bearer tokens" },
      { id: "browser", x: 0, y: 480, w: 272, h: 280, label: "Browser session" },
    ],
    nodes: [
      { id: "signin", x: 16, y: 40, w: 240, title: "Sign in", sub: "POST /api/auth/login", align: "center" },
      { id: "pair", x: 16, y: 124, w: 240, title: "Access and refresh", sub: "15 minutes, 30 days", align: "center", tone: "accent" },
      { id: "call", x: 16, y: 208, w: 240, title: "Call the API", sub: "Authorization: Bearer", align: "center" },
      { id: "refresh", x: 16, y: 292, w: 240, title: "Refresh when it expires", sub: "a new pair, the old one revoked", align: "center" },
      { id: "reuse", x: 16, y: 376, w: 240, title: "Token used twice", sub: "the whole session ends", align: "center", tone: "muted" },
      { id: "bsignin", x: 16, y: 520, w: 240, title: "Browser sign-in", sub: "a new id and CSRF token", align: "center" },
      { id: "cookie", x: 16, y: 604, w: 240, title: "Session cookie", sub: "HttpOnly, encrypted id", align: "center", tone: "accent" },
      { id: "csrf", x: 16, y: 688, w: 240, title: "CSRF token", sub: "on every state change", align: "center" },
    ],
    edges: [
      { from: "signin.bottom", to: "pair.top", step: 0 },
      { from: "pair.bottom", to: "call.top", step: 1 },
      { from: "call.bottom", to: "refresh.top", step: 2 },
      { from: "refresh.bottom", to: "reuse.top", kind: "async" },
      { from: "bsignin.bottom", to: "cookie.top", step: 3 },
      { from: "cookie.bottom", to: "csrf.top", step: 4 },
    ],
  },
};
