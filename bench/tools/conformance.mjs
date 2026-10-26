#!/usr/bin/env node
// Checks an implementation against SPEC.md. Usage:
//   node bench/tools/conformance.mjs http://127.0.0.1:8080
// Run it against a freshly reset database: it expects exactly 1,000 deals
// and creates one. Exits 1 when any check fails.
import { TOKEN, accessClaims, deal, sign, userId, world } from "./fixtures.mjs";

const base = (process.argv[2] ?? "http://127.0.0.1:8080").replace(/\/$/, "");
let failures = 0;
let passes = 0;

function check(name, ok, detail = "") {
  if (ok) {
    passes++;
  } else {
    failures++;
    console.log(`FAIL ${name}${detail ? `: ${detail}` : ""}`);
  }
  return ok;
}

async function call(method, path, { token, body, raw } = {}) {
  const headers = {};
  if (token) headers.authorization = `Bearer ${token}`;
  if (body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(base + path, {
    method,
    headers,
    body: body === undefined ? undefined : raw ? body : JSON.stringify(body),
  });
  const text = await res.text();
  let json;
  try {
    json = JSON.parse(text);
  } catch {
    json = undefined;
  }
  return { status: res.status, type: res.headers.get("content-type") ?? "", text, json };
}

const show = (value) => JSON.stringify(value)?.slice(0, 300);
// application/json, or a JSON subtype such as application/problem+json (RFC 9457).
const isJson = (r) => /^application\/([\w.-]+\+)?json\b/i.test(r.type.trim());
const isValidationStatus = (s) => s === 400 || s === 422;
const sameInstant = (a, b) => typeof a === "string" && Date.parse(a) === Date.parse(b);

function checkWorld(name, row) {
  return check(
    name,
    row &&
      Number.isInteger(row.id) &&
      row.id >= 1 &&
      row.id <= 10000 &&
      row.randomNumber === world(row.id).randomNumber &&
      Object.keys(row).length === 2,
    show(row),
  );
}

function checkDeal(name, got, want) {
  const ok =
    got &&
    got.id === want.id &&
    got.title === want.title &&
    got.amount_cents === want.amount_cents &&
    got.stage === want.stage &&
    got.owner?.id === want.owner.id &&
    got.owner?.name === want.owner.name &&
    sameInstant(got.created_at, want.created_at);
  return check(name, ok, `got ${show(got)} want ${show(want)}`);
}

async function main() {
  // 3.1 /json
  const json = await call("GET", "/json");
  check("json status", json.status === 200, `${json.status}`);
  check("json content-type", isJson(json), json.type);
  check("json body", json.json?.message === "Hello, World!" && Object.keys(json.json).length === 1, json.text);

  // 3.2 /db
  const ids = new Set();
  for (let i = 0; i < 20; i++) {
    const r = await call("GET", "/db");
    if (!check("db status", r.status === 200, `${r.status} ${r.text.slice(0, 200)}`)) break;
    check("db content-type", isJson(r), r.type);
    checkWorld("db row", r.json);
    ids.add(r.json?.id);
  }
  check("db ids are random", ids.size > 1, `${ids.size} distinct ids in 20 calls`);

  // 3.3 /queries
  for (const [query, len] of [
    ["?queries=20", 20],
    ["?queries=1", 1],
    ["?queries=0", 1],
    ["?queries=foo", 1],
    ["", 1],
    ["?queries=501", 500],
  ]) {
    const r = await call("GET", `/queries${query}`);
    const ok = check(`queries${query} status`, r.status === 200, `${r.status} ${r.text.slice(0, 200)}`);
    if (ok && check(`queries${query} length`, Array.isArray(r.json) && r.json.length === len, `${r.json?.length}`)) {
      r.json.slice(0, 20).forEach((row) => checkWorld(`queries${query} row`, row));
    }
  }

  // 3.4 /api/deals: authentication
  const unauthorized = {
    "no token": undefined,
    "bad signature": sign(accessClaims(), "another-secret-0123456789abcdef0123456789abcdef"),
    expired: sign(accessClaims({ iat: 1735689600, exp: 1767225600 })),
    "wrong issuer": sign(accessClaims({ iss: "someone-else" })),
    "unknown user": sign(accessClaims({ sub: userId(999) })),
    "not a jwt": "not-a-jwt",
  };
  for (const [label, token] of Object.entries(unauthorized)) {
    const r = await call("GET", "/api/deals", { token });
    check(`deals ${label} -> 401`, r.status === 401, `${r.status} ${r.text.slice(0, 200)}`);
  }

  // 3.4 list
  const list = await call("GET", "/api/deals", { token: TOKEN });
  if (check("deals status", list.status === 200, `${list.status} ${list.text.slice(0, 300)}`)) {
    check("deals content-type", isJson(list), list.type);
    const meta = list.json?.meta;
    check(
      "deals meta",
      meta?.current_page === 1 && meta?.per_page === 20 && meta?.total === 1000 && meta?.last_page === 50,
      show(meta),
    );
    check("deals data length", list.json?.data?.length === 20, `${list.json?.data?.length}`);
    checkDeal("deals first item", list.json?.data?.[0], deal(1000));
    checkDeal("deals order", list.json?.data?.[1], deal(999));
    checkDeal("deals 20th item", list.json?.data?.[19], deal(981));
  }

  const page3 = await call("GET", "/api/deals?page=3&per_page=5", { token: TOKEN });
  if (check("deals page 3 status", page3.status === 200, `${page3.status} ${page3.text.slice(0, 200)}`)) {
    const m = page3.json?.meta;
    check(
      "deals page 3 meta",
      m?.current_page === 3 && m?.per_page === 5 && m?.total === 1000 && m?.last_page === 200,
      show(m),
    );
    check("deals page 3 length", page3.json?.data?.length === 5);
    checkDeal("deals page 3 first", page3.json?.data?.[0], deal(990));
  }

  const last = await call("GET", "/api/deals?page=10&per_page=100", { token: TOKEN });
  if (check("deals last page status", last.status === 200, `${last.status}`)) {
    checkDeal("deals last page last item", last.json?.data?.[99], deal(1));
  }

  const beyond = await call("GET", "/api/deals?page=600", { token: TOKEN });
  check(
    "deals page past the end is empty",
    beyond.status === 200 && Array.isArray(beyond.json?.data) && beyond.json.data.length === 0 &&
      beyond.json?.meta?.last_page === 50,
    `${beyond.status} ${beyond.text.slice(0, 200)}`,
  );

  for (const query of ["?page=0", "?page=-1", "?page=abc", "?per_page=0", "?per_page=101", "?per_page=x"]) {
    const r = await call("GET", `/api/deals${query}`, { token: TOKEN });
    check(`deals${query} -> 400/422`, isValidationStatus(r.status), `${r.status} ${r.text.slice(0, 200)}`);
    check(`deals${query} error is JSON`, isJson(r), r.type);
  }

  // 3.5 create
  const noAuth = await call("POST", "/api/deals", { body: { title: "x", amount_cents: 1, stage: "lead" } });
  check("create without token -> 401", noAuth.status === 401, `${noAuth.status}`);

  const invalid = {
    "missing title": { amount_cents: 100, stage: "lead" },
    "empty title": { title: "", amount_cents: 100, stage: "lead" },
    "long title": { title: "x".repeat(201), amount_cents: 100, stage: "lead" },
    "negative amount": { title: "Deal", amount_cents: -1, stage: "lead" },
    "fractional amount": { title: "Deal", amount_cents: 10.5, stage: "lead" },
    "huge amount": { title: "Deal", amount_cents: 1000000000001, stage: "lead" },
    "missing amount": { title: "Deal", stage: "lead" },
    "unknown stage": { title: "Deal", amount_cents: 100, stage: "bogus" },
    "missing stage": { title: "Deal", amount_cents: 100 },
  };
  for (const [label, body] of Object.entries(invalid)) {
    const r = await call("POST", "/api/deals", { token: TOKEN, body });
    check(`create ${label} -> 400/422`, isValidationStatus(r.status), `${r.status} ${r.text.slice(0, 200)}`);
  }
  const malformed = await call("POST", "/api/deals", { token: TOKEN, body: "{not json", raw: true });
  check("create malformed JSON -> 400/422", isValidationStatus(malformed.status), `${malformed.status}`);

  const before = Date.now();
  const created = await call("POST", "/api/deals", {
    token: TOKEN,
    body: { title: "Benchmark deal", amount_cents: 1000000000000, stage: "proposal" },
  });
  if (check("create status 201", created.status === 201, `${created.status} ${created.text.slice(0, 300)}`)) {
    const d = created.json;
    check("create content-type", isJson(created), created.type);
    check("create id is a uuid", /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(d?.id ?? ""), d?.id);
    check(
      "create body",
      d?.title === "Benchmark deal" && d?.amount_cents === 1000000000000 && d?.stage === "proposal" &&
        d?.owner?.id === userId(1) && d?.owner?.name === "User 1",
      show(d),
    );
    const at = Date.parse(d?.created_at);
    check("create created_at is now", Math.abs(at - before) < 60_000, d?.created_at);

    const newest = await call("GET", "/api/deals?per_page=1", { token: TOKEN });
    check("create is persisted", newest.json?.data?.[0]?.id?.toLowerCase() === d?.id?.toLowerCase(), show(newest.json?.data?.[0]));
    check("create total", newest.json?.meta?.total === 1001, show(newest.json?.meta));
  }

  console.log(`${failures === 0 ? "PASS" : "FAIL"}: ${passes} checks passed, ${failures} failed`);
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((error) => {
  console.log(`FAIL: ${error.message}`);
  process.exit(1);
});
