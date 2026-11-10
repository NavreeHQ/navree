//! AWS Signature Version 4 against the requests of AWS's published SigV4
//! test suite (example.amazonaws.com, service `service` in us-east-1,
//! signed on 30 August 2015 at 12:36:00 UTC with the suite's example keys)
//! and the IAM `ListUsers` example of the AWS documentation.

use chrono::{DateTime, TimeZone, Utc};
use navree_crypto::sigv4::{self, Credentials, Signer};
use url::Url;

const ACCESS_KEY_ID: &str = "AKIDEXAMPLE";
const SECRET_ACCESS_KEY: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";

fn credentials() -> Credentials {
    Credentials::new(ACCESS_KEY_ID, SECRET_ACCESS_KEY)
}

fn signed_at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap()
}

/// A request of the test suite and what it is signed with.
struct Vector {
    name: &'static str,
    method: &'static str,
    url: &'static str,
    /// The headers other than `Host` and `X-Amz-Date`, as the suite sends
    /// them.
    headers: &'static [(&'static str, &'static str)],
    body: &'static [u8],
    signed_headers: &'static str,
    signature: &'static str,
}

/// The suite's requests on the path `/` (every service encodes a path twice,
/// so these are the ones signed the same way by every service).
const VECTORS: &[Vector] = &[
    Vector {
        name: "get-vanilla",
        method: "GET",
        url: "https://example.amazonaws.com/",
        headers: &[],
        body: b"",
        signed_headers: "host;x-amz-date",
        signature: "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31",
    },
    Vector {
        name: "get-vanilla-empty-query-key",
        method: "GET",
        url: "https://example.amazonaws.com/?Param1=value1",
        headers: &[],
        body: b"",
        signed_headers: "host;x-amz-date",
        signature: "a67d582fa61cc504c4bae71f336f98b97f1ea3c7a6bfe1b6e45aec72011b9aeb",
    },
    Vector {
        name: "get-vanilla-query-order-key",
        method: "GET",
        url: "https://example.amazonaws.com/?Param1=value2&Param1=Value1",
        headers: &[],
        body: b"",
        signed_headers: "host;x-amz-date",
        signature: "eedbc4e291e521cf13422ffca22be7d2eb8146eecf653089df300a15b2382bd1",
    },
    Vector {
        name: "get-vanilla-query-order-key-case",
        method: "GET",
        url: "https://example.amazonaws.com/?Param2=value2&Param1=value1",
        headers: &[],
        body: b"",
        signed_headers: "host;x-amz-date",
        signature: "b97d918cfa904a5beff61c982a1b6f458b799221646efd99d3219ec94cdf2500",
    },
    Vector {
        name: "get-vanilla-query-order-value",
        method: "GET",
        url: "https://example.amazonaws.com/?Param1=value2&Param1=value1",
        headers: &[],
        body: b"",
        signed_headers: "host;x-amz-date",
        signature: "5772eed61e12b33fae39ee5e7012498b51d56abc0abb7c60486157bd471c4694",
    },
    Vector {
        name: "get-vanilla-query-unreserved",
        method: "GET",
        url: "https://example.amazonaws.com/?-._~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz=-._~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
        headers: &[],
        body: b"",
        signed_headers: "host;x-amz-date",
        signature: "9c3e54bfcdf0b19771a7f523ee5669cdf59bc7cc0884027167c21bb143a40197",
    },
    Vector {
        name: "get-vanilla-utf8-query",
        method: "GET",
        url: "https://example.amazonaws.com/?\u{1234}=bar",
        headers: &[],
        body: b"",
        signed_headers: "host;x-amz-date",
        signature: "2cdec8eed098649ff3a119c94853b13c643bcf08f8b0a1d91e12c9027818dd04",
    },
    Vector {
        name: "get-header-key-duplicate",
        method: "GET",
        url: "https://example.amazonaws.com/",
        headers: &[
            ("My-Header1", "value2"),
            ("My-Header1", "value2"),
            ("My-Header1", "value1"),
        ],
        body: b"",
        signed_headers: "host;my-header1;x-amz-date",
        signature: "c9d5ea9f3f72853aea855b47ea873832890dbdd183b4468f858259531a5138ea",
    },
    Vector {
        name: "get-header-value-order",
        method: "GET",
        url: "https://example.amazonaws.com/",
        headers: &[
            ("My-Header1", "value4"),
            ("My-Header1", "value1"),
            ("My-Header1", "value3"),
            ("My-Header1", "value2"),
        ],
        body: b"",
        signed_headers: "host;my-header1;x-amz-date",
        signature: "08c7e5a9acfcfeb3ab6b2185e75ce8b1deb5e634ec47601a50643f830c755c01",
    },
    Vector {
        name: "get-header-value-trim",
        method: "GET",
        url: "https://example.amazonaws.com/",
        headers: &[("My-Header1", " value1"), ("My-Header2", " \"a   b   c\"")],
        body: b"",
        signed_headers: "host;my-header1;my-header2;x-amz-date",
        signature: "acc3ed3afb60bb290fc8d2dd0098b9911fcaa05412b367055dee359757a9c736",
    },
    Vector {
        name: "post-vanilla",
        method: "POST",
        url: "https://example.amazonaws.com/",
        headers: &[],
        body: b"",
        signed_headers: "host;x-amz-date",
        signature: "5da7c1a2acd57cee7505fc6676e4e544621c30862966e37dddb68e92efbe5d6b",
    },
    Vector {
        name: "post-vanilla-query",
        method: "POST",
        url: "https://example.amazonaws.com/?Param1=value1",
        headers: &[],
        body: b"",
        signed_headers: "host;x-amz-date",
        signature: "28038455d6de14eafc1f9222cf5aa6f1a96197d7deb8263271d420d138af7f11",
    },
    Vector {
        name: "post-header-key-sort",
        method: "POST",
        url: "https://example.amazonaws.com/",
        headers: &[("My-Header1", "value1")],
        body: b"",
        signed_headers: "host;my-header1;x-amz-date",
        signature: "c5410059b04c1ee005303aed430f6e6645f61f4dc9e1461ec8f8916fdf18852c",
    },
    Vector {
        name: "post-header-value-case",
        method: "POST",
        url: "https://example.amazonaws.com/",
        headers: &[("My-Header1", "VALUE1")],
        body: b"",
        signed_headers: "host;my-header1;x-amz-date",
        signature: "cdbc9802e29d2942e5e10b5bccfdd67c5f22c7c4e8ae67b53629efa58b974b7d",
    },
    Vector {
        name: "post-x-www-form-urlencoded",
        method: "POST",
        url: "https://example.amazonaws.com/",
        headers: &[("Content-Type", "application/x-www-form-urlencoded")],
        body: b"Param1=value1",
        signed_headers: "content-type;host;x-amz-date",
        signature: "ff11897932ad3f4e8b18135d722051e5ac45fc38421b1da7b9d196a0fe09473a",
    },
    Vector {
        name: "post-x-www-form-urlencoded-parameters",
        method: "POST",
        url: "https://example.amazonaws.com/",
        headers: &[(
            "Content-Type",
            "application/x-www-form-urlencoded; charset=utf8",
        )],
        body: b"Param1=value1",
        signed_headers: "content-type;host;x-amz-date",
        signature: "1a72ec8f64bd914b0e42e42607c7fbce7fb2c7465f63e3092b3b0d39fa77a6fe",
    },
];

/// The suite's requests on other paths, whose canonical path is the path
/// encoded once: what S3 signs (the path as sent).
const PATH_VECTORS: &[Vector] = &[
    Vector {
        name: "get-utf8",
        method: "GET",
        url: "https://example.amazonaws.com/\u{1234}",
        headers: &[],
        body: b"",
        signed_headers: "host;x-amz-date",
        signature: "8318018e0b0f223aa2bbf98705b62bb787dc9c0e678f255a891fd03141be5d85",
    },
    Vector {
        name: "get-space",
        method: "GET",
        url: "https://example.amazonaws.com/example space/",
        headers: &[],
        body: b"",
        signed_headers: "host;x-amz-date",
        signature: "652487583200325589f1fba4c7e578f72c47cb61beeca81406b39ddec1366741",
    },
];

fn authorization(headers: &[(String, String)]) -> &str {
    &headers
        .iter()
        .find(|(name, _)| name == "authorization")
        .expect("an authorization header")
        .1
}

fn check(signer: &Signer, vector: &Vector) {
    let url = Url::parse(vector.url).unwrap();
    let headers = signer.sign(
        vector.method,
        &url,
        vector.headers,
        vector.body,
        signed_at(),
    );
    assert_eq!(
        authorization(&headers),
        format!(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders={}, Signature={}",
            vector.signed_headers, vector.signature
        ),
        "{}",
        vector.name
    );
    assert_eq!(
        headers[0],
        ("x-amz-date".to_owned(), "20150830T123600Z".to_owned()),
        "{}",
        vector.name
    );
}

#[test]
fn the_test_suite_requests_are_signed_as_aws_signs_them() {
    let signer = Signer::new(credentials(), "us-east-1", "service");
    for vector in VECTORS {
        check(&signer, vector);
    }
}

#[test]
fn the_test_suite_paths_are_signed_as_sent_for_s3() {
    let signer = Signer::new(credentials(), "us-east-1", "service").path_as_sent();
    for vector in PATH_VECTORS.iter().chain(VECTORS) {
        check(&signer, vector);
    }
}

#[test]
fn other_services_encode_the_path_again() {
    // The same request signed by a service other than S3 is another
    // signature: its canonical path is `/example%2520space/`.
    let signer = Signer::new(credentials(), "us-east-1", "service");
    let url = Url::parse("https://example.amazonaws.com/example space/").unwrap();
    let headers = signer.sign("GET", &url, &[], b"", signed_at());
    assert!(!authorization(&headers).ends_with(PATH_VECTORS[1].signature));
    assert_eq!(
        sigv4::canonical_path(url.path(), true),
        "/example%2520space/"
    );
}

#[test]
fn the_parts_match_the_suites_get_vanilla() {
    let canonical = sigv4::canonical_request(
        "GET",
        &sigv4::canonical_path("/", true),
        &sigv4::canonical_query(Vec::<(&str, &str)>::new()),
        &[
            ("Host", "example.amazonaws.com"),
            ("X-Amz-Date", "20150830T123600Z"),
        ],
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
    assert_eq!(
        canonical.text,
        "GET\n/\n\nhost:example.amazonaws.com\nx-amz-date:20150830T123600Z\n\nhost;x-amz-date\n\
         e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(canonical.signed_headers, "host;x-amz-date");
    assert_eq!(
        sigv4::string_to_sign(
            "20150830T123600Z",
            &sigv4::scope("20150830", "us-east-1", "service"),
            &canonical.text
        ),
        "AWS4-HMAC-SHA256\n20150830T123600Z\n20150830/us-east-1/service/aws4_request\n\
         bb579772317eb040ac9ed261061d46c1f17a8133879d6129b6e1c25292927e63"
    );
    assert_eq!(
        sigv4::signature(
            SECRET_ACCESS_KEY,
            "20150830T123600Z",
            "us-east-1",
            "service",
            &canonical.text
        ),
        "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
    );
}

/// The IAM `ListUsers` example of the AWS documentation: its canonical
/// request's hash, derived signing key and signature.
#[test]
fn the_iam_documentation_example_is_signed_as_documented() {
    let url = Url::parse("https://iam.amazonaws.com/?Action=ListUsers&Version=2010-05-08").unwrap();
    let content_type = (
        "Content-Type",
        "application/x-www-form-urlencoded; charset=utf-8",
    );
    let canonical = sigv4::canonical_request(
        "GET",
        &sigv4::canonical_path(url.path(), true),
        &sigv4::canonical_query(url.query_pairs()),
        &[
            content_type,
            ("Host", "iam.amazonaws.com"),
            ("X-Amz-Date", "20150830T123600Z"),
        ],
        &navree_crypto::sha256_hex(b""),
    );
    assert_eq!(
        navree_crypto::sha256_hex(canonical.text.as_bytes()),
        "f536975d06c0309214f805bb90ccff089219ecd68b2577efef23edd43b7e1a59"
    );
    assert_eq!(
        hex::encode(sigv4::signing_key(
            SECRET_ACCESS_KEY,
            "20150830",
            "us-east-1",
            "iam"
        )),
        "c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9"
    );
    let headers = Signer::new(credentials(), "us-east-1", "iam").sign(
        "GET",
        &url,
        &[content_type],
        b"",
        signed_at(),
    );
    assert_eq!(
        authorization(&headers),
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
         SignedHeaders=content-type;host;x-amz-date, \
         Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
    );
}

#[test]
fn temporary_credentials_sign_and_send_the_session_token() {
    let signer = Signer::new(
        credentials().with_session_token(Some("session".to_owned())),
        "eu-west-1",
        "ses",
    );
    let url = Url::parse("https://email.eu-west-1.amazonaws.com/v2/email/outbound-emails").unwrap();
    let headers = signer.sign("POST", &url, &[], b"{}", signed_at());
    assert!(
        authorization(&headers).contains("SignedHeaders=host;x-amz-date;x-amz-security-token"),
        "{}",
        authorization(&headers)
    );
    assert!(headers.contains(&("x-amz-security-token".to_owned(), "session".to_owned())));
    // The token is part of what is signed: another token, another signature.
    let other = Signer::new(
        credentials().with_session_token(Some("other".to_owned())),
        "eu-west-1",
        "ses",
    )
    .sign("POST", &url, &[], b"{}", signed_at());
    assert_ne!(authorization(&headers), authorization(&other));
}

#[test]
fn everything_signed_changes_the_signature() {
    let signer = Signer::new(credentials(), "us-east-1", "service");
    let url = Url::parse("https://example.amazonaws.com/?a=1").unwrap();
    let base = signer.sign(
        "POST",
        &url,
        &[("content-type", "text/plain")],
        b"body",
        signed_at(),
    );
    let variants = [
        signer.sign(
            "PUT",
            &url,
            &[("content-type", "text/plain")],
            b"body",
            signed_at(),
        ),
        signer.sign(
            "POST",
            &Url::parse("https://example.amazonaws.com/?a=2").unwrap(),
            &[("content-type", "text/plain")],
            b"body",
            signed_at(),
        ),
        signer.sign(
            "POST",
            &Url::parse("https://other.amazonaws.com/?a=1").unwrap(),
            &[("content-type", "text/plain")],
            b"body",
            signed_at(),
        ),
        signer.sign(
            "POST",
            &url,
            &[("content-type", "text/html")],
            b"body",
            signed_at(),
        ),
        signer.sign(
            "POST",
            &url,
            &[("content-type", "text/plain")],
            b"bodY",
            signed_at(),
        ),
        signer.sign(
            "POST",
            &url,
            &[("content-type", "text/plain")],
            b"body",
            signed_at() + chrono::TimeDelta::seconds(1),
        ),
        Signer::new(credentials(), "us-west-2", "service").sign(
            "POST",
            &url,
            &[("content-type", "text/plain")],
            b"body",
            signed_at(),
        ),
        Signer::new(
            Credentials::new(ACCESS_KEY_ID, "another secret"),
            "us-east-1",
            "service",
        )
        .sign(
            "POST",
            &url,
            &[("content-type", "text/plain")],
            b"body",
            signed_at(),
        ),
    ];
    let signature = |headers: &[(String, String)]| {
        authorization(headers)
            .rsplit_once("Signature=")
            .unwrap()
            .1
            .to_owned()
    };
    for (i, variant) in variants.iter().enumerate() {
        assert_ne!(signature(&base), signature(variant), "variant {i}");
    }
}
