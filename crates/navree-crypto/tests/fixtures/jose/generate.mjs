// Generates `independent-vectors.json`: keys and compact JWS made with
// Node's crypto module, an implementation independent of `ring`, so the
// jose tests check interoperability and not only round trips.
//
//   node crates/navree-crypto/tests/fixtures/jose/generate.mjs
//
// Keys are throwaway test keys. Re-running changes every value.
import { createHmac, generateKeyPairSync, randomBytes, sign } from "node:crypto";
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const b64url = (buf) => Buffer.from(buf).toString("base64url");
const claims = { iss: "https://issuer.test", sub: "user-42", aud: "client-1", iat: 1761436800, exp: 4102444800 };

function pem(label, der) {
  const body = der.toString("base64").match(/.{1,64}/g).join("\n");
  return `-----BEGIN ${label}-----\n${body}\n-----END ${label}-----\n`;
}

/** DER length octets. */
function len(n) {
  if (n < 0x80) return Buffer.from([n]);
  if (n < 0x100) return Buffer.from([0x81, n]);
  return Buffer.from([0x82, n >> 8, n & 0xff]);
}
function tlv(tag, body) {
  return Buffer.concat([Buffer.from([tag]), len(body.length), body]);
}

/**
 * Re-encodes a P-256 PKCS#8 document the way Apple's `.p8` files are laid
 * out: the inner ECPrivateKey also carries the `[0]` curve parameters.
 */
function appleLayout(pkcs8, publicPoint) {
  // PrivateKeyInfo ::= SEQUENCE { version, AlgorithmIdentifier, OCTET STRING }
  const algId = Buffer.from("301306072a8648ce3d020106082a8648ce3d030107", "hex");
  // Find the 32-byte private scalar: `04 20 <d>` inside the ECPrivateKey.
  const at = pkcs8.indexOf(Buffer.from("0420", "hex"), 20);
  const d = pkcs8.subarray(at + 2, at + 34);
  const ecPrivateKey = tlv(
    0x30,
    Buffer.concat([
      Buffer.from("020101", "hex"),
      tlv(0x04, d),
      tlv(0xa0, Buffer.from("06082a8648ce3d030107", "hex")),
      tlv(0xa1, tlv(0x03, Buffer.concat([Buffer.from([0]), publicPoint]))),
    ]),
  );
  return tlv(0x30, Buffer.concat([Buffer.from("020100", "hex"), algId, tlv(0x04, ecPrivateKey)]));
}

function jws(header, sig) {
  const input = `${b64url(JSON.stringify(header))}.${b64url(JSON.stringify(claims))}`;
  return `${input}.${b64url(sig(Buffer.from(input)))}`;
}

const out = { claims };

// Ed25519 (EdDSA).
{
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const jwk = publicKey.export({ format: "jwk" });
  out.ed25519 = {
    private_pem: privateKey.export({ format: "pem", type: "pkcs8" }),
    public_pem: publicKey.export({ format: "pem", type: "spki" }),
    public_jwk: jwk,
    private_jwk: privateKey.export({ format: "jwk" }),
    jws: jws({ alg: "EdDSA", kid: "ed-1" }, (m) => sign(null, m, privateKey)),
  };
}

// P-256 (ES256), with an Apple-style `.p8`.
{
  const { privateKey, publicKey } = generateKeyPairSync("ec", { namedCurve: "P-256" });
  const jwk = publicKey.export({ format: "jwk" });
  const point = Buffer.concat([Buffer.from([4]), Buffer.from(jwk.x, "base64url"), Buffer.from(jwk.y, "base64url")]);
  const der = privateKey.export({ format: "der", type: "pkcs8" });
  out.p256 = {
    private_pem: privateKey.export({ format: "pem", type: "pkcs8" }),
    apple_p8: pem("PRIVATE KEY", appleLayout(der, point)),
    public_pem: publicKey.export({ format: "pem", type: "spki" }),
    public_jwk: jwk,
    private_jwk: privateKey.export({ format: "jwk" }),
    jws: jws({ alg: "ES256", kid: "ec-1" }, (m) => sign("sha256", m, { key: privateKey, dsaEncoding: "ieee-p1363" })),
  };
}

// P-384 (ES384).
{
  const { privateKey, publicKey } = generateKeyPairSync("ec", { namedCurve: "P-384" });
  out.p384 = {
    private_pem: privateKey.export({ format: "pem", type: "pkcs8" }),
    public_pem: publicKey.export({ format: "pem", type: "spki" }),
    public_jwk: publicKey.export({ format: "jwk" }),
    jws: jws({ alg: "ES384", kid: "ec-384" }, (m) => sign("sha384", m, { key: privateKey, dsaEncoding: "ieee-p1363" })),
  };
}

// RSA 2048: RS256/384/512 and PS256/384/512.
{
  const { privateKey, publicKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
  const tokens = {};
  for (const bits of [256, 384, 512]) {
    tokens[`RS${bits}`] = jws({ alg: `RS${bits}`, kid: "rsa-1" }, (m) => sign(`sha${bits}`, m, privateKey));
    tokens[`PS${bits}`] = jws({ alg: `PS${bits}`, kid: "rsa-1" }, (m) =>
      sign(`sha${bits}`, m, { key: privateKey, padding: 6 /* RSA_PKCS1_PSS_PADDING */, saltLength: bits / 8 }),
    );
  }
  out.rsa = {
    private_pem: privateKey.export({ format: "pem", type: "pkcs8" }),
    public_pem: publicKey.export({ format: "pem", type: "spki" }),
    public_jwk: publicKey.export({ format: "jwk" }),
    jws: tokens,
  };
}

// HMAC: HS256/384/512 with a 64-byte secret.
{
  const secret = randomBytes(64);
  const tokens = {};
  for (const bits of [256, 384, 512]) {
    tokens[`HS${bits}`] = jws({ alg: `HS${bits}`, typ: "JWT" }, (m) => createHmac(`sha${bits}`, secret).update(m).digest());
  }
  out.hmac = { secret: b64url(secret), jws: tokens };
}

writeFileSync(join(here, "independent-vectors.json"), `${JSON.stringify(out, null, 2)}\n`);
console.log("wrote independent-vectors.json");
