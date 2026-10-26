// The benchmark's shared secret, tokens and seed formulas (SPEC.md §2, §3.4).
import { createHash, createHmac } from "node:crypto";

export const JWT_SECRET = "keel-bench-jwt-secret-0123456789abcdef0123456789";
export const JWT_ISSUER = "keel";

export const STAGES = ["lead", "qualified", "proposal", "negotiation", "won", "lost"];
const SEED_EPOCH = Date.UTC(2026, 0, 1);

/** `md5(text)::uuid` as PostgreSQL computes it. */
export function md5Uuid(text) {
  const h = createHash("md5").update(text).digest("hex");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

export const userId = (i) => md5Uuid(`user-${i}`);

export function world(id) {
  return { id, randomNumber: ((id * 7919) % 10000) + 1 };
}

export function deal(i) {
  const owner = (i % 100) + 1;
  return {
    id: md5Uuid(`deal-${i}`),
    title: `Deal ${i}`,
    amount_cents: (i * 104729) % 100000000,
    stage: STAGES[i % 6],
    owner: { id: userId(owner), name: `User ${owner}` },
    created_at: new Date(SEED_EPOCH + i * 60_000).toISOString(),
  };
}

const b64url = (value) => Buffer.from(value).toString("base64url");

/** An HS256 JWT with Keel's access-token claims. */
export function sign(claims, secret = JWT_SECRET) {
  const head = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" }));
  const body = b64url(JSON.stringify(claims));
  const sig = createHmac("sha256", secret).update(`${head}.${body}`).digest("base64url");
  return `${head}.${body}.${sig}`;
}

export function accessClaims(overrides = {}) {
  return {
    sub: userId(1),
    iat: 1767225600, // 2026-01-01
    exp: 1893456000, // 2030-01-01
    iss: JWT_ISSUER,
    jti: "bench-token-1",
    typ: "access",
    ...overrides,
  };
}

/** The token every load test sends: user 1, valid until 2030. */
export const TOKEN = sign(accessClaims());

// Run directly: print the load-test token.
if (import.meta.url === `file://${process.argv[1]}` || process.argv[1]?.endsWith("fixtures.mjs")) {
  console.log(TOKEN);
}
