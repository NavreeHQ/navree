//! JSON Web Key Sets: the published sets of real providers, publishing
//! your own, and the tolerance rules of RFC 7517 §5.

use std::collections::BTreeSet;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use navree_crypto::jose::{Algorithm, Jwk, JwkSet, Jws, SigningKey, VerifyingKey};
use serde_json::{Value, json};

use super::{fixture, json_fixture};

fn provider(name: &str) -> JwkSet {
    serde_json::from_str(&fixture(&format!("{name}-jwks.json"))).unwrap()
}

#[test]
fn provider_key_sets_parse_into_verifying_keys() {
    for name in ["google", "apple", "microsoft"] {
        let set = provider(name);
        let raw = json_fixture(&format!("{name}-jwks.json"));
        assert_eq!(
            set.keys.len(),
            raw["keys"].as_array().unwrap().len(),
            "{name}"
        );
        assert!(!set.keys.is_empty(), "{name}");
        let mut kids = BTreeSet::new();
        for jwk in &set.keys {
            assert_eq!(jwk.kty, "RSA", "{name}");
            assert!(!jwk.is_private());
            let kid = jwk.kid.as_deref().unwrap();
            assert!(kids.insert(kid.to_owned()), "{name}: duplicate kid {kid}");
            assert!(set.has_kid(kid));
            assert_eq!(set.find(kid), Some(jwk));
            let key = VerifyingKey::from_jwk(jwk).unwrap();
            assert_eq!(key.kid(), Some(kid));
            assert!(key.accepts(Algorithm::RS256), "{name} {kid}");
            assert_eq!(jwk.thumbprint().unwrap().len(), 43);
        }
        assert!(!set.has_kid("not-a-kid"));
    }
}

#[test]
fn provider_specific_members_survive_a_round_trip() {
    // Microsoft adds `x5t`, `x5c` and `issuer`; Apple adds `alg`.
    let microsoft = provider("microsoft");
    let first = &microsoft.keys[0];
    assert!(first.other.contains_key("x5c"));
    assert!(first.other.contains_key("issuer"));
    let raw = json_fixture("microsoft-jwks.json");
    assert_eq!(serde_json::to_value(&microsoft).unwrap(), raw);

    let apple = provider("apple");
    assert!(apple.keys.iter().all(|k| k.alg.as_deref() == Some("RS256")));
    assert!(
        apple
            .keys
            .iter()
            .all(|k| k.algorithm() == Some(Algorithm::RS256))
    );
    assert_eq!(
        serde_json::to_value(&apple).unwrap(),
        json_fixture("apple-jwks.json")
    );
}

#[test]
fn unusable_entries_are_skipped_not_fatal() {
    // RFC 7517 §5: ignore keys that are malformed or of unknown types.
    let set: JwkSet = serde_json::from_value(json!({
        "keys": [
            {"kid": "no-kty"},
            "not an object",
            {"kty": "RSA", "kid": "ok", "n": "AQAB", "e": "AQAB"},
            {"kty": "future-type", "kid": "unknown"},
        ]
    }))
    .unwrap();
    let kids: Vec<_> = set.keys.iter().map(|k| k.kid.as_deref().unwrap()).collect();
    assert_eq!(kids, ["ok", "unknown"]);
    assert!(VerifyingKey::from_jwk(&set.keys[1]).is_err());
    // A set needs its `keys` array.
    assert!(serde_json::from_value::<JwkSet>(json!({"keys": "x"})).is_err());
    assert_eq!(
        serde_json::from_value::<JwkSet>(json!({})).unwrap(),
        JwkSet::default()
    );
}

#[test]
fn publishing_a_set_shows_only_public_keys() {
    let current = SigningKey::generate_ed25519().with_thumbprint_kid();
    let previous = SigningKey::generate_es256().with_kid("2025-01");
    let set: JwkSet = [&current, &previous]
        .into_iter()
        .filter_map(SigningKey::public_jwk)
        .collect();
    let published = serde_json::to_value(&set).unwrap();
    assert_eq!(published["keys"].as_array().unwrap().len(), 2);
    assert_eq!(published["keys"][0]["kid"], current.kid().unwrap());
    assert_eq!(published["keys"][1]["kid"], "2025-01");
    assert!(
        published["keys"]
            .as_array()
            .unwrap()
            .iter()
            .all(|k| k.get("d").is_none())
    );

    // A token signed by either key verifies against the published set
    // after a round trip through JSON, as a client would fetch it.
    let fetched: JwkSet = serde_json::from_value(published).unwrap();
    for key in [&current, &previous] {
        let token = key.sign_jwt(&json!({"sub": "ada"})).unwrap();
        let jws = Jws::parse(&token).unwrap();
        assert_eq!(jws.verify_with(&fetched).unwrap().kid.as_deref(), key.kid());
    }
}

#[test]
fn to_public_strips_every_private_member() {
    let v = json_fixture("rfc-vectors.json");
    let rsa: Jwk = serde_json::from_value(v["rfc7515_a2"]["jwk"].clone()).unwrap();
    assert!(rsa.is_private());
    for member in ["p", "q", "dp", "dq", "qi"] {
        assert!(rsa.other.contains_key(member), "{member}");
    }
    let public = rsa.to_public();
    let value = serde_json::to_value(&public).unwrap();
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["e", "kty", "n"])
    );
    let set = JwkSet::new([rsa.clone(), public.clone()]).to_public();
    assert!(set.keys.iter().all(|k| !k.is_private()));
    assert_eq!(rsa.thumbprint(), public.thumbprint());

    let oct: Jwk = serde_json::from_value(v["rfc7515_a1"]["jwk"].clone()).unwrap();
    assert!(oct.is_private());
    assert_eq!(oct.to_public().k, None);
}

#[test]
fn thumbprints_need_the_required_members() {
    let mut jwk = Jwk {
        kty: "EC".into(),
        crv: Some("P-256".into()),
        x: Some(URL_SAFE_NO_PAD.encode([1_u8; 32])),
        ..Jwk::default()
    };
    assert!(jwk.thumbprint().is_err(), "no y");
    jwk.y = Some(URL_SAFE_NO_PAD.encode([2_u8; 32]));
    let thumbprint = jwk.thumbprint().unwrap();
    // RFC 7638 §3.2: the members in lexicographic order, no whitespace.
    let canonical = format!(
        r#"{{"crv":"P-256","kty":"EC","x":"{}","y":"{}"}}"#,
        jwk.x.as_deref().unwrap(),
        jwk.y.as_deref().unwrap()
    );
    use sha2::Digest as _;
    assert_eq!(
        thumbprint,
        URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(canonical.as_bytes()))
    );
    assert!(
        Jwk {
            kty: "nope".into(),
            ..Jwk::default()
        }
        .thumbprint()
        .is_err()
    );
    let value: Value = serde_json::to_value(&jwk).unwrap();
    assert_eq!(value["kty"], "EC");
}
