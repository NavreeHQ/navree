//! The consent page the module draws with Navree UI: what the page shows for a request, the
//! decisions it posts, and who may use it.
#![cfg(feature = "ui")]

use std::sync::Arc;

use navree_kernel::{Factory, Module, ModuleBuilder};
use navree_oauth::testing::pkce_pair;
use navree_ui::{ExtensionFile, PAGE_CSP, Shell, UiExtension, UiModule};

use crate::support::{REDIRECT_URI, World, builder, param};

#[tokio::test]
async fn the_page_shows_the_request_and_the_decisions_go_back_to_the_client() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read", "offline_access"]).await;
    let (_, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read offline_access", &challenge)
        .await;

    let page = world
        .client()
        .get(&format!("/oauth/consent?request={request}"))
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("Authorize Agra mobile"), "{page}");
    assert!(
        page.contains("deals:read") && page.contains("Read your deals"),
        "{page}"
    );
    assert!(
        page.contains(&format!("/oauth/consent/{request}/approve"))
            && page.contains(&format!("/oauth/consent/{request}/deny")),
        "{page}"
    );

    // Allow: the browser goes back with a code and the state, as the JSON route's would.
    let approved = world
        .client()
        .post(&format!("/oauth/consent/{request}/approve"))
        .bearer(&token)
        .send()
        .await;
    assert!(approved.status().is_redirection(), "{}", approved.text());
    let location = approved.header("location").expect("a location").to_owned();
    assert!(
        location.starts_with(&format!("{REDIRECT_URI}?code=nvc_")),
        "{location}"
    );
    assert_eq!(param(&location, "state").as_deref(), Some("af0ifjsldkj"));

    // The request was decided once: the page no longer finds it.
    world
        .client()
        .get(&format!("/oauth/consent?request={request}"))
        .bearer(&token)
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn deny_sends_the_client_access_denied() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let (_, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    let denied = world
        .client()
        .post(&format!("/oauth/consent/{request}/deny"))
        .bearer(&token)
        .send()
        .await;
    assert!(denied.status().is_redirection(), "{}", denied.text());
    let location = denied.header("location").expect("a location").to_owned();
    assert_eq!(param(&location, "error").as_deref(), Some("access_denied"));
    assert_eq!(param(&location, "state").as_deref(), Some("af0ifjsldkj"));
}

#[tokio::test]
async fn a_guest_and_a_token_are_refused() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let (_, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    let path = format!("/oauth/consent?request={request}");

    // Nobody signed in.
    world.client().get(&path).send().await.assert_status(401);

    // An OAuth token may not decide for a person: get one, then try.
    let tokens = world
        .code_flow(client.client.id, &token, "deals:read")
        .await;
    let access = tokens["access_token"].as_str().expect("a token");
    let other = world
        .request(client.client.id, "deals:read", &pkce_pair().1)
        .await;
    world
        .client()
        .post(&format!("/oauth/consent/{other}/approve"))
        .bearer(access)
        .send()
        .await
        .assert_status(403);
}

/// A component of the app's own, with a stylesheet: an app that contributes one has pages that
/// start from a clean document.
static PROOF: UiExtension = UiExtension::new("app", "proof").sheet(ExtensionFile::__new(
    br#":where([data-slot^="app:"]){}"#,
    "34f360b879e8",
    Some("app"),
));

/// The module that contributes [`PROOF`].
struct ProofModule;

impl Module for ProofModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(PROOF);
    }
}

/// The app's shell, which the consent page is drawn in when the app has one.
struct ShellModule;

impl Module for ShellModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global();
        m.factory(Factory::<Shell>::new(|_| async {
            Ok(Arc::new(Shell::new("Agra")))
        }))
        .exports::<Shell>();
    }
}

/// The consent page of a request that `builder` makes, and what it says of itself: whether its
/// root element holds the mark of a page that starts clean, the header that says so, and its
/// content security policy.
async fn consent_page(builder: navree_testing::TestAppBuilder) -> (bool, Option<String>, String) {
    let world = World::with(builder).await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let (_, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    let page = world
        .client()
        .get(&format!("/oauth/consent?request={request}"))
        .bearer(&token)
        .send()
        .await
        .assert_status(200);
    let html = page.text();
    let start = html.find("<html").expect("a root element");
    let root = &html[start..start + html[start..].find('>').expect("a closed tag")];
    (
        root.contains("data-ui-fresh-page"),
        page.header("x-navree-fresh").map(str::to_owned),
        page.header("content-security-policy")
            .expect("a policy")
            .to_owned(),
    )
}

// The consent page approves a client's access to a person's account: in an app with a UI
// extension it starts from a clean document, drawn in the app's shell or on its own, and keeps
// the toolkit's policy; in an app with none it is the page it was.
#[tokio::test]
async fn the_consent_page_is_fresh() {
    let on_its_own = consent_page(builder().module(UiModule).module(ProofModule)).await;
    let in_the_shell = consent_page(
        builder()
            .module(UiModule)
            .module(ProofModule)
            .module(ShellModule),
    )
    .await;
    for (marked, header, policy) in [on_its_own, in_the_shell] {
        assert!(marked, "the page is marked");
        assert_eq!(header.as_deref(), Some("1"));
        assert_eq!(policy, PAGE_CSP);
    }
    let (marked, header, policy) = consent_page(builder().module(UiModule)).await;
    assert!(!marked, "no extension, no mark");
    assert_eq!(header, None);
    assert_eq!(policy, PAGE_CSP);
}
