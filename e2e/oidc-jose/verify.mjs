// Verifies ID tokens with the jose library, apart from Navree.
//
// Usage: node verify.mjs <cases.json>
//
// <cases.json> is a list of { alg, token, jwks, issuer, audience, subject,
// nonce }. Each token is verified against its JWKS with the issuer, the
// audience and the algorithm pinned; its subject and nonce are checked.
// Prints [{ alg, sub, email }] and exits 0, or prints the first failure and
// exits 1.

import { readFileSync } from "node:fs";
import { createLocalJWKSet, decodeProtectedHeader, jwtVerify } from "jose";

const cases = JSON.parse(readFileSync(process.argv[2], "utf8"));
const results = [];

for (const item of cases) {
  const header = decodeProtectedHeader(item.token);
  if (header.alg !== item.alg) {
    console.error(`expected alg ${item.alg}, the token says ${header.alg}`);
    process.exit(1);
  }
  const keys = createLocalJWKSet(item.jwks);
  try {
    const { payload, protectedHeader } = await jwtVerify(item.token, keys, {
      issuer: item.issuer,
      audience: item.audience,
      algorithms: [item.alg],
      // The test clocks are fixed in the past.
      currentDate: new Date(payload_iat(item.token) * 1000 + 1000),
    });
    if (payload.sub !== item.subject) {
      throw new Error(`sub ${payload.sub}, expected ${item.subject}`);
    }
    if (payload.nonce !== item.nonce) {
      throw new Error(`nonce ${payload.nonce}, expected ${item.nonce}`);
    }
    results.push({ alg: protectedHeader.alg, sub: payload.sub, email: payload.email });
  } catch (error) {
    console.error(`${item.alg}: ${error.code ?? ""} ${error.message}`);
    process.exit(1);
  }
}

console.log(JSON.stringify(results));

/** The token's `iat`, read without verifying (only to set the clock). */
function payload_iat(token) {
  const payload = token.split(".")[1];
  return JSON.parse(Buffer.from(payload, "base64url").toString("utf8")).iat;
}
