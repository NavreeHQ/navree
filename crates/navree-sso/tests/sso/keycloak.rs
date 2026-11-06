//! SAML sign-in against a real identity provider: Keycloak. The service
//! provider asks for a login with an AuthnRequest, a browser (a small cookie-
//! keeping HTTP client here) signs the user in on Keycloak's own login page,
//! and the signed response Keycloak posts back is verified by the assertion
//! consumer service and signs the user in.
//!
//! Unlike the other SAML tests, nothing here is written by Navree: the
//! metadata, the signature, the canonical XML, the assertion and its
//! conditions all come from Keycloak.
//!
//! These run in the integration workflow, which starts Keycloak with
//! `scripts/integration-services.sh start identity` (variable
//! `NAVREE_TEST_KEYCLOAK_URL`, with the realm `navree` of
//! `scripts/services/integrations/keycloak/navree-realm.json`); they are
//! `#[ignore]`d in the other suites. Run them with `-- --ignored`. A missing
//! variable fails the test when `NAVREE_TEST_REQUIRE_SERVICES=1` and prints
//! `SKIPPED` otherwise.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_sso::{NewConnection, SsoOwner};
use reqwest::header::{COOKIE, LOCATION, SET_COOKIE};
use serde_json::json;

use crate::support::{self, World};

/// The user of the realm, and the password it was imported with.
const USER: &str = "harpreet.kaur@example.in";
const PASSWORD: &str = "Passw0rd-Test!";
/// The SAML client of the realm (the service provider's entity id) and the
/// name of its IdP-initiated sign-in URL.
const SLUG: &str = "keycloak";
/// The slug of the OpenID Connect connection.
const OIDC_SLUG: &str = "keycloak-oidc";

/// The value of `variable`; `None` after a notice, or a panic when
/// `NAVREE_TEST_REQUIRE_SERVICES=1`.
fn service(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: {variable} is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            println!("SKIPPED {test}: set {variable}");
            None
        }
    }
}

// -- a browser ------------------------------------------------------------------

/// A page the browser ended on.
struct Page {
    url: String,
    status: u16,
    body: String,
}

/// An HTTP client that keeps cookies and follows redirects, the two things a
/// browser does between a login redirect and the page that posts the answer.
struct Browser {
    http: reqwest::Client,
    cookies: BTreeMap<String, String>,
    /// A redirect to a URL that starts like this is not followed: the page
    /// is the service provider's, which this machine does not serve.
    stop_at: Option<String>,
}

impl Browser {
    fn new() -> Browser {
        // reqwest is built without a crypto provider; nothing here is HTTPS,
        // but the client needs one to be built.
        let _ = rustls::crypto::ring::default_provider().install_default();
        Browser {
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()
                .expect("a client"),
            cookies: BTreeMap::new(),
            stop_at: None,
        }
    }

    /// Stops at redirects to URLs that start with `prefix`, returning a page
    /// with that URL.
    fn stopping_at(mut self, prefix: &str) -> Browser {
        self.stop_at = Some(prefix.to_owned());
        self
    }

    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Requests `url` (a POST of `form` when there is one) and follows
    /// redirects, keeping every cookie set on the way.
    async fn visit(&mut self, url: &str, form: Option<&[(&str, &str)]>) -> Page {
        let mut url = url.to_owned();
        let mut form = form;
        for _ in 0..10 {
            let mut request = match form {
                Some(fields) => self.http.post(&url).form(fields),
                None => self.http.get(&url),
            };
            if !self.cookies.is_empty() {
                request = request.header(COOKIE, self.cookie_header());
            }
            let response = request
                .send()
                .await
                .unwrap_or_else(|error| panic!("{url}: {error}"));
            for header in response.headers().get_all(SET_COOKIE) {
                let text = header.to_str().unwrap_or_default();
                if let Some((name, value)) =
                    text.split(';').next().and_then(|pair| pair.split_once('='))
                {
                    if value.is_empty() {
                        self.cookies.remove(name.trim());
                    } else {
                        self.cookies
                            .insert(name.trim().to_owned(), value.to_owned());
                    }
                }
            }
            let status = response.status().as_u16();
            if (300..400).contains(&status)
                && let Some(location) = response.headers().get(LOCATION)
            {
                let location = location.to_str().expect("a location").to_owned();
                url = reqwest::Url::parse(&url)
                    .and_then(|base| base.join(&location))
                    .expect("a URL")
                    .to_string();
                if self
                    .stop_at
                    .as_deref()
                    .is_some_and(|prefix| url.starts_with(prefix))
                {
                    return Page {
                        url,
                        status,
                        body: String::new(),
                    };
                }
                // A redirect after a POST is followed with a GET.
                form = None;
                continue;
            }
            return Page {
                url,
                status,
                body: response.text().await.expect("a body"),
            };
        }
        panic!("{url}: too many redirects");
    }
}

// -- HTML -----------------------------------------------------------------------

/// `&amp;`, `&quot;`, `&#x2F;`... undone.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';').filter(|end| *end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|n| n.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The value of attribute `name` in `tag` (any case), unescaped.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let key = format!(" {name}=\"");
    let start = lower.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(unescape(&tag[start..end]))
}

/// The `action` of the form `marker` appears in.
fn form_action(html: &str, marker: &str) -> Option<String> {
    let at = html.find(marker)?;
    let start = html[..at]
        .rfind("<form")
        .or_else(|| html[..at].rfind("<FORM"))?;
    let end = html[start..].find('>')? + start;
    attribute(&html[start..end], "action")
}

/// Every `<input>` with a name: `(name, value)`.
fn inputs(html: &str) -> Vec<(String, String)> {
    let lower = html.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<input") {
        let start = from + at;
        let Some(length) = lower[start..].find('>') else {
            break;
        };
        let tag = &html[start..start + length];
        if let Some(name) = attribute(tag, "name") {
            found.push((name, attribute(tag, "value").unwrap_or_default()));
        }
        from = start + length;
    }
    found
}

// -- the flow -------------------------------------------------------------------

/// The SAML connection to Keycloak's realm, with its domain verified.
async fn connect(world: &World, keycloak: &str) {
    let descriptor = Browser::new()
        .visit(
            &format!("{keycloak}/realms/navree/protocol/saml/descriptor"),
            None,
        )
        .await;
    assert_eq!(
        descriptor.status, 200,
        "the realm exists: {}",
        descriptor.body
    );
    let metadata = descriptor.body;
    let sso = world.sso().await;
    let input = NewConnection::saml(SsoOwner::App, "Keycloak", metadata).slug(SLUG);
    let connection = world
        .write(|uow| async move { sso.create_connection(&uow, input).await })
        .await
        .expect("the connection is created from Keycloak's metadata");
    world.verified_domain(&connection, "example.in").await;
}

/// Submits the login form on `page` (Keycloak's login page) with the realm's
/// user and returns the page that answers.
async fn submit_login(browser: &mut Browser, page: &Page) -> Page {
    assert_eq!(page.status, 200, "{}: {}", page.url, page.body);
    let action = form_action(&page.body, "kc-form-login")
        .unwrap_or_else(|| panic!("no login form on {}:\n{}", page.url, page.body));
    browser
        .visit(&action, Some(&[("username", USER), ("password", PASSWORD)]))
        .await
}

/// Logs in on the page `page` and returns the form the browser would post to
/// the service provider: `SAMLResponse` and `RelayState` when there is one.
async fn log_in(browser: &mut Browser, page: &Page) -> Vec<(String, String)> {
    let answered = submit_login(browser, page).await;
    assert_eq!(answered.status, 200, "{}: {}", answered.url, answered.body);
    let form = inputs(&answered.body);
    assert!(
        form.iter().any(|(name, _)| name == "SAMLResponse"),
        "Keycloak did not post a SAML response:\n{}",
        answered.body
    );
    let acs = form_action(&answered.body, "SAMLResponse").expect("the form posts somewhere");
    assert!(
        acs.ends_with(&format!("/api/sso/saml/{SLUG}/acs")),
        "the response is for {acs}"
    );
    form.into_iter()
        .filter(|(name, _)| name == "SAMLResponse" || name == "RelayState")
        .collect()
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_user_signs_in_with_keycloaks_signed_saml_response() {
    let test = "a_user_signs_in_with_keycloaks_signed_saml_response";
    let Some(keycloak) = service("NAVREE_TEST_KEYCLOAK_URL", test) else {
        return;
    };
    let world = support::world().await;
    connect(&world, &keycloak).await;

    let url = world.login_url(SLUG, Some("/deals")).await;
    assert!(
        url.starts_with(&format!("{keycloak}/realms/navree/protocol/saml?")),
        "{url}"
    );
    let mut browser = Browser::new();
    let login = browser.visit(&url, None).await;
    let form = log_in(&mut browser, &login).await;

    let body = world
        .acs(SLUG, &form)
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!(USER))
        .assert_json_path("user.name", json!("Harpreet Kaur"))
        .assert_json_path("token_type", json!("Bearer"))
        .assert_json_path("return_to", json!("/deals"))
        .json_value();
    // The tokens work.
    world
        .app
        .client()
        .get("/api/auth/me")
        .bearer(body["access_token"].as_str().unwrap())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email", json!(USER));
    // The same response again is a replay.
    world.acs(SLUG, &form).await.assert_status(400);
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_response_changed_after_keycloak_signed_it_is_refused() {
    let test = "a_response_changed_after_keycloak_signed_it_is_refused";
    let Some(keycloak) = service("NAVREE_TEST_KEYCLOAK_URL", test) else {
        return;
    };
    let world = support::world().await;
    connect(&world, &keycloak).await;
    let url = world.login_url(SLUG, None).await;
    let mut browser = Browser::new();
    let login = browser.visit(&url, None).await;
    let form = log_in(&mut browser, &login).await;

    // The same response, naming another person in the assertion.
    let forged: Vec<(String, String)> = form
        .iter()
        .map(|(name, value)| {
            if name != "SAMLResponse" {
                return (name.clone(), value.clone());
            }
            let xml = String::from_utf8(STANDARD.decode(value).expect("base64")).expect("XML");
            assert!(xml.contains(USER), "the response names the user:\n{xml}");
            let xml = xml.replace(USER, "admin@example.in");
            (name.clone(), STANDARD.encode(xml))
        })
        .collect();

    let refused = world.acs(SLUG, &forged).await;
    assert!(
        refused.status().as_u16() >= 400,
        "a forged response was accepted: {}",
        refused.text()
    );
    assert!(
        !refused.text().contains("access_token"),
        "{}",
        refused.text()
    );
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_response_nobody_asked_for_is_refused_unless_idp_initiated_sign_in_is_allowed() {
    let test = "a_response_nobody_asked_for_is_refused_unless_idp_initiated_sign_in_is_allowed";
    let Some(keycloak) = service("NAVREE_TEST_KEYCLOAK_URL", test) else {
        return;
    };
    let world = support::world().await;
    connect(&world, &keycloak).await;

    // Keycloak's IdP-initiated sign-in: no AuthnRequest came first.
    let mut browser = Browser::new();
    let login = browser
        .visit(
            &format!("{keycloak}/realms/navree/protocol/saml/clients/navree"),
            None,
        )
        .await;
    let form = log_in(&mut browser, &login).await;

    let refused = world.acs(SLUG, &form).await;
    assert!(
        refused.status().as_u16() >= 400,
        "an unsolicited response was accepted: {}",
        refused.text()
    );
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_user_signs_in_with_keycloaks_openid_connect() {
    let test = "a_user_signs_in_with_keycloaks_openid_connect";
    let Some(keycloak) = service("NAVREE_TEST_KEYCLOAK_URL", test) else {
        return;
    };
    let world = support::world().await;
    let sso = world.sso().await;
    let input = NewConnection::oidc(
        SsoOwner::App,
        "Keycloak (OpenID Connect)",
        format!("{keycloak}/realms/navree"),
        "navree-oidc",
        "oidc-client-secret",
    )
    .slug(OIDC_SLUG);
    let connection = world
        .write(|uow| async move { sso.create_connection(&uow, input).await })
        .await
        .expect("the connection is created");
    world.verified_domain(&connection, "example.in").await;

    // The authorization request: Keycloak's endpoint from its discovery
    // document, with a state, a nonce and a PKCE challenge.
    let url = world.login_url(OIDC_SLUG, Some("/home")).await;
    assert!(
        url.starts_with(&format!(
            "{keycloak}/realms/navree/protocol/openid-connect/auth?"
        )),
        "{url}"
    );
    let mut browser = Browser::new().stopping_at("https://api.acme.test/");
    let login = browser.visit(&url, None).await;
    let redirected = submit_login(&mut browser, &login).await;

    // Keycloak sends the browser back to the service provider with a code.
    assert!(
        redirected
            .url
            .starts_with("https://api.acme.test/api/sso/oidc/keycloak-oidc/callback?"),
        "{} {}",
        redirected.url,
        redirected.body
    );
    let query: BTreeMap<String, String> = reqwest::Url::parse(&redirected.url)
        .expect("a URL")
        .query_pairs()
        .into_owned()
        .collect();
    world
        .app
        .client()
        .get("/api/sso/oidc/keycloak-oidc/callback")
        .query(&[
            ("code", query["code"].as_str()),
            ("state", query["state"].as_str()),
        ])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!(USER))
        .assert_json_path("return_to", json!("/home"));
}
