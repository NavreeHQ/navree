//! An export and a menu inside a workspace: who may go on past the first chunk,
//! what a credential limited to a workspace reads, what an upload waits for.

use std::collections::BTreeMap;

use futures::StreamExt;
use navree_http::ModuleBuilderHttpExt;

use super::*;

const CSV: &str = "/admin/resources/notes/export.csv";

/// Enters the workspace the `x-test-entered` header names, as an app's own
/// middleware does.
struct AppEnters;

impl Module for AppEnters {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(navree_http::GlobalLayer::new(
            navree_http::LayerSlot::Context,
            "test-enter",
            |_ctx, router| {
                Ok(router.layer(navree_http::middleware::from_fn(
                    |request: navree_http::extract::Request,
                     next: navree_http::middleware::Next| async move {
                        let named = request
                            .headers()
                            .get("x-test-entered")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| Uuid::parse_str(v).ok());
                        let uow = request.extensions().get::<navree_db::Uow>().cloned();
                        if let (Some(workspace), Some(uow)) = (named, uow) {
                            enter(&uow, workspace).await;
                        }
                        next.run(request).await
                    },
                )))
            },
        ));
    }
}

/// Limits the request to the workspace the `x-test-pin` header names, as a
/// credential limited to one does.
struct Pin;

impl Module for Pin {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(navree_http::GlobalLayer::new(
            navree_http::LayerSlot::Context,
            "test-pin",
            |_ctx, router| {
                Ok(router.layer(navree_http::middleware::from_fn(
                    |mut request: navree_http::extract::Request,
                     next: navree_http::middleware::Next| async move {
                        let pin = request
                            .headers()
                            .get("x-test-pin")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| Uuid::parse_str(v).ok());
                        if let Some(pin) = pin {
                            request
                                .extensions_mut()
                                .insert(navree_workspace::PinnedWorkspace(pin));
                        }
                        next.run(request).await
                    },
                )))
            },
        ));
    }
}

async fn seed_many(app: &TestApp, workspace: Uuid, count: usize) {
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    enter(&uow, workspace).await;
    for i in 0..count {
        let note = Note {
            title: format!("Row {i:05}"),
            created_at: now(),
            ..Note::default()
        };
        Note::create(&uow, note).await.unwrap();
    }
    uow.commit().await.unwrap();
}

/// The status of a response, its lines and the error its body ended with.
async fn drain(response: http::Response<navree_http::body::Body>) -> (u16, usize, Option<String>) {
    let status = response.status().as_u16();
    let mut body = response.into_body().into_data_stream();
    let mut lines = 0;
    while let Some(frame) = body.next().await {
        match frame {
            Ok(data) => lines += data.split(|b| *b == b'\n').count() - 1,
            Err(error) => return (status, lines, Some(error.to_string())),
        }
    }
    (status, lines, None)
}

#[tokio::test]
async fn in_a_workspace_the_app_entered_with_no_gate_an_export_is_refused_as_a_page() {
    let app = crate::servers::database(TestApp::builder())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(navree_db::DatabaseModule)
        .module(navree_access::AccessModule::for_root(options()))
        .module(MemberNotesModule)
        .module(AppEnters)
        .module(navree_helm::HelmModule::from_config())
        .build()
        .await;
    let agra = Uuid::now_v7();
    seed_many(&app, agra, 600).await;
    let editor = editor(&app).await;
    let list = editor
        .get("/admin/resources/notes")
        .header("accept", "text/html")
        .header("x-test-entered", agra.to_string())
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(!list.contains("export.csv"), "not offered: {list}");
    let csv = editor
        .get(CSV)
        .header("accept", "text/html")
        .header("x-test-entered", agra.to_string())
        .send_stream()
        .await;
    let (status, lines, error) = drain(csv).await;
    assert_eq!(status, 403, "up front, not at row 500");
    assert!(lines < 100 && error.is_none(), "a page: {lines} {error:?}");
}

#[tokio::test]
async fn in_a_workspace_the_app_entered_an_export_asks_the_gate_first() {
    let builder = crate::servers::database(TestApp::builder()).module(AppEnters);
    let world = world(with_members(builder).build().await).await;
    seed_many(&world.app, world.agra, 600).await;
    // The gate doesn't know them: the page says so before any row.
    let outsider = world
        .outsider
        .get(CSV)
        .header("accept", "text/html")
        .header("x-test-entered", world.agra.to_string())
        .send_stream()
        .await;
    let (status, _, error) = drain(outsider).await;
    assert_eq!(status, 403);
    assert_eq!(error, None);
    // It does know a member, who gets the whole file.
    let member = world
        .member
        .get(CSV)
        .header("accept", "text/html")
        .header("x-test-entered", world.agra.to_string())
        .send_stream()
        .await;
    let (status, lines, error) = drain(member).await;
    assert_eq!((status, error), (200, None));
    assert!(lines > 600, "{lines}");
}

#[tokio::test]
async fn staff_whose_permission_goes_lose_an_open_export() {
    let (world, staff) = staff_world().await;
    seed_many(&world.app, world.gulati, 1100).await;
    let response = staff
        .get(CSV)
        .header("accept", "text/html")
        .header("X-Workspace", world.gulati.to_string())
        .send_stream()
        .await;
    assert_eq!(response.status(), 200);
    let mut body = response.into_body().into_data_stream();
    body.next().await.unwrap().unwrap();
    let service = world.app.get::<navree_access::AccessService>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    service
        .revoke(&uow, "support", "support.enter")
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let mut rest = 0;
    let mut error = None;
    while let Some(frame) = body.next().await {
        match frame {
            Ok(data) => rest += data.len(),
            Err(failure) => {
                error = Some(failure.to_string());
                break;
            }
        }
    }
    assert_eq!(rest, 0, "no more rows");
    assert!(error.is_some());
}

#[tokio::test]
async fn the_entry_of_an_export_says_which_workspace() {
    let builder = crate::servers::database(TestApp::builder())
        .config("audit.routes", false)
        .module(navree_audit::AuditModule::from_config());
    let world = world(with_members(builder).build().await).await;
    world
        .member
        .get(CSV)
        .header("accept", "text/html")
        .header("X-Workspace", world.agra.to_string())
        .send()
        .await
        .assert_status(200)
        .text();
    let log = world.app.get::<navree_audit::AuditLog>().await;
    log.flush().await;
    let rows = navree_audit::AuditLog::list(
        &log,
        &navree_audit::AuditFilter::default(),
        navree_db::PageQuery {
            page: 1,
            per_page: 100,
        },
    )
    .await
    .unwrap()
    .data;
    let entry = rows
        .iter()
        .find(|row| row.action == "helm.export")
        .expect("the entry");
    let entry = serde_json::to_value(entry).unwrap();
    assert_eq!(
        entry["metadata"]["workspace"],
        world.agra.to_string(),
        "{entry}"
    );
}

#[tokio::test]
async fn a_credential_limited_to_a_workspace_reads_only_its_count() {
    let builder = crate::servers::database(TestApp::builder()).module(Pin);
    let world = world(with_members(builder).build().await).await;
    join(&world.app, world.gulati, world.member_id).await;
    seed(&world.app, world.gulati, "Gulati two").await;
    seed(&world.app, world.gulati, "Gulati three").await;
    // Across workspaces, in Agra: everything the role of the database shows.
    let unpinned = world
        .member
        .get("/admin/counts")
        .header("X-Workspace", world.agra.to_string())
        .send()
        .await
        .json_value();
    let across = unpinned["counts"]["all-notes"]["value"].as_u64().unwrap();
    // The same user held to Agra gets what its list shows, not that number.
    let pinned = world
        .member
        .get("/admin/counts")
        .header("x-test-pin", world.agra.to_string())
        .send()
        .await
        .json_value();
    let list = world
        .member
        .get("/admin/api/resources/all-notes")
        .header("x-test-pin", world.agra.to_string())
        .send()
        .await
        .json_value();
    let rows = list["data"].as_array().unwrap().len() as u64;
    assert_eq!(rows, 1, "{list}");
    assert_eq!(pinned["counts"]["all-notes"]["value"], rows, "{pinned}");
    // (A role row-level security binds sees only Agra either way.)
    assert!(across >= rows, "{across}");
}

// ---------------------------------------------------------------------------
// Uploads
// ---------------------------------------------------------------------------

struct AttachNotes;

impl Injectable for AttachNotes {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(AttachNotes) })
    }
}

impl Resource for AttachNotes {
    type Model = Note;

    const LABEL: &'static str = "Attachments";
    const SLUG: &'static str = "attach-notes";
    const TITLE: &'static str = "title";

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("title").required().max_length(20),
            Field::file("body", "files")
                .accept(["image/png"])
                .max_size(1_000_000),
        ]
    }
}

struct AttachModule;

impl Module for AttachModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.resource::<AttachNotes>();
    }
}

fn files_in(dir: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            if entry.path().is_dir() {
                files_in(&entry.path())
            } else {
                1
            }
        })
        .sum()
}

fn multipart(title: &str) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.resize(10_000, 0);
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--edge\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\n{title}\r\n--edge\r\nContent-Disposition: form-data; name=\"body\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&png);
    body.extend_from_slice(b"\r\n--edge--\r\n");
    body
}

async fn upload_world() -> (World, std::path::PathBuf, TestClient) {
    let dir = std::env::temp_dir().join(format!("helm-uploads-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let options = navree_storage::StorageOptions {
        default: "files".to_owned(),
        disks: BTreeMap::from([(
            "files".to_owned(),
            navree_storage::DiskConfig::Local {
                root: dir.clone(),
                public_url: None,
            },
        )]),
        base_url: None,
    };
    let builder = crate::servers::database(TestApp::builder())
        .module(navree_storage::StorageModule::for_root(options))
        .module(AttachModule);
    let world = world(with_members(builder).build().await).await;
    let members = world.app.get::<Members>().await;
    let db = world.app.get::<Db>().await;
    let viewer_id = account(&world.app, "viewer").await;
    let uow = db.uow(None, None).await;
    uow.scope(members.add(&uow, world.agra, viewer_id, WorkspaceRole::parse("viewer")))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    give_role(&world.app, viewer_id, "editor", &["helm.view", "notes.*"]).await;
    let viewer = world.app.acting_as(navree_testing::principal(viewer_id));
    (world, dir, viewer)
}

fn post_upload(client: &TestClient, title: &str) -> navree_testing::TestRequest {
    client
        .post("/admin/resources/attach-notes")
        .header("accept", "text/html")
        .header("content-type", "multipart/form-data; boundary=edge")
        .body(multipart(title))
}

#[tokio::test]
async fn an_upload_with_no_workspace_stores_nothing() {
    let (world, dir, _) = upload_world().await;
    let response = post_upload(&world.outsider, "O").send().await;
    assert_eq!(response.status(), 400);
    assert_eq!(files_in(&dir), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn an_upload_a_role_refuses_stores_nothing_whichever_way_it_names_the_workspace() {
    let (world, dir, viewer) = upload_world().await;
    let by_header = post_upload(&viewer, "H")
        .header("X-Workspace", world.agra.to_string())
        .send()
        .await;
    assert_eq!(by_header.status(), 403);
    assert_eq!(files_in(&dir), 0);
    viewer
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", world.agra.to_string().as_str())])
        .send()
        .await;
    let by_cookie = post_upload(&viewer, "C").send().await;
    assert_eq!(by_cookie.status(), 403);
    assert_eq!(files_in(&dir), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_failed_create_leaves_no_file_behind() {
    let (world, dir, _) = upload_world().await;
    // Too long a title: the form comes back.
    let failed = post_upload(&world.member, &"x".repeat(30))
        .header("X-Workspace", world.agra.to_string())
        .send()
        .await;
    assert_eq!(failed.status(), 400);
    assert_eq!(files_in(&dir), 0);
    // A good one keeps its file.
    let made = post_upload(&world.member, "Fine")
        .header("X-Workspace", world.agra.to_string())
        .send()
        .await;
    assert_eq!(made.status(), 303);
    assert_eq!(files_in(&dir), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
