//! DKIM signing: RSA and Ed25519 signatures verified against the published
//! key, and SMTP signing.

use std::any::Any;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_contracts::Secret;
use navree_crypto::jose::{Algorithm, SigningKey, VerifyingKey};
use navree_mail::{DkimAlgorithm, DkimConfig, DkimSigner, Message, SmtpTransport, TransportConfig};
use serde_json::json;
use sha2::{Digest as _, Sha256};

const RSA_KEY: &str = include_str!("../fixtures/dkim-rsa-2048.pem");

const MESSAGE: &[u8] = b"From: Agra <hello@acme.test>\r\n\
    To: anu@example.com\r\n\
    Subject:   Your   invoice\r\n\
    X-Ignored: not signed\r\n\
    \r\n\
    Thanks for   your order.\r\n\
    \r\n\
    \r\n";

/// The `DKIM-Signature` value without its CRLF and folding, and the
/// decoded signature.
fn parse(header: &str) -> (String, Vec<u8>) {
    let value = header
        .strip_prefix("DKIM-Signature: ")
        .expect("header name")
        .trim_end_matches("\r\n")
        .replace("\r\n\t", " ");
    let (unsigned, signature) = value.rsplit_once("b=").expect("b=");
    (
        format!("{unsigned}b="),
        STANDARD.decode(signature).expect("base64"),
    )
}

/// The canonical input of [`MESSAGE`] for `h=from:subject:to`.
fn signing_input(unsigned: &str) -> Vec<u8> {
    format!(
        "from:Agra <hello@acme.test>\r\nsubject:Your invoice\r\nto:anu@example.com\r\ndkim-signature:{unsigned}"
    )
    .into_bytes()
}

/// The public key published in the signer's DNS record.
fn published_key(signer: &DkimSigner) -> Vec<u8> {
    let record = signer.dns_record();
    let p = record.split("p=").nth(1).expect("p=");
    STANDARD.decode(p).expect("base64")
}

#[test]
fn rsa_signatures_verify_with_the_published_key() {
    let signer = DkimSigner::from_pem("acme.test", "mail", RSA_KEY).expect("signer");
    assert_eq!(signer.algorithm(), DkimAlgorithm::RsaSha256);
    assert_eq!(signer.record_name(), "mail._domainkey.acme.test");
    assert!(
        signer
            .dns_record()
            .starts_with("v=DKIM1; k=rsa; p=MIIBIjAN")
    );

    let header = signer.sign_at(MESSAGE, 1_700_000_000).expect("sign");
    let (unsigned, signature) = parse(&header);

    assert_eq!(
        unsigned,
        format!(
            "v=1; a=rsa-sha256; c=relaxed/relaxed; d=acme.test; s=mail; t=1700000000; \
             h=from:subject:to; bh={}; b=",
            STANDARD.encode(Sha256::digest(b"Thanks for your order.\r\n"))
        )
    );
    let key = VerifyingKey::from_spki_der(&published_key(&signer)).expect("spki");
    key.verify(Algorithm::RS256, &signing_input(&unsigned), &signature)
        .expect("the signature verifies");
}

#[test]
fn ed25519_signatures_verify_with_the_published_key() {
    let pem = SigningKey::generate_ed25519().to_pkcs8_pem().expect("pem");
    let signer = DkimSigner::from_pem("acme.test", "ed", &pem).expect("signer");
    assert_eq!(signer.algorithm(), DkimAlgorithm::Ed25519Sha256);

    let header = signer.sign_at(MESSAGE, 1_700_000_000).expect("sign");
    let (unsigned, signature) = parse(&header);

    assert!(unsigned.starts_with("v=1; a=ed25519-sha256;"));
    let key = VerifyingKey::ed25519(&published_key(&signer)).expect("key");
    key.verify(
        Algorithm::EdDSA,
        &Sha256::digest(signing_input(&unsigned)),
        &signature,
    )
    .expect("the signature verifies");
}

#[test]
fn only_the_signing_domain_and_its_subdomains_are_signed() {
    let signer = DkimSigner::from_pem("Acme.test.", "mail", RSA_KEY).expect("signer");
    assert_eq!(signer.domain(), "acme.test");
    assert!(signer.applies_to("acme.test"));
    assert!(signer.applies_to("news.ACME.test"));
    assert!(!signer.applies_to("notacme.test"));
    assert!(!signer.applies_to("acme.test.evil"));
}

#[test]
fn invalid_settings_are_config_errors() {
    for (domain, selector, key) in [
        ("", "mail", RSA_KEY),
        ("acme.test", "", RSA_KEY),
        ("acme.test", "mail", "not a key"),
    ] {
        let error = DkimSigner::from_pem(domain, selector, key).unwrap_err();
        assert!(
            error.to_string().contains("invalid DKIM settings"),
            "{error}"
        );
    }
    let p256 = SigningKey::generate_es256().to_pkcs8_pem().expect("pem");
    let error = DkimSigner::from_pem("acme.test", "mail", &p256).unwrap_err();
    assert!(error.to_string().contains("RSA or Ed25519"), "{error}");
}

#[test]
fn configured_headers_always_include_from() {
    let signer = DkimSigner::new(&DkimConfig {
        domain: "acme.test".to_owned(),
        selector: "mail".to_owned(),
        private_key: Secret::new(RSA_KEY.to_owned()),
        headers: vec!["Subject".to_owned()],
    })
    .expect("signer");
    let header = signer.sign_at(MESSAGE, 1).expect("sign");
    assert!(header.contains("h=from:subject;"), "{header}");
}

#[test]
fn smtp_signs_mail_from_its_domain() {
    let pem = SigningKey::generate_ed25519().to_pkcs8_pem().expect("pem");
    let transport = SmtpTransport::from_url("smtp://localhost:1025")
        .expect("smtp")
        .with_dkim(DkimSigner::from_pem("acme.test", "mail", &pem).expect("signer"));
    let message = |from: &str| {
        Message::new()
            .from(from)
            .to("anu@example.com")
            .subject("Hi")
            .text("Hello")
    };

    let signed = transport
        .signed_bytes(&message("Agra <hello@acme.test>"))
        .expect("sign")
        .expect("signed");
    let text = String::from_utf8(signed).expect("text");
    assert!(
        text.starts_with("DKIM-Signature: v=1;\r\n\ta=ed25519-sha256;"),
        "{text}"
    );
    assert!(
        text.contains("\r\nFrom: Agra <hello@acme.test>\r\n"),
        "{text}"
    );

    assert!(
        transport
            .signed_bytes(&message("someone@other.test"))
            .expect("sign")
            .is_none()
    );
}

#[test]
fn the_smtp_table_takes_dkim_settings() {
    let pem = SigningKey::generate_ed25519().to_pkcs8_pem().expect("pem");
    let config: TransportConfig = serde_json::from_value(json!({
        "type": "smtp",
        "url": "smtps://u:p@smtp.acme.test",
        "dkim": { "domain": "acme.test", "selector": "mail", "private_key": pem },
    }))
    .expect("config");

    let built = config.build().expect("build");
    let smtp = (built.as_ref() as &dyn Any)
        .downcast_ref::<SmtpTransport>()
        .expect("smtp");
    assert_eq!(smtp.dkim().expect("dkim").selector(), "mail");
    assert!(!format!("{config:?}").contains("PRIVATE KEY"));
}
