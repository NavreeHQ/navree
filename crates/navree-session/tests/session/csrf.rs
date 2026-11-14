//! CSRF protection: the `XSRF-TOKEN` cookie endpoint and the 403 check on
//! cookie-authenticated writes.

use navree_testing::cookie::SameSite;
use navree_testing::cookie::time::Duration;
use serde_json::json;

use crate::support::{self, COOKIE, XSRF};

#[tokio::test]
async fn csrf_cookie_endpoint_sets_xsrf_token() {
    let app = support::app().await;
    let client = app.client();

    let response = client.get("/csrf-cookie").send().await.assert_status(204);
    let xsrf = response
        .set_cookie(XSRF)
        .expect("the XSRF-TOKEN cookie is set");
    assert_ne!(xsrf.http_only(), Some(true), "JavaScript must read it");
    assert_eq!(xsrf.path(), Some("/"));
    assert_eq!(xsrf.same_site(), Some(SameSite::Lax));
    assert_eq!(xsrf.max_age(), Some(Duration::minutes(120)));
    assert_eq!(xsrf.value().len(), 40);
    assert!(
        response.set_cookie(COOKIE).is_some(),
        "the token lives in a new session"
    );

    let body = client
        .get("/api/probe/token")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body["token"],
        json!(xsrf.value()),
        "the cookie carries the session's token"
    );

    // Asking again keeps the token and hands the cookie out again.
    let again = client.get("/csrf-cookie").send().await.assert_status(204);
    assert_eq!(again.set_cookie(XSRF).unwrap().value(), xsrf.value());
}

#[tokio::test]
async fn post_with_session_without_xsrf_is_403() {
    let app = support::app().await;
    let client = app.client();
    client.csrf_cookie().await;

    let response = client
        .post("/api/probe/write")
        .json(&json!({}))
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    assert_eq!(
        response.json_value(),
        json!({
            "statusCode": 403,
            "message": "Invalid CSRF token",
            "error": "Forbidden",
            "code": "csrf_token_mismatch",
        })
    );
    assert!(response.header("x-request-id").is_some());
}

#[tokio::test]
async fn post_with_wrong_xsrf_is_403() {
    let app = support::app().await;
    let client = app.client();
    let token = client.csrf_cookie().await.set_cookie(XSRF).unwrap();

    let mut wrong = token.value().to_owned();
    let last = wrong.pop().unwrap();
    wrong.push(if last == 'a' { 'b' } else { 'a' });
    for (header, value) in [
        ("x-xsrf-token", wrong.as_str()),
        ("x-csrf-token", wrong.as_str()),
        ("x-xsrf-token", ""),
        ("x-csrf-token", &token.value()[..39]),
    ] {
        client
            .post("/api/probe/write")
            .header(header, value)
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
    }

    // Another session's token does not fit this session.
    let other = app.client();
    let other_token = other.csrf_cookie().await.set_cookie(XSRF).unwrap();
    client
        .post("/api/probe/write")
        .header("x-xsrf-token", other_token.value())
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
}

#[tokio::test]
async fn post_with_matching_xsrf_passes() {
    let app = support::app().await;
    let client = app.client();
    let token = client.csrf_cookie().await.set_cookie(XSRF).unwrap();

    client
        .post("/api/probe/write")
        .xsrf()
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/write")
        .header("x-csrf-token", token.value())
        .send()
        .await
        .assert_status(200);
    for method in ["PUT", "PATCH", "DELETE"] {
        let method = method.parse().unwrap();
        client
            .request(method, "/api/probe/write")
            .xsrf()
            .send()
            .await
            .assert_status(200);
    }
}

#[tokio::test]
async fn xsrf_header_is_url_decoded() {
    let app = support::app().await;
    let client = app.client();
    let token = client.csrf_cookie().await.set_cookie(XSRF).unwrap();
    let encoded: String = token.value().bytes().map(|b| format!("%{b:02X}")).collect();
    client
        .post("/api/probe/write")
        .header("x-xsrf-token", encoded.as_str())
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn every_unsafe_method_is_checked() {
    let app = support::app().await;
    let client = app.client();
    client.csrf_cookie().await;
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        client
            .request(method.parse().unwrap(), "/api/probe/write")
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
    }
}

#[tokio::test]
async fn bearer_request_skips_csrf() {
    let app = support::app().await;
    let client = app.client();
    client.csrf_cookie().await;
    client
        .post("/api/probe/write")
        .bearer("1|plain-text-token")
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/write")
        .header("authorization", "Basic dXNlcjpwYXNz")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    client
        .post("/api/probe/write")
        .header("authorization", "Bearer ")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
}

#[tokio::test]
async fn get_is_never_csrf_checked() {
    let app = support::app().await;
    let client = app.client();
    client.csrf_cookie().await;
    client
        .get("/api/probe/token")
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/probe/token")
        .header("x-xsrf-token", "wrong")
        .send()
        .await
        .assert_status(200);
    client
        .request("HEAD".parse().unwrap(), "/api/probe/token")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn post_without_session_cookie_is_not_csrf_checked() {
    let app = support::app().await;
    let client = app.client();
    client
        .post("/api/probe/write")
        .send()
        .await
        .assert_status(200);
    // Other cookies do not count.
    client.set_cookie("theme", "dark");
    client
        .post("/api/probe/write")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn a_stale_session_cookie_still_needs_the_token() {
    let app = support::app().await;
    let client = app.client();
    client.set_cookie(COOKIE, "v1.not-a-valid.cookie.value");
    client
        .post("/api/probe/write")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    // The handshake replaces the stale cookie, and the write then passes.
    client.csrf_cookie().await;
    client
        .post("/api/probe/write")
        .xsrf()
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn csrf_except_path_skips() {
    let app = support::app_with(navree_session::SessionOptions {
        csrf_except: vec!["/webhooks/*".into(), "api/probe/put".into()],
        ..navree_session::SessionOptions::default()
    })
    .await;
    let client = app.client();
    client.csrf_cookie().await;

    client
        .post("/webhooks/incoming")
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/put")
        .json(&json!({ "key": "k", "value": 1 }))
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/write")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
}

#[tokio::test]
async fn a_failed_check_does_not_save_the_session() {
    let app = support::app().await;
    let client = app.client();
    client.csrf_cookie().await;
    let response = client
        .post("/api/probe/put")
        .json(&json!({ "key": "k", "value": 1 }))
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    assert!(response.set_cookie(COOKIE).is_none());
    let (_, value) = support::get(&client, "k").await;
    assert_eq!(value, json!(null), "the handler did not run");
}

#[tokio::test]
async fn cross_site_browser_writes_without_a_session_are_rejected() {
    // Login CSRF: a cross-site form must not sign the victim in, even
    // though the victim's browser has no session cookie yet.
    let app = support::base()
        .config(
            "http.cors.allowed_origins",
            json!(["http://localhost:3000"]),
        )
        .module(navree_session::SessionModule::for_root(
            navree_session::SessionOptions {
                csrf_except: vec!["/webhooks/*".into()],
                ..navree_session::SessionOptions::default()
            },
        ))
        .build()
        .await;
    let client = app.client();
    let response = client
        .post("/api/probe/write")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://evil.example")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    assert!(response.set_cookie(COOKIE).is_none());

    // An allowed front-end origin, same-site and same-origin requests,
    // non-browser clients and exempt paths pass.
    client
        .post("/api/probe/write")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "http://localhost:3000")
        .send()
        .await
        .assert_status(200);
    for site in ["same-site", "same-origin", "none"] {
        client
            .post("/api/probe/write")
            .header("sec-fetch-site", site)
            .header("origin", "https://evil.example")
            .send()
            .await
            .assert_status(200);
    }
    client
        .post("/api/probe/write")
        .header("origin", "https://evil.example")
        .send()
        .await
        .assert_status(200);
    client
        .post("/webhooks/incoming")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://payments.example")
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/write")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://evil.example")
        .bearer("1|token")
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/probe/untouched")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://evil.example")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn csrf_except_globs_know_only_star_and_question_mark() {
    // Which of the two probe writes (`put`, `write`) a list of patterns
    // exempts: 200 when exempt, 403 when the CSRF check applies.
    async fn exempt(patterns: &[&str]) -> (u16, u16) {
        let app = support::app_with(navree_session::SessionOptions {
            csrf_except: patterns.iter().map(|p| (*p).to_owned()).collect(),
            ..navree_session::SessionOptions::default()
        })
        .await;
        let client = app.client();
        client.csrf_cookie().await;
        let put = client
            .post("/api/probe/put")
            .json(&json!({ "key": "k", "value": 1 }))
            .send()
            .await
            .status();
        let write = client.post("/api/probe/write").send().await.status();
        (put.as_u16(), write.as_u16())
    }

    // `?` is one character; `*` crosses `/` and may match nothing.
    assert_eq!(exempt(&["/api/probe/p?t"]).await, (200, 403));
    assert_eq!(exempt(&["/api/*/wr?te"]).await, (403, 200));
    assert_eq!(exempt(&["/api/*"]).await, (200, 200));
    assert_eq!(exempt(&["/api*probe/put*"]).await, (200, 403));
    // The whole path has to match.
    assert_eq!(exempt(&["/api/probe/pu"]).await, (403, 403));
    assert_eq!(exempt(&["/api/probe/p??t"]).await, (403, 403));
    // A pattern without a leading `/` gets one; matching is case-sensitive
    // and brackets are literal.
    assert_eq!(exempt(&["api/probe/put"]).await, (200, 403));
    assert_eq!(exempt(&["/API/probe/put"]).await, (403, 403));
    assert_eq!(exempt(&["/api/probe/[pw]ut"]).await, (403, 403));
}

/// A plain HTML form has no script to put the token in a header, so a
/// urlencoded body may carry it as `_csrf`, once.
mod plain_forms {
    use navree_testing::TestClient;

    use crate::support::{self, XSRF};

    pub(super) async fn signed_in() -> (navree_testing::TestApp, TestClient, String) {
        let app = support::app().await;
        let client = app.client();
        let token = client
            .csrf_cookie()
            .await
            .set_cookie(XSRF)
            .unwrap()
            .value()
            .to_owned();
        (app, client, token)
    }

    fn form(client: &TestClient, body: String) -> navree_testing::TestRequest {
        client
            .post("/api/probe/echo")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
    }

    #[tokio::test]
    async fn the_token_field_passes_and_the_handler_never_sees_it() {
        let (_app, client, token) = signed_in().await;
        let response = form(&client, format!("title=Agra&_csrf={token}&amount=12"))
            .send()
            .await
            .assert_status(200);
        assert_eq!(response.text(), "title=Agra&amount=12");
        let response = form(&client, format!("_csrf={token}&title=Agra"))
            .send()
            .await
            .assert_status(200);
        assert_eq!(response.text(), "title=Agra");
        let response = form(&client, format!("_csrf={token}"))
            .send()
            .await
            .assert_status(200);
        assert_eq!(response.text(), "");
    }

    #[tokio::test]
    async fn a_wrong_or_missing_token_field_is_403() {
        let (_app, client, token) = signed_in().await;
        let mut wrong = token.clone();
        let last = wrong.pop().unwrap();
        wrong.push(if last == 'a' { 'b' } else { 'a' });
        for body in [
            "title=Agra".to_owned(),
            format!("_csrf={wrong}"),
            "_csrf=".to_owned(),
            format!("_csrf={}", &token[..39]),
            format!("csrf={token}"),
            format!("_CSRF={token}"),
            format!("x_csrf={token}"),
            format!("_csrf%5B%5D={token}"),
        ] {
            form(&client, body.clone())
                .send()
                .await
                .assert_error(403, "Invalid CSRF token");
        }
    }

    #[tokio::test]
    async fn two_token_fields_are_refused_whichever_is_right() {
        let (_app, client, token) = signed_in().await;
        for body in [
            format!("_csrf=x&_csrf={token}"),
            format!("_csrf={token}&_csrf=x"),
            format!("_csrf={token}&_csrf={token}"),
        ] {
            form(&client, body)
                .send()
                .await
                .assert_error(403, "Invalid CSRF token");
        }
    }

    #[tokio::test]
    async fn a_token_in_the_url_or_another_body_type_does_not_count() {
        let (_app, client, token) = signed_in().await;
        client
            .post(&format!("/api/probe/echo?_csrf={token}"))
            .header("content-type", "application/x-www-form-urlencoded")
            .body("title=Agra")
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
        for kind in [
            "application/json",
            "text/plain",
            "multipart/form-data; boundary=x",
        ] {
            client
                .post("/api/probe/echo")
                .header("content-type", kind)
                .body(format!("_csrf={token}"))
                .send()
                .await
                .assert_error(403, "Invalid CSRF token");
        }
        client
            .post("/api/probe/echo")
            .body(format!("_csrf={token}"))
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
    }

    #[tokio::test]
    async fn a_header_that_is_wrong_is_not_rescued_by_the_field() {
        let (_app, client, token) = signed_in().await;
        form(&client, format!("_csrf={token}"))
            .header("x-csrf-token", "wrong")
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
        // The header alone still passes, and leaves the body as it is.
        let response = form(&client, "_csrf=whatever&title=Agra".to_owned())
            .header("x-csrf-token", &token)
            .send()
            .await
            .assert_status(200);
        assert_eq!(response.text(), "_csrf=whatever&title=Agra");
    }

    #[tokio::test]
    async fn a_body_past_the_cap_must_send_the_header() {
        let (_app, client, token) = signed_in().await;
        let big = "x".repeat(300 * 1024);
        form(&client, format!("_csrf={token}&note={big}"))
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
        form(&client, format!("note={big}"))
            .header("x-csrf-token", &token)
            .send()
            .await
            .assert_status(200);
    }

    #[tokio::test]
    async fn an_encoded_token_passes_and_a_session_less_form_is_untouched() {
        let (_app, client, token) = signed_in().await;
        let encoded: String = token.bytes().map(|b| format!("%{b:02X}")).collect();
        form(&client, format!("_csrf={encoded}&a=1"))
            .send()
            .await
            .assert_status(200);
        // No session cookie: nothing to check, and nothing removed.
        let app = support::app().await;
        let fresh = app.client();
        let response = form(&fresh, "_csrf=whatever&a=1".to_owned())
            .send()
            .await
            .assert_status(200);
        assert_eq!(response.text(), "_csrf=whatever&a=1");
    }
}

/// A form with a file field posts `multipart/form-data`; with no script, its
/// first part may carry the token as `_csrf`.
mod multipart_forms {
    use navree_testing::TestClient;

    use super::plain_forms::signed_in;
    use crate::support::{self, XSRF};

    const BOUNDARY: &str = "----NavreeForm7MA4YWxkTrZu0gW";

    /// A field part.
    fn field(name: &str, value: &str) -> String {
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        )
    }

    /// A file part.
    fn file(name: &str, file_name: &str, value: &str) -> String {
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\nContent-Type: text/plain\r\n\r\n{value}\r\n"
        )
    }

    /// The parts, then the close.
    fn body(parts: &[String]) -> String {
        format!("{}--{BOUNDARY}--\r\n", parts.concat())
    }

    /// A part with `head` for its header lines.
    fn with_head(head: &str, value: &str) -> String {
        format!("--{BOUNDARY}\r\n{head}\r\n\r\n{value}\r\n--{BOUNDARY}--\r\n")
    }

    fn post(client: &TestClient, body: String) -> navree_testing::TestRequest {
        client
            .post("/api/probe/echo")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(body)
    }

    #[tokio::test]
    async fn the_token_part_first_passes_and_the_body_reaches_the_handler_as_it_came() {
        let (_app, client, token) = signed_in().await;
        let sent = body(&[
            field("_csrf", &token),
            field("title", "Rent agreement, Ludhiana"),
            file("scan", "agreement.txt", "Rent: ₹25,000 a month"),
        ]);
        let response = post(&client, sent.clone()).send().await.assert_status(200);
        assert_eq!(
            response.text(),
            sent,
            "the handler reads the form as it was sent"
        );
        // The form's only part, and the field's name unquoted.
        post(&client, body(&[field("_csrf", &token)]))
            .send()
            .await
            .assert_status(200);
        let unquoted = with_head("Content-Disposition: form-data; name=_csrf", &token);
        post(&client, unquoted).send().await.assert_status(200);
    }

    #[tokio::test]
    async fn a_large_file_after_the_token_passes_whole() {
        let (_app, client, token) = signed_in().await;
        let large = "x".repeat(1024 * 1024);
        let sent = body(&[field("_csrf", &token), file("scan", "scan.txt", &large)]);
        let response = post(&client, sent.clone()).send().await.assert_status(200);
        assert_eq!(response.text().len(), sent.len());
        assert_eq!(response.text(), sent);
    }

    #[tokio::test]
    async fn a_wrong_or_missing_token_part_is_403() {
        let (_app, client, token) = signed_in().await;
        let mut wrong = token.clone();
        let last = wrong.pop().unwrap();
        wrong.push(if last == 'a' { 'b' } else { 'a' });
        let other_app = support::app().await;
        let other = other_app.client();
        let other_token = other
            .csrf_cookie()
            .await
            .set_cookie(XSRF)
            .unwrap()
            .value()
            .to_owned();
        let scan = file("scan", "scan.txt", "x");
        for sent in [
            // A token that is not the session's.
            body(&[field("_csrf", &wrong), scan.clone()]),
            body(&[field("_csrf", ""), scan.clone()]),
            body(&[field("_csrf", &token[..39]), scan.clone()]),
            body(&[field("_csrf", &format!("{token}x")), scan.clone()]),
            body(&[field("_csrf", &other_token), scan.clone()]),
            // No token field first.
            body(&[field("title", "Agra"), scan.clone()]),
            body(&[field("title", "Agra"), field("_csrf", &token), scan.clone()]),
            body(&[scan.clone(), field("_csrf", &token)]),
            body(&[file("_csrf", "token.txt", &token)]),
            body(&[field("_CSRF", &token), scan.clone()]),
            body(&[field("csrf", &token), scan.clone()]),
            body(&[field("_csrf[]", &token), scan.clone()]),
            body(&[]),
            // A body that does not open with the delimiter, or names another.
            format!("preamble\r\n{}", body(&[field("_csrf", &token)])),
            body(&[field("_csrf", &token)]).replace(BOUNDARY, "other-boundary"),
            format!("_csrf={token}"),
            String::new(),
        ] {
            post(&client, sent)
                .send()
                .await
                .assert_error(403, "Invalid CSRF token");
        }
    }

    #[tokio::test]
    async fn a_token_that_is_not_utf8_or_a_session_without_one_is_403() {
        let (_app, client, token) = signed_in().await;
        let mut sent = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"_csrf\"\r\n\r\n{token}"
        )
        .into_bytes();
        sent.push(0xff);
        sent.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
        client
            .post("/api/probe/echo")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(sent)
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
        // A session cookie that names no session: the new one has no token yet.
        let app = support::app().await;
        let stale = app.client();
        stale.set_cookie(support::COOKIE, "v1.not-a-valid.cookie.value");
        post(&stale, body(&[field("_csrf", &token)]))
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
    }

    #[tokio::test]
    async fn a_token_part_that_could_be_read_two_ways_is_403() {
        let (_app, client, token) = signed_in().await;
        for head in [
            "Content-Disposition: form-data; name=\"_csrf\"\r\nContent-Disposition: form-data; name=\"title\"",
            "Content-Disposition: form-data; name=\"title\"\r\nContent-Disposition: form-data; name=\"_csrf\"",
            "Content-Disposition: form-data; name=\"_csrf\"; filename=\"t.txt\"",
            "Content-Disposition: form-data; name=\"_csrf\"; name=\"title\"",
            "X-Note: 1\nContent-Disposition: form-data; name=\"_csrf\"",
            "X-Note: 1\r\n Content-Disposition: form-data; name=\"_csrf\"",
        ] {
            post(&client, with_head(head, &token))
                .send()
                .await
                .assert_error(403, "Invalid CSRF token");
        }
        // Padding after the first delimiter.
        let padded = body(&[field("_csrf", &token)]).replacen(
            &format!("--{BOUNDARY}\r\n"),
            &format!("--{BOUNDARY} \r\n"),
            1,
        );
        post(&client, padded)
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
    }

    #[tokio::test]
    async fn a_token_part_that_ends_past_8_kib_must_send_the_header() {
        let (_app, client, token) = signed_in().await;
        let head = |pad: usize| {
            format!(
                "X-Pad: {}\r\nContent-Disposition: form-data; name=\"_csrf\"",
                "p".repeat(pad)
            )
        };
        let padded = with_head(&head(8 * 1024), &token);
        post(&client, padded.clone())
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
        post(&client, padded)
            .header("x-csrf-token", &token)
            .send()
            .await
            .assert_status(200);
        // Under the cap, the same part passes.
        post(&client, with_head(&head(7 * 1024), &token))
            .send()
            .await
            .assert_status(200);
    }

    #[tokio::test]
    async fn a_header_that_is_wrong_is_not_rescued_by_the_part() {
        let (_app, client, token) = signed_in().await;
        post(&client, body(&[field("_csrf", &token)]))
            .header("x-csrf-token", "wrong")
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
        // The header alone passes, whatever the body holds.
        let sent = body(&[field("title", "Agra"), field("_csrf", "whatever")]);
        let response = post(&client, sent.clone())
            .header("x-csrf-token", &token)
            .send()
            .await
            .assert_status(200);
        assert_eq!(response.text(), sent);
    }

    #[tokio::test]
    async fn a_token_in_the_url_or_a_boundary_that_is_not_one_does_not_count() {
        let (_app, client, token) = signed_in().await;
        client
            .post(&format!("/api/probe/echo?_csrf={token}"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(body(&[field("title", "Agra")]))
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
        let sent = body(&[field("_csrf", &token)]);
        for kind in [
            format!("multipart/form-data; boundary={BOUNDARY}; boundary=other"),
            format!("multipart/mixed; boundary={BOUNDARY}"),
            "multipart/form-data".to_owned(),
            format!("multipart/form-data; boundary={}", "b".repeat(71)),
        ] {
            client
                .post("/api/probe/echo")
                .header("content-type", kind)
                .body(sent.clone())
                .send()
                .await
                .assert_error(403, "Invalid CSRF token");
        }
    }

    #[tokio::test]
    async fn a_session_less_multipart_form_is_untouched() {
        let app = support::app().await;
        let fresh = app.client();
        let sent = body(&[field("title", "Agra"), field("_csrf", "whatever")]);
        let response = post(&fresh, sent.clone()).send().await.assert_status(200);
        assert_eq!(response.text(), sent);
    }
}

/// Says `/api/probe/write` checks the origin of its posts in place of the token.
struct OriginChecked;

impl navree_kernel::Module for OriginChecked {
    fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
        m.contribute(navree_http::csrf::SameOriginOnly("/api/probe/write"));
    }
}

// A route that changes only a preference the browser keeps (a theme toggle on a page cached
// for everyone, which carries no token) is posted to without the token by a reader with a
// session, and still refuses a post from any other site.
#[tokio::test]
async fn a_same_origin_only_path_takes_a_post_from_this_site_without_the_token() {
    let app = support::base()
        .module(navree_session::SessionModule::for_root(
            navree_session::SessionOptions::default(),
        ))
        .module(OriginChecked)
        .build()
        .await;
    let client = app.client();
    client.csrf_cookie().await;

    client
        .post("/api/probe/write")
        .header("sec-fetch-site", "same-origin")
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/write")
        .header("origin", "https://app.example")
        .header("host", "app.example")
        .send()
        .await
        .assert_status(200);
    // A request that says nothing of where it comes from passes only with no session to
    // ride on (a script); with the session cookie it is refused.
    app.client()
        .post("/api/probe/write")
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/write")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    // Every state-changing method at the path is checked alike.
    client
        .put("/api/probe/write")
        .header("sec-fetch-site", "cross-site")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");

    // Another site, a sibling subdomain or a navigation typed in is refused.
    for site in ["cross-site", "same-site", "none"] {
        client
            .post("/api/probe/write")
            .header("sec-fetch-site", site)
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
    }
    for origin in [
        "https://evil.example",
        "https://app.example.evil.example",
        "null",
    ] {
        client
            .post("/api/probe/write")
            .header("origin", origin)
            .header("host", "app.example")
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
    }
    // Without a session the same check holds.
    app.client()
        .post("/api/probe/write")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://evil.example")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");

    // Every other path still needs the token.
    client
        .post("/api/probe/put")
        .header("sec-fetch-site", "same-origin")
        .json(&json!({ "key": "k", "value": 1 }))
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
}

/// A page writes the session's token masked afresh in every answer, so no two
/// pages carry the same string; each of them posts, as the cookie's value does,
/// for as long as the session keeps its token.
mod masked_tokens {
    use navree_http::csrf::mask_token;
    use navree_testing::TestClient;

    use super::plain_forms::signed_in;
    use crate::support;

    /// The token the form page at `/api/probe/form` carries this time.
    async fn drawn(client: &TestClient) -> String {
        let html = client
            .get("/api/probe/form")
            .send()
            .await
            .assert_status(200)
            .text();
        let marker = r#"name="_csrf" value=""#;
        let start = html.find(marker).expect("a token field") + marker.len();
        let token = html[start..].split('"').next();
        token.unwrap_or_default().to_owned()
    }

    /// Posts `token` in a form's `_csrf` field and in each header.
    async fn post_all_ways(client: &TestClient, token: &str, status: u16) {
        let response = client
            .post("/api/probe/echo")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("_csrf={token}&title=Agra"))
            .send()
            .await;
        assert_eq!(response.status().as_u16(), status, "the field: {token}");
        if status == 200 {
            assert_eq!(response.text(), "title=Agra");
        }
        for header in ["x-csrf-token", "x-xsrf-token"] {
            let response = client
                .post("/api/probe/write")
                .header(header, token)
                .send()
                .await;
            assert_eq!(response.status().as_u16(), status, "{header}: {token}");
        }
    }

    #[tokio::test]
    async fn a_form_drawn_twice_carries_two_tokens_and_both_post() {
        let (_app, client, token) = signed_in().await;
        let first = drawn(&client).await;
        let second = drawn(&client).await;
        assert_ne!(first, second, "a fresh mask for every page");
        assert!(!first.contains(&token) && !second.contains(&token));
        // The page drawn first still posts after the second was drawn, and so does the
        // cookie's value.
        for given in [&first, &second, &token] {
            post_all_ways(&client, given, 200).await;
        }
        // The same page again, after posts: the token is the session's until it changes.
        post_all_ways(&client, &first, 200).await;
    }

    #[tokio::test]
    async fn a_wrong_token_fails_masked_or_not() {
        let (_app, client, token) = signed_in().await;
        let page = drawn(&client).await;
        let mut wrong = token.clone();
        let last = wrong.pop().unwrap();
        wrong.push(if last == 'a' { 'b' } else { 'a' });
        // Another session's page.
        let other_app = support::app().await;
        let other = other_app.client();
        other.csrf_cookie().await;
        let other_page = drawn(&other).await;
        for given in [
            mask_token(&wrong),
            mask_token(&token[..39]),
            page[..page.len() - 1].to_owned(),
            format!("{page}A"),
            other_page,
        ] {
            post_all_ways(&client, &given, 403).await;
        }
    }
}

/// The session layer vouches for a request (`TokenChecked` in its extensions) only when it
/// compared the session's token and the token matched; a form's origin check reads it.
mod vouched {
    use navree_http::csrf::{TokenChecked, mask_token};
    use navree_http::extract::Request;
    use navree_http::middleware::{self, Next};
    use navree_http::response::Response;
    use navree_http::{GlobalLayer, LayerSlot, ModuleBuilderHttpExt};
    use navree_kernel::{Module, ModuleBuilder};
    use navree_session::SessionModule;
    use navree_testing::TestClient;

    use super::plain_forms::signed_in;
    use crate::support;

    /// What the handler at `/api/probe/vouched` says of a request.
    async fn said(client: &TestClient, post: bool, header: Option<&str>) -> String {
        let mut request = if post {
            client.post("/api/probe/vouched")
        } else {
            client.get("/api/probe/vouched")
        };
        if let Some(token) = header {
            request = request.header("x-csrf-token", token);
        }
        request.send().await.assert_status(200).text()
    }

    #[tokio::test]
    async fn only_a_matching_token_puts_the_mark_on_the_request() {
        let (app, client, token) = signed_in().await;
        let masked = mask_token(&token);
        for given in [token.as_str(), masked.as_str()] {
            assert_eq!(said(&client, true, Some(given)).await, "checked", "{given}");
        }
        // In a form's field: on the request rebuilt without it, which the handler gets.
        let field = client
            .post("/api/probe/vouched")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("_csrf={masked}"))
            .send()
            .await
            .assert_status(200);
        assert_eq!(field.text(), "checked");
        // A method that changes nothing is not checked, so nothing vouches for it.
        assert_eq!(said(&client, false, None).await, "unchecked");
        // No session cookie: no token was compared.
        assert_eq!(said(&app.client(), true, None).await, "unchecked");
        // A wrong token never reaches the handler.
        client
            .post("/api/probe/vouched")
            .header("x-csrf-token", "wrong")
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
    }

    /// Puts the mark on every request, outside the session layer.
    struct Planted;

    impl Module for Planted {
        fn register(&self, m: &mut ModuleBuilder) {
            m.http_layer(GlobalLayer::new(LayerSlot::Edge, "plant", |_, router| {
                Ok(router.layer(middleware::from_fn(plant)))
            }));
        }
    }

    async fn plant(mut request: Request, next: Next) -> Response {
        request.extensions_mut().insert(TokenChecked);
        next.run(request).await
    }

    #[tokio::test]
    async fn a_mark_from_before_the_session_layer_is_taken_away() {
        let app = support::base()
            .module(SessionModule::for_root(Default::default()))
            .module(Planted)
            .build()
            .await;
        let client = app.client();
        client.csrf_cookie().await;
        assert_eq!(said(&client, false, None).await, "unchecked");
        assert_eq!(said(&app.client(), true, None).await, "unchecked");
    }
}
