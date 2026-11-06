//! The workspace cookie of a signed-in browser is the panel's own: one it finds
//! at another path, or from another sign-in, names nothing.

use navree_auth::{AuthModule, AuthPages, AuthService};
use navree_session::{SessionModule, SessionOptions};
use navree_ui::UiModule;

use super::*;

/// The notes, for an app with real sign-in.
struct AuthedNotesModule;

impl Module for AuthedNotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("notes")
                .after(["auth"])
                .migration(SchemaMigration::new(
                    "m20261101_000001_create_notes",
                    CreateNotes,
                )),
        )
        .resource::<NoteResource>()
        .resource::<EveryNote>();
    }
}

const PASSWORD: &str = "correct horse battery";

async fn signed_in_app() -> TestApp {
    crate::servers::database(TestApp::builder())
        .config("app.key", navree_crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.routes", false)
        .module(navree_crypto::CryptoModule::from_config())
        .module(navree_db::DatabaseModule)
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(AuthModule::from_config())
        .module(UiModule)
        .module(AuthPages::new())
        .module(navree_access::AccessModule::for_root(options()))
        .module(WorkspaceModule::from_config())
        .module(AuthedNotesModule)
        .module(navree_helm::HelmModule::from_config())
        .build()
        .await
}

fn csrf(html: &str) -> String {
    let marker = "<meta name=\"csrf-token\" content=\"";
    let start = html.find(marker).unwrap() + marker.len();
    html[start..].split('"').next().unwrap().to_owned()
}

/// Signs `email` in on the app's sign-in page, and returns the panel page's
/// token for forms.
async fn sign_in(client: &TestClient, email: &str) -> String {
    client
        .get("/sign-in")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200);
    let xsrf = client
        .cookie("XSRF-TOKEN")
        .expect("the sign-in page handed out the token");
    client
        .post("/sign-in")
        .header("accept", "text/html")
        .form(&[
            ("email", email),
            ("password", PASSWORD),
            ("next", "/admin"),
            ("_csrf", xsrf.as_str()),
        ])
        .send()
        .await
        .assert_status(303);
    let page = client
        .get("/admin")
        .header("accept", "text/html")
        .send()
        .await
        .text();
    csrf(&page)
}

#[tokio::test]
async fn a_cookie_planted_at_another_path_names_nothing_after_sign_in() {
    let app = signed_in_app().await;
    let auth = app.get::<AuthService>().await;
    let user = auth
        .create_user("One", "one@navree.test", PASSWORD)
        .await
        .unwrap()
        .id;
    let acme = workspace(&app, user, "Acme").await.id;
    give_role(&app, user, "editor", &["helm.view", "notes.*"]).await;
    seed(&app, acme, "Acme secret").await;
    let client = app.client();
    // The test client's jar keeps one cookie of a name, so the planted one is
    // put back after sign-in: what clearing `{base}` doesn't reach.
    let token = sign_in(&client, "one@navree.test").await;
    client.set_cookie(WORKSPACE_COOKIE, &acme.to_string());
    let response = client.get(NOTES).send().await.assert_status(400);
    assert!(response.text().contains("Choose a workspace first"));
    // The form's works, for this sign-in.
    let response = client
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[
            ("workspace", acme.to_string().as_str()),
            ("_csrf", token.as_str()),
        ])
        .send()
        .await
        .assert_status(303);
    let chosen = response
        .set_cookie(WORKSPACE_COOKIE)
        .expect("a cookie")
        .value()
        .to_owned();
    client
        .get(NOTES)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(1));
    // Taken to another sign-in, it names nothing.
    let other = app.client();
    sign_in(&other, "one@navree.test").await;
    other.set_cookie(WORKSPACE_COOKIE, &chosen);
    other.get(NOTES).send().await.assert_status(400);
}
