//! The building blocks compose: a blog app (`tests/blog`) written with only
//! the facade and its macros, booted whole and exercised over HTTP through
//! `TestApp`: JWT login and refresh, abilities on records and lists,
//! validation, a queued mail on publish, the audit trail, the OpenAPI
//! document, CASL rules for the front end, `check`, and a wiring mistake.

mod blog;

use keel::audit::{AuditFilter, AuditLog};
use keel::auth::AuthService;
use keel::mail::Mailer;
use keel::prelude::*;
use keel::queue::Queue;
use keel::testing::{TempDirGuard, TestApp, TestResponse, temp_sqlite_url};
use serde_json::{Value, json};
use uuid::Uuid;

use blog::posts::PostService;

/// A password that passes the default policy.
const PASSWORD: &str = "correct horse battery";

/// The booted blog and its database (deleted after the app).
struct Blog {
    app: TestApp,
    _db: TempDirGuard,
}

/// A signed-in user: their id and tokens.
struct Author {
    id: Uuid,
    access: String,
    refresh: String,
}

impl Blog {
    async fn boot() -> Blog {
        let (url, db) = temp_sqlite_url();
        let app = TestApp::from_builder(blog::app(blog::sources(url))).await;
        Blog { app, _db: db }
    }

    /// Creates the account (as a seeder would) and logs in over HTTP.
    async fn sign_in(&self, name: &str, email: &str) -> Author {
        self.app
            .get::<AuthService>()
            .await
            .create_user(name, email, PASSWORD)
            .await
            .expect("the user is created");
        let body = self
            .login(email, PASSWORD)
            .await
            .assert_status(200)
            .json_value();
        Author {
            id: serde_json::from_value(body["user"]["id"].clone()).expect("user.id is a UUID"),
            access: body["access_token"]
                .as_str()
                .expect("access_token")
                .to_owned(),
            refresh: body["refresh_token"]
                .as_str()
                .expect("refresh_token")
                .to_owned(),
        }
    }

    async fn login(&self, email: &str, password: &str) -> TestResponse {
        self.app
            .client()
            .post("/api/auth/login")
            .json(&json!({ "email": email, "password": password }))
            .send()
            .await
    }

    /// Writes a draft as `author`; its id.
    async fn write(&self, author: &Author, title: &str) -> String {
        let response = self
            .app
            .client()
            .post("/api/v1/posts")
            .bearer(&author.access)
            .json(&json!({ "title": title, "body": "Lorem ipsum." }))
            .send()
            .await
            .assert_status(201)
            .assert_json_path("title", json!(title))
            .assert_json_path("published", json!(false));
        let id = response.json_value()["id"].as_str().expect("id").to_owned();
        let location = format!("/api/v1/posts/{id}");
        assert_eq!(response.header("location"), Some(location.as_str()));
        id
    }

    /// Publishes `id` as `author`.
    async fn publish(&self, author: &Author, id: &str) -> TestResponse {
        self.app
            .client()
            .post(&format!("/api/v1/posts/{id}/publish"))
            .bearer(&author.access)
            .send()
            .await
    }

    /// `GET /api/v1/posts` as `who` (a guest when `None`).
    async fn list(&self, who: Option<&Author>) -> Value {
        let request = self.app.client().get("/api/v1/posts");
        let request = match who {
            Some(author) => request.bearer(&author.access),
            None => request,
        };
        request.send().await.assert_status(200).json_value()
    }
}

/// The titles of a page of posts.
fn titles(page: &Value) -> Vec<&str> {
    page["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .map(|post| post["title"].as_str().expect("title"))
        .collect()
}

#[tokio::test]
async fn jwt_login_refresh_and_reuse_detection() {
    let blog = Blog::boot().await;
    let alice = blog.sign_in("Alice", "alice@example.com").await;
    blog.app
        .client()
        .get("/api/auth/me")
        .bearer(&alice.access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(alice.id))
        .assert_json_path("email", json!("alice@example.com"));

    let refreshed = blog
        .app
        .client()
        .post("/api/auth/refresh")
        .json(&json!({ "refresh_token": alice.refresh }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("token_type", json!("Bearer"))
        .json_value();
    let access = refreshed["access_token"].as_str().expect("access_token");
    assert_ne!(refreshed["refresh_token"], json!(alice.refresh));
    blog.app
        .client()
        .get("/api/auth/me")
        .bearer(access)
        .send()
        .await
        .assert_status(200);

    // A refresh token is single-use.
    blog.app
        .client()
        .post("/api/auth/refresh")
        .json(&json!({ "refresh_token": alice.refresh }))
        .send()
        .await
        .assert_error(401, "Invalid refresh token");
    blog.login("alice@example.com", "wrong password")
        .await
        .assert_error(401, "Invalid credentials");
    // Registration is off in this app.
    blog.app
        .client()
        .post("/api/auth/register")
        .json(&json!({ "name": "Eve", "email": "eve@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn guests_list_only_published_posts() {
    let blog = Blog::boot().await;
    let alice = blog.sign_in("Alice", "alice@example.com").await;
    let bob = blog.sign_in("Bob", "bob@example.com").await;
    let live = blog.write(&alice, "Hello world").await;
    blog.write(&alice, "Secret draft").await;
    blog.publish(&alice, &live).await.assert_status(200);

    let guest = blog.list(None).await;
    assert_eq!(titles(&guest), ["Hello world"]);
    assert_eq!(guest["meta"]["total"], json!(1));
    assert_eq!(guest["meta"]["current_page"], json!(1));
    // The author also sees her drafts (newest first); others do not.
    assert_eq!(
        titles(&blog.list(Some(&alice)).await),
        ["Secret draft", "Hello world"]
    );
    assert_eq!(titles(&blog.list(Some(&bob)).await), ["Hello world"]);
}

#[tokio::test]
async fn author_updates_own_post_and_others_get_403() {
    let blog = Blog::boot().await;
    let alice = blog.sign_in("Alice", "alice@example.com").await;
    let bob = blog.sign_in("Bob", "bob@example.com").await;
    let id = blog.write(&alice, "First title").await;
    let path = format!("/api/v1/posts/{id}");

    blog.app
        .client()
        .patch(&path)
        .bearer(&alice.access)
        .json(&json!({ "title": "Better title" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Better title"))
        .assert_json_path("author_id", json!(alice.id));

    blog.app
        .client()
        .patch(&path)
        .bearer(&bob.access)
        .json(&json!({ "title": "Hijacked" }))
        .send()
        .await
        .assert_error(403, "Forbidden");
    blog.app
        .client()
        .get(&path)
        .bearer(&bob.access)
        .send()
        .await
        .assert_status(403);
    blog.publish(&bob, &id).await.assert_status(403);
    blog.app
        .client()
        .get(&path)
        .bearer(&alice.access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Better title"));
    blog.app
        .client()
        .get(&format!("/api/v1/posts/{}", Uuid::now_v7()))
        .bearer(&alice.access)
        .send()
        .await
        .assert_status(404);
    // Guests must sign in to write.
    blog.app
        .client()
        .post("/api/v1/posts")
        .json(&json!({ "title": "Anonymous", "body": "..." }))
        .send()
        .await
        .assert_error(401, "Unauthorized");
}

#[tokio::test]
async fn invalid_input_is_a_400_naming_the_fields() {
    let blog = Blog::boot().await;
    let alice = blog.sign_in("Alice", "alice@example.com").await;
    let response = blog
        .app
        .client()
        .post("/api/v1/posts")
        .bearer(&alice.access)
        .json(&json!({ "title": "Hi", "body": "" }))
        .send()
        .await
        .assert_validation_error("title")
        .assert_validation_error("body");
    assert_eq!(response.json_value()["error"], json!("Bad Request"));
    // Nothing was written.
    assert_eq!(blog.list(Some(&alice)).await["meta"]["total"], json!(0));
}

#[tokio::test]
async fn publishing_queues_a_mail_to_the_author() {
    let blog = Blog::boot().await;
    let alice = blog.sign_in("Alice", "alice@example.com").await;
    let id = blog.write(&alice, "Launch day").await;
    blog.publish(&alice, &id)
        .await
        .assert_status(200)
        .assert_json_path("published", json!(true));

    let mailer = blog.app.get::<Mailer>().await;
    let sent = || {
        mailer
            .array_transport()
            .expect("the array transport")
            .sent()
    };
    assert!(sent().is_empty(), "the mail is queued, not sent inline");
    let queue = blog.app.get::<Queue>().await;
    assert_eq!(queue.pending_count("default").await.expect("count"), 1);

    let stats = queue.run_pending().await.expect("the queue runs");
    assert_eq!((stats.succeeded, stats.failed), (1, 0));
    let mails = sent();
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].to, ["alice@example.com"]);
    assert_eq!(mails[0].subject, "Your post is live");
    assert_eq!(
        mails[0].text.as_deref(),
        Some("Hi Alice, \"Launch day\" is live.")
    );
    assert!(
        mails[0]
            .html
            .as_deref()
            .is_some_and(|html| html.contains("<strong>Launch day</strong>"))
    );
}

#[tokio::test]
async fn a_forbidden_publish_queues_nothing() {
    let blog = Blog::boot().await;
    let alice = blog.sign_in("Alice", "alice@example.com").await;
    let bob = blog.sign_in("Bob", "bob@example.com").await;
    let id = blog.write(&alice, "Not yours").await;
    blog.publish(&bob, &id).await.assert_status(403);
    let queue = blog.app.get::<Queue>().await;
    assert_eq!(queue.pending_count("default").await.expect("count"), 0);
}

#[tokio::test]
async fn changes_and_sign_ins_are_audited() {
    let blog = Blog::boot().await;
    let alice = blog.sign_in("Alice", "alice@example.com").await;
    let id = blog.write(&alice, "Audited").await;
    blog.app
        .client()
        .patch(&format!("/api/v1/posts/{id}"))
        .bearer(&alice.access)
        .json(&json!({ "body": "Edited." }))
        .send()
        .await
        .assert_status(200);

    let audit = blog.app.get::<AuditLog>().await;
    audit.flush().await;
    let filter = AuditFilter {
        subject_type: Some("Post".to_owned()),
        subject_id: Some(id.clone()),
        ..AuditFilter::default()
    };
    let page = audit
        .list(&filter, PageQuery::default())
        .await
        .expect("audit rows");
    let mut actions: Vec<&str> = page.data.iter().map(|row| row.action.as_str()).collect();
    actions.sort_unstable();
    assert_eq!(actions, ["post.created", "post.updated"]);
    for row in &page.data {
        assert_eq!(row.actor_id, Some(alice.id), "{row:?}");
        assert!(row.request_id.is_some(), "{row:?}");
    }
    let updated = page
        .data
        .iter()
        .find(|row| row.action == "post.updated")
        .expect("the update row");
    assert_eq!(
        updated
            .new_values
            .as_ref()
            .and_then(|values| values.get("body")),
        Some(&json!("Edited."))
    );

    let logins = audit
        .list(
            &AuditFilter {
                action: Some("auth.login".to_owned()),
                ..AuditFilter::default()
            },
            PageQuery::default(),
        )
        .await
        .expect("audit rows");
    assert_eq!(logins.data.len(), 1);
    assert_eq!(logins.data[0].actor_id, Some(alice.id));
}

#[tokio::test]
async fn openapi_lists_the_routes() {
    let blog = Blog::boot().await;
    let doc = blog
        .app
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(doc["info"]["title"], json!("Blog API"));
    for (path, method) in [
        ("/api/v1/posts", "get"),
        ("/api/v1/posts", "post"),
        ("/api/v1/posts/{id}", "get"),
        ("/api/v1/posts/{id}", "patch"),
        ("/api/v1/posts/{id}/publish", "post"),
        ("/api/auth/login", "post"),
        ("/api/auth/refresh", "post"),
        ("/api/me/abilities", "get"),
    ] {
        assert!(
            doc["paths"][path][method].is_object(),
            "{method} {path} is missing from {:#}",
            doc["paths"]
        );
    }
    assert_eq!(
        doc["paths"]["/api/v1/posts"]["post"]["summary"],
        json!("Write a draft")
    );
    assert!(doc["paths"]["/api/v1/posts"]["post"]["responses"]["201"].is_object());
    assert!(doc["components"]["schemas"]["CreatePost"].is_object());
    assert!(doc["components"]["schemas"]["UpdatePost"].is_object());
}

#[tokio::test]
async fn abilities_endpoint_serves_casl_rules() {
    let blog = Blog::boot().await;
    let alice = blog.sign_in("Alice", "alice@example.com").await;
    let response = blog
        .app
        .client()
        .get("/api/me/abilities")
        .bearer(&alice.access)
        .send()
        .await
        .assert_status(200);
    assert!(response.header("etag").is_some());
    assert_eq!(
        response.json_value(),
        json!([
            { "action": "read", "subject": "Post", "conditions": { "published": true } },
            { "action": "read", "subject": "Post", "conditions": { "author_id": alice.id } },
            { "action": "create", "subject": "Post" },
            { "action": "update", "subject": "Post", "conditions": { "author_id": alice.id } },
            { "action": "publish", "subject": "Post", "conditions": { "author_id": alice.id } },
        ])
    );
    blog.app
        .client()
        .get("/api/me/abilities")
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn check_command_validates_the_whole_app() {
    let (url, _db) = temp_sqlite_url();
    let report = blog::app(blog::sources(url.clone()))
        .validate()
        .expect("every import and provider lines up");
    let posts = report
        .modules
        .iter()
        .find(|m| m.name == "PostsModule")
        .expect("PostsModule is in the graph");
    assert!(
        posts.imports.iter().any(|name| name.contains("Access")),
        "{:?}",
        posts.imports
    );
    blog::app(blog::sources(url))
        .run_with_args(vec!["check".to_owned()])
        .await
        .expect("`check` passes");
}

/// Comments need `PostService`, but the module forgets to import
/// `PostsModule`.
#[injectable]
struct CommentService {
    #[allow(dead_code)]
    posts: Arc<PostService>,
}

#[module(providers = [CommentService])]
struct CommentsModule;

#[tokio::test]
async fn wiring_mistake_fails_boot_with_a_readable_error() {
    let (url, _db) = temp_sqlite_url();
    let error = blog::app(blog::sources(url.clone()))
        .module(CommentsModule)
        .boot()
        .await
        .expect_err("the boot fails")
        .to_string();
    for needle in [
        "CommentsModule",
        "PostService",
        "PostsModule",
        "does not import",
    ] {
        assert!(
            error.contains(needle),
            "`{needle}` is missing from: {error}"
        );
    }
    // `check` reports the same, without constructing anything.
    let check = blog::app(blog::sources(url))
        .module(CommentsModule)
        .run_with_args(vec!["check".to_owned()])
        .await
        .expect_err("`check` fails")
        .to_string();
    assert_eq!(check, error);
}
