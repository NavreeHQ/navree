---
name: navree-jose
description: "JSON Web Signatures and JSON Web Keys with `navree::crypto::jose` (crate navree-crypto, facade feature `crypto`, in `full`): `SigningKey` (`from_pem` for PKCS#8, PKCS#1 and SEC 1 keys such as Apple `.p8` files, `from_jwk`, `hmac`, `generate_ed25519`, `generate_es256`, `with_kid`, `with_thumbprint_kid`, `with_algorithm`, `sign_jwt`, `sign_json`, `sign_jws`, `to_pkcs8_pem`, `public_jwk`), `VerifyingKey` (`from_pem`, `from_certificate_der`, `from_jwk`, `rsa`, `ec`, `ed25519`, `hmac`, `verify_der`, `verify_legacy_sha1`), `Jws::parse`, `verify`, `verify_with(&JwkSet)` and `claims::<T>()`, `Jwk` and `JwkSet` (thumbprints, `to_public`), the algorithms HS256/384/512, EdDSA, ES256/384, RS256/384/512 and PS256/384/512, and `JoseError`. Use when signing a JWT another service verifies (a partner assertion, Apple's client secret, a service account's token), verifying a token another service signed (ID tokens against a provider's JWKS, OIDC push tokens, a partner's callback), publishing a JWKS endpoint, loading or generating a private key, or debugging `NoKey`, `BadSignature`, `UnsupportedAlgorithm` or `InvalidKey`."
---

# Signatures and keys

`navree::crypto::jose` signs and verifies compact JWS and reads and publishes
JWKs, on `ring`. It is a set of types, not a module: nothing to register.
Everything below is in the code: when in doubt read
`crates/navree-crypto/src/jose/`, its tests in
`crates/navree-crypto/tests/crypto/jose/` (RFC vectors, tokens from another
implementation, real provider key sets) and `crates/navree/tests/jose_docs.rs`
(the docs page's examples through the facade). Docs page:
`/docs/security/signatures-and-keys/`.

Not for checking Navree's own access tokens: those are navree-auth's HS256
JWTs, written with this module but checked by `JwtService` (load
`navree-auth`). Social sign-in and the OAuth server already use this module
for ID tokens; reach for it directly when your own code signs or verifies.

```rust
use navree::crypto::jose::{Algorithm, Header, JoseError, Jwk, JwkSet, Jws, SigningKey, VerifyingKey};
```

## Steps: sign a token another service verifies

1. Put the private key in the environment (a PEM with literal `\n` works, as
   does the base64 body without armor lines), read it into a config section.
2. Load and sign:

```rust
pub fn partner_assertion(pem: &str, key_id: &str, now: i64) -> Result<String, JoseError> {
    let key = SigningKey::from_pem(pem)?.with_kid(key_id);
    key.sign_jwt(&serde_json::json!({
        "iss": "acme",
        "aud": "https://api.partner.example",
        "iat": now,
        "exp": now + 300,
    }))
}
```

   `sign_jwt` uses the header `{alg, kid, typ: "JWT"}`; the algorithm comes
   from the key (Ed25519 -> EdDSA, P-256 -> ES256, P-384 -> ES384, RSA ->
   RS256, change with `with_algorithm(Algorithm::PS256)`). For another header
   use `key.sign_json(&key.header(), &claims)` (`{alg, kid}`, what Apple
   wants) or build one with `Header::new(alg).with_kid(..).with_typ(..)
   .with_param(name, value)`. `sign_jws` signs raw bytes; `sign` returns a raw
   signature.
3. Test it: sign, then `Jws::parse(&token)?.verify(&key.verifying_key())`
   and compare `claims::<Value>()`.

## Steps: verify a token

1. Get the trusted key: `VerifyingKey::from_pem` (`PUBLIC KEY`,
   `RSA PUBLIC KEY` or the key of a `CERTIFICATE`), `from_certificate_der`,
   `from_jwk`, `rsa(n, e)`, `ec(Algorithm::ES256, x, y)`, `ed25519(bytes)`
   or `hmac(alg, secret)`. A certificate's own signature and validity are
   not checked: trust it for where it came from.
2. Parse, verify, then read the claims, in that order:

```rust,ignore
let jws = Jws::parse(token)?;            // refuses `none`, unknown algs, `crit`
jws.verify(&key)?;                       // NoKey (wrong key type/alg) or BadSignature
let claims: MyClaims = jws.claims()?;    // Payload when the JSON does not fit
```

3. Check `exp`, `nbf`, `aud` and `iss` yourself: signatures say who signed,
   not whether the token is still valid.

For a provider's key set, fetch the JWKS with `HttpClient`, cache it with
`Cache::remember` (an hour), and call `jws.verify_with(&keys)`: it picks the
key by `kid` (or tries every key of the type when the token has none), skips
keys whose `use`, `key_ops` or `alg` don't fit, and refuses HMAC tokens. On
`NoKey` with a `kid` the cached set lacks (`keys.has_kid(kid)`), refetch, at
most every 30 seconds or so. The full service is on the docs page.

## Steps: publish a JWKS

```rust
pub struct TokenKeys(pub Vec<SigningKey>);   // current first, then previous

#[injectable]
pub struct JwksController {
    keys: Arc<TokenKeys>,
}

#[controller("/tokens")]
impl JwksController {
    #[get("!/.well-known/jwks.json")]
    #[public]
    async fn jwks(&self) -> Json<JwkSet> {
        Json(self.keys.0.iter().filter_map(SigningKey::public_jwk).collect())
    }
}
```

Register `TokenKeys` with `m.value(TokenKeys(keys))` in the module's
`register` function. Give keys stable ids with `with_thumbprint_kid()`
(RFC 7638), keep a rotated-out key in the set until its tokens expire.
`public_jwk()` never includes private members (`None` for HMAC keys);
`Jwk::to_public()` and `JwkSet::to_public()` strip them from JWKs read
elsewhere.

## Keys

- Loading: `SigningKey::from_pem` (PKCS#8 `PRIVATE KEY`: Ed25519, P-256,
  P-384, RSA; `RSA PRIVATE KEY`; `EC PRIVATE KEY`), `from_pkcs8_der`,
  `from_jwk` (private JWK; its `kid` and `alg` carry over), `hmac(alg,
  secret)` (secret at least 32/48/64 bytes for HS256/384/512),
  `ed25519_from_seed(&[u8; 32])`.
- Generating: `SigningKey::generate_ed25519()`, `generate_es256()`; store
  with `to_pkcs8_pem()` (Ed25519 exports the 48-byte PKCS#8 v1 form). RSA
  keys cannot be generated here.
- WebAuthn ES256 signatures are DER: verify them with
  `verifying_key.verify_der(Algorithm::ES256, message, signature)`.
- RSA PKCS#1 v1.5 with SHA-1 (AWS SNS signature version 1) is no JWS
  algorithm: `verifying_key.verify_legacy_sha1(message, signature)`, RSA
  keys not pinned to an algorithm only.

## Checks

- `cargo test` with a round trip: sign with the key, verify with its public
  half or the published set; a tampered payload must give `BadSignature`.
- `cargo tree -i rsa`, `-i p256`, `-i ed25519-dalek` print nothing: the
  crypto policy keeps them out, and `crates/navree/tests/dependency_graph.rs`
  fails if `Cargo.lock` lists them.

## Common mistakes

- Reading claims before `verify` succeeded: until then they are whatever the
  sender wrote. Use only the header's `kid` to find the key.
- Verifying with a key built from the token's own header (`jwk`, `jku`,
  `x5u`): never trust keys the token names; use a configured key or set.
- Expecting `verify_with` to accept HS256: a key set is public, so HMAC
  tokens need `VerifyingKey::hmac` and an explicit `verify`.
- `NoKey` from `sign_jws`/`sign_json`: the header's `alg` is not the key's.
- `InvalidKey("an HMAC secret must be at least as long as the hash output")`:
  use a 32-byte secret for HS256 (64 for HS512).
- Checking only the signature: also check `exp`, `aud` and `iss`.
- Logging keys: `Debug` of `SigningKey`, `VerifyingKey` and `Jwk` is
  redacted, but a PEM string is not.
