//! A form's file field (`#[form(widget = file)]`): drawn as an upload in a multipart form, a
//! file posted with the form stored through the app's `Uploads` and given to the handler as its
//! path, a key the upload module put in the form read as its file by the person it was made for,
//! and a key the app did not sign, made for someone else, made for another folder or for a file
//! that no longer fits, refused under its field. Runs with the `storage` feature.
#![cfg(feature = "storage")]

use std::sync::Arc;

use http::{Method, StatusCode};
use navree_http::extract::Multipart;
use navree_http::response::{IntoResponse, Response};
use navree_http::{Controller, HttpModule, ModuleBuilderHttpExt, RouteDef, UrlSigner};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_storage::{Disk, DiskConfig, Storage, StorageOptions};
use navree_testing::{TestApp, TestClient, principal};
use navree_ui::form::{Form as _, FormView, Rejected, Widget};
use navree_ui::upload::{UploadOwner, file_key, file_key_for, file_path, file_path_for};
use navree_ui::{
    Form, Received, Submit, UiModule, Upload, UploadLimits, Uploads, Validate, render,
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

/// The first bytes of a PNG file, and some more.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\x07\x07\x07\x07";
/// The first bytes of a Windows executable: a kind the receipts do not accept.
const EXE: &[u8] = b"MZ\x90\0\x03\0\0\0";
const BOUNDARY: &str = "navree-form-files";

#[derive(Debug, Deserialize, Validate, Form)]
struct Claim {
    /// Title
    #[validate(length(min = 1))]
    title: String,
    /// Receipt
    #[validate(skip)]
    #[form(widget = file)]
    receipt: String,
}

async fn claim(submit: Result<Submit<Claim>, Rejected<Claim>>) -> Response {
    match submit {
        Ok(Submit(claim)) => format!("{} {}", claim.title, claim.receipt).into_response(),
        Err(rejected) => rejected.into_response(),
    }
}

struct Pages {
    signer: Arc<UrlSigner>,
    uploads: Arc<Uploads>,
}

impl Injectable for Pages {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<UrlSigner>(), ProviderKey::of::<Uploads>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                signer: r.get::<UrlSigner>().await?,
                uploads: r.get::<Uploads>().await?,
            })
        })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let me = Arc::clone(&self);
        vec![
            RouteDef::on(Method::POST, "!/claims", claim).public(),
            // The file the form's upload sends first: stored, and answered with its key.
            RouteDef::on(
                Method::POST,
                "!/claims/receipt",
                move |owner: UploadOwner, mut form: Multipart| {
                    let me = Arc::clone(&me);
                    async move {
                        match me.uploads.receive(&mut form).await {
                            Ok(received) => {
                                received.into_keyed_response_for(&me.signer, owner.as_str())
                            }
                            Err(error) => error.into_response(),
                        }
                    }
                },
            )
            .public(),
        ]
    }
}

struct App {
    disk: Disk,
}

impl Module for App {
    fn register(&self, m: &mut ModuleBuilder) {
        let limits = UploadLimits::new(1024 * 1024).accept(["image/png", "application/pdf"]);
        m.value(Uploads::new(self.disk.clone(), "receipts", limits));
        m.controller::<Pages>();
    }
}

fn memory_disk() -> Disk {
    let mut options = StorageOptions {
        default: "scratch".into(),
        ..StorageOptions::default()
    };
    options.disks.clear();
    options.disks.insert("scratch".into(), DiskConfig::Memory);
    Storage::new(&options, b"signing key")
        .expect("a memory storage")
        .default_disk()
}

async fn app(disk: &Disk) -> TestApp {
    TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App { disk: disk.clone() })
        .build()
        .await
}

/// A multipart body: text `fields`, then `file` (name and bytes) as `receipt`.
fn multipart(fields: &[(&str, &str)], file: Option<(&str, &[u8])>) -> Vec<u8> {
    multipart_files(fields, file.as_slice())
}

/// A multipart body: text `fields`, then each of `files` (name and bytes) as `receipt`.
fn multipart_files(fields: &[(&str, &str)], files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    for (file_name, bytes) in files {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"receipt\"; filename=\"{file_name}\"\r\nContent-Type: image/png\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn post_multipart(app: &TestApp, body: Vec<u8>) -> navree_testing::TestResponse {
    app.client()
        .post("/claims")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .header("accept", "text/html")
        .body(body)
        .send()
        .await
}

/// A urlencoded post by `client`: a guest, or a person the test client acts as.
async fn post_form_by(client: &TestClient, body: String) -> navree_testing::TestResponse {
    client
        .post("/claims")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "text/html")
        .body(body)
        .send()
        .await
}

fn encoded(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

#[test]
fn the_derive_draws_a_file_field_as_an_upload_in_a_multipart_form() {
    let receipt = Claim::FIELDS
        .iter()
        .find(|field| field.name == "receipt")
        .unwrap();
    assert_eq!(receipt.widget, Widget::File);
    let html = render(
        &FormView::<Claim>::new()
            .post("/claims")
            .upload(
                "receipt",
                Upload::new("ignored", "Receipt").send_to("/claims/receipt"),
            )
            .into(),
    );
    assert!(html.contains(r#"enctype="multipart/form-data""#), "{html}");
    assert!(html.contains(r#"data-send="/claims/receipt""#), "{html}");
    // The form names the upload after its field, and gives it a field for the key.
    assert!(
        html.contains(r#"type="file""#) && html.contains(r#"name="receipt""#),
        "{html}"
    );
    assert!(html.contains(r#"data-slot="upload-key""#), "{html}");
    assert!(!html.contains(r#"name="ignored""#), "{html}");
}

#[tokio::test]
async fn a_file_posted_with_the_form_is_stored_and_the_handler_gets_its_path() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let response = post_multipart(
        &app,
        multipart(&[("title", "Diwali stall")], Some(("bill.png", PNG))),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK, "{}", response.text());
    let text = response.text();
    let path = text
        .strip_prefix("Diwali stall ")
        .expect("the title, then the path");
    assert!(
        path.starts_with("receipts/") && path.ends_with(".png"),
        "{path}"
    );
    assert!(disk.exists(path).await.unwrap(), "{path} is on the disk");
}

#[tokio::test]
async fn a_file_field_keeps_the_first_file_it_is_sent_and_no_other() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let response = post_multipart(
        &app,
        multipart_files(
            &[("title", "Diwali stall")],
            &[("first.png", PNG), ("second.png", PNG)],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK, "{}", response.text());
    let text = response.text();
    let path = text
        .strip_prefix("Diwali stall ")
        .expect("the title, then the path");
    assert_eq!(
        disk.list("receipts").await.unwrap(),
        vec![path.to_owned()],
        "the field's file is the only one on the disk"
    );
}

#[tokio::test]
async fn a_file_after_a_refused_one_is_not_stored() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let response = post_multipart(
        &app,
        multipart_files(
            &[("title", "Diwali stall")],
            &[("run.exe", EXE), ("second.png", PNG)],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.text().contains("run.exe"), "{}", response.text());
    assert!(
        disk.list("receipts").await.unwrap().is_empty(),
        "the refused field stores nothing"
    );
}

#[tokio::test]
async fn a_file_the_upload_refuses_is_an_error_under_its_field() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let response = post_multipart(
        &app,
        multipart(
            &[("title", "Diwali stall")],
            Some(("run.exe", b"MZ\x90\0\x03\0\0\0")),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.text().contains("run.exe"), "{}", response.text());
}

/// The path of a receipt an earlier request stored.
const STORED: &str = "receipts/k3v9q.png";

/// Puts `bytes` on the disk at `path`, as an earlier request would have stored a file.
async fn stored(disk: &Disk, path: &str, bytes: &[u8]) {
    disk.put(path, bytes.to_vec()).await.unwrap();
}

/// A post of the claim form with `key` as its receipt, by `client`.
async fn post_key(client: &TestClient, key: &str) -> navree_testing::TestResponse {
    post_form_by(client, format!("title=Lohri&receipt={}", encoded(key))).await
}

/// The post was refused under its field: the person chooses the file again.
fn assert_chooses_again(response: &navree_testing::TestResponse) {
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        response.text().contains("choose the file again"),
        "{}",
        response.text()
    );
}

#[tokio::test]
async fn a_key_made_for_the_person_posting_is_read_as_its_file() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let signer = app.ctx().get::<UrlSigner>().await.unwrap();
    stored(&disk, STORED, PNG).await;
    // The key of a file sent by a guest is read from a guest.
    let key = file_key_for(&signer, "", STORED);
    let response = post_key(&app.client(), &key).await;
    assert_eq!(response.status(), StatusCode::OK, "{}", response.text());
    assert_eq!(response.text(), format!("Lohri {STORED}"));
    // The key of a file sent by a signed-in person is read from that person.
    let person = Uuid::new_v4();
    let key = file_key_for(&signer, &person.to_string(), STORED);
    let response = post_key(&app.acting_as(principal(person)), &key).await;
    assert_eq!(response.status(), StatusCode::OK, "{}", response.text());
    assert_eq!(response.text(), format!("Lohri {STORED}"));
}

#[tokio::test]
async fn a_path_or_a_changed_key_is_refused_under_its_field() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let signer = app.ctx().get::<UrlSigner>().await.unwrap();
    stored(&disk, STORED, PNG).await;
    let changed = file_key(&signer, STORED).replace("k3v9q", "other");
    let changed_for = file_key_for(&signer, "", STORED).replace("k3v9q", "other");
    for receipt in [STORED.to_owned(), changed, changed_for] {
        assert_chooses_again(&post_key(&app.client(), &receipt).await);
    }
}

#[tokio::test]
async fn a_key_that_names_a_path_and_no_person_is_refused_under_its_field() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let signer = app.ctx().get::<UrlSigner>().await.unwrap();
    stored(&disk, STORED, PNG).await;
    let key = file_key(&signer, STORED);
    assert_chooses_again(&post_key(&app.client(), &key).await);
}

#[tokio::test]
async fn a_key_made_for_someone_else_is_refused_under_its_field() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let signer = app.ctx().get::<UrlSigner>().await.unwrap();
    stored(&disk, STORED, PNG).await;
    let (alice, meera) = (Uuid::new_v4(), Uuid::new_v4());
    let key = file_key_for(&signer, &alice.to_string(), STORED);
    // Another person, and a guest, post it: neither gets the file.
    for client in [app.acting_as(principal(meera)), app.client()] {
        assert_chooses_again(&post_key(&client, &key).await);
    }
    // And a key made for a guest is not a signed-in person's.
    let guest_key = file_key_for(&signer, "", STORED);
    assert_chooses_again(&post_key(&app.acting_as(principal(meera)), &guest_key).await);
    // The person it was made for posts it.
    let response = post_key(&app.acting_as(principal(alice)), &key).await;
    assert_eq!(response.status(), StatusCode::OK, "{}", response.text());
}

#[tokio::test]
async fn a_key_for_a_file_outside_the_fields_folder_is_refused() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let signer = app.ctx().get::<UrlSigner>().await.unwrap();
    // Files the field's own folder does not hold: another upload's, one a level down, one of
    // the parts of an upload that is not finished.
    for path in [
        "avatars/k3v9q.png",
        "receipts/old/k3v9q.png",
        "receipts/_parts/0a1b/2c3d/00000000",
    ] {
        stored(&disk, path, PNG).await;
        let key = file_key_for(&signer, "", path);
        assert_chooses_again(&post_key(&app.client(), &key).await);
    }
}

#[tokio::test]
async fn a_key_for_a_file_that_no_longer_fits_the_field_is_refused() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let signer = app.ctx().get::<UrlSigner>().await.unwrap();
    // The receipts take files up to 1 MiB of PNG or PDF.
    let mut large = PNG.to_vec();
    large.resize(1024 * 1024 + 1, 7);
    stored(&disk, "receipts/ok.png", PNG).await;
    stored(&disk, "receipts/large.png", &large).await;
    stored(&disk, "receipts/run.png", EXE).await;
    stored(&disk, "receipts/empty.png", b"").await;
    for path in [
        "receipts/large.png",
        "receipts/run.png",
        "receipts/empty.png",
        "receipts/gone.png",
    ] {
        let key = file_key_for(&signer, "", path);
        assert_chooses_again(&post_key(&app.client(), &key).await);
    }
    let key = file_key_for(&signer, "", "receipts/ok.png");
    let response = post_key(&app.client(), &key).await;
    assert_eq!(response.status(), StatusCode::OK, "{}", response.text());
}

#[tokio::test]
async fn the_key_the_upload_route_answers_is_read_back_by_the_person_who_sent_the_file() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let (alice, meera) = (Uuid::new_v4(), Uuid::new_v4());
    let sent = app
        .acting_as(principal(alice))
        .post("/claims/receipt")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(multipart(&[], Some(("bill.png", PNG))))
        .send()
        .await;
    assert_eq!(sent.status(), StatusCode::OK, "{}", sent.text());
    let json: Value = serde_json::from_str(&sent.text()).expect("JSON");
    let key = json["stored"][0]["key"].as_str().expect("a key").to_owned();
    // The person who sent the file takes it into the form.
    let response = post_key(&app.acting_as(principal(alice)), &key).await;
    assert_eq!(response.status(), StatusCode::OK, "{}", response.text());
    let text = response.text();
    let path = text.strip_prefix("Lohri ").expect("the title, then the path");
    assert!(disk.exists(path).await.unwrap(), "{path} is on the disk");
    // Nobody else does.
    for client in [app.acting_as(principal(meera)), app.client()] {
        assert_chooses_again(&post_key(&client, &key).await);
    }
}

#[tokio::test]
async fn a_refused_form_keeps_the_file_it_stored_as_a_key() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let signer = app.ctx().get::<UrlSigner>().await.unwrap();
    // The title is missing: the file is stored, and the form comes back with its key.
    let response = post_multipart(&app, multipart(&[("title", "")], Some(("bill.png", PNG)))).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let page = response.text();
    let start = page
        .find(r#"data-slot="upload-key""#)
        .expect("the form carries the key");
    let value = page[start..]
        .split(r#"value=""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a value")
        .replace("&amp;", "&");
    let path = file_path_for(&signer, "", "receipts", &value).expect("the key the app signed");
    assert!(disk.exists(&path).await.unwrap());
    assert!(
        !page.contains(&format!(r#"value="{path}""#)),
        "the path stays on the server"
    );
    // The key is the one the same person can post again.
    let again = post_key(&app.client(), &value).await;
    assert_eq!(again.status(), StatusCode::OK, "{}", again.text());
}

#[tokio::test]
async fn a_refused_form_shows_a_signed_in_person_the_key_that_person_can_post() {
    let disk = memory_disk();
    let app = app(&disk).await;
    let person = Uuid::new_v4();
    let client = app.acting_as(principal(person));
    let response = client
        .post("/claims")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .header("accept", "text/html")
        .body(multipart(&[("title", "")], Some(("bill.png", PNG))))
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let page = response.text();
    let start = page
        .find(r#"data-slot="upload-key""#)
        .expect("the form carries the key");
    let value = page[start..]
        .split(r#"value=""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a value")
        .replace("&amp;", "&");
    let again = post_key(&client, &value).await;
    assert_eq!(again.status(), StatusCode::OK, "{}", again.text());
    // Not a guest, nor another person.
    assert_chooses_again(&post_key(&app.client(), &value).await);
    assert_chooses_again(&post_key(&app.acting_as(principal(Uuid::new_v4())), &value).await);
}

#[tokio::test]
async fn the_upload_route_answers_each_stored_file_with_its_key() {
    let signer = UrlSigner::new(b"a key only this test knows");
    let received = Received {
        stored: vec![navree_storage::StoredFile {
            disk: "scratch".into(),
            path: "receipts/k3v9q.png".into(),
            size: 12,
            mime: "image/png".into(),
            original_name: Some("bill.png".into()),
        }],
        refused: Vec::new(),
    };
    let json = received.json_with_keys(&signer);
    let key = json["stored"][0]["key"].as_str().expect("a key");
    assert_eq!(
        file_path(&signer, key).as_deref(),
        Some("receipts/k3v9q.png")
    );
    assert_eq!(json["stored"][0]["name"], "bill.png");
    assert!(json["stored"][0].get("path").is_none(), "{json}");
}

#[tokio::test]
async fn the_upload_route_can_answer_each_stored_file_with_a_key_for_its_owner() {
    let signer = UrlSigner::new(b"a key only this test knows");
    let received = Received {
        stored: vec![navree_storage::StoredFile {
            disk: "scratch".into(),
            path: "receipts/k3v9q.png".into(),
            size: 12,
            mime: "image/png".into(),
            original_name: Some("bill.png".into()),
        }],
        refused: Vec::new(),
    };
    let json = received.json_with_keys_for(&signer, "user-7");
    let key = json["stored"][0]["key"].as_str().expect("a key");
    assert_eq!(
        file_path_for(&signer, "user-7", "receipts", key).as_deref(),
        Some("receipts/k3v9q.png")
    );
    assert_eq!(file_path_for(&signer, "user-8", "receipts", key), None);
    assert_eq!(file_path(&signer, key), None, "it is not a key of the path alone");
    assert!(json["stored"][0].get("path").is_none(), "{json}");
}
