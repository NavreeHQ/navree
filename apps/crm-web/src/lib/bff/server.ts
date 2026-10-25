import { createBff } from "./bff";

/**
 * The app's BFF instance. Read at request time on the server, so
 * `KEEL_API_URL` can change without a rebuild.
 */
export const bff = createBff({
  apiUrl: process.env.KEEL_API_URL ?? "http://127.0.0.1:8080",
  // Secure cookies in production; set KEEL_COOKIE_SECURE=0 to serve a production build over plain http
  // on a host other than localhost.
  secureCookies:
    process.env.KEEL_COOKIE_SECURE !== undefined
      ? process.env.KEEL_COOKIE_SECURE === "1"
      : process.env.NODE_ENV === "production",
  // Public origin(s) when the app sits behind a proxy that rewrites Host, e.g. https://crm.example.com
  allowedOrigins: process.env.APP_ORIGINS?.split(",")
    .map((o) => o.trim())
    .filter(Boolean),
});
