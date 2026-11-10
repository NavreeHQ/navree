//! Uploads in the states the browser suite runs against: `/upload`, the plain and the
//! script-driven posts, the chunk routes, and the files kept in memory.
//!
//! | Page | Shows |
//! |---|---|
//! | `/upload` | a photo upload, a document upload with chunks, a form sent by hand, a field inside a form, a slow upload, and the states |
//! | `POST /upload/photo`, `POST /upload/docs`, `POST /upload/slow` | receive files (a redirect with no script, JSON for the module) |
//! | `POST /upload/chunks`, `PUT /upload/chunks/{id}/{n}`, `POST /upload/chunks/{id}/finish`, `DELETE /upload/chunks/{id}` | a large file in parts |
//! | `GET /upload/stored`, `GET /upload/stored.json` | the files kept, as a fragment and as JSON |
//! | `POST /upload/remove/{id}`, `POST /upload/reset` | take one away; start over (a hook for the suite) |
//! | `POST /upload/fail/{count}` | makes the next `count` begin or part requests answer 503, to prove a retry |
//! | `/upload/claim` | a derived form with a file field: the file is sent first, and the form posts its key |
//! | `POST /upload/claim/receipt` | stores the claim's receipt and answers with its key |
//! | `GET /upload/claimed` | what the claim's handler was given |

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::Duration;

use http::{HeaderMap, Method, StatusCode};
use navree::http::body::Bytes;
use navree::http::extract::{Multipart, Path, Query};
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, Json, RouteDef, UrlSigner};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::storage::{Disk, DiskConfig, Storage, StorageOptions, StoredFile};
use navree::ui::form::{FormView, Rejected};
use navree::ui::*;
use serde::Deserialize;

/// Who the proof app says is sending.
const OWNER: &str = "proof";
const MB: u64 = 1024 * 1024;

/// A file the page remembers.
#[derive(Clone)]
struct Kept {
    id: u32,
    name: String,
    size: u64,
    mime: String,
    path: String,
}

struct State {
    disk: Disk,
    photo: Uploads,
    docs: Uploads,
    kept: Mutex<Vec<Kept>>,
    next: AtomicU32,
}

static STATE: LazyLock<State> = LazyLock::new(|| {
    let mut options = StorageOptions {
        default: "scratch".into(),
        ..StorageOptions::default()
    };
    options.disks.clear();
    options.disks.insert("scratch".into(), DiskConfig::Memory);
    let disk = Storage::new(&options, b"ui-proof signing key")
        .expect("a memory storage")
        .default_disk();
    State {
        photo: Uploads::new(
            disk.clone(),
            "photos",
            UploadLimits::new(MB).accept(["image/png", "image/jpeg"]),
        ),
        docs: Uploads::new(disk.clone(), "documents", docs_limits()).chunk_bytes(MB),
        disk,
        kept: Mutex::new(Vec::new()),
        next: AtomicU32::new(1),
    }
});

fn docs_limits() -> UploadLimits {
    UploadLimits::new(20 * MB)
        .max_files(5)
        .accept(["application/pdf", "image/*", "text/csv"])
}

/// How many posts the suite has asked to fail: a hook for the tests of retry.
static FAILS: AtomicU32 = AtomicU32::new(0);

fn kept() -> Vec<Kept> {
    STATE
        .kept
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn keep(stored: &[StoredFile]) {
    let mut all = STATE.kept.lock().unwrap_or_else(PoisonError::into_inner);
    for file in stored {
        all.push(Kept {
            id: STATE.next.fetch_add(1, Ordering::Relaxed),
            name: file.original_name.clone().unwrap_or_default(),
            size: file.size,
            mime: file.mime.clone(),
            path: file.path.clone(),
        });
    }
}

fn rows() -> Vec<UploadedFile> {
    kept()
        .into_iter()
        .map(|file| {
            UploadedFile::new(file.name)
                .size(file.size)
                .remove(format!("/upload/remove/{}", file.id))
        })
        .collect()
}

fn section(id: &'static str, title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("id", id)
        .children((Heading::h2(title), Stack::gap(4).children(content)))
}

fn stored_list() -> Node {
    let files = kept();
    if files.is_empty() {
        return Text::muted("Nothing is stored yet.").into();
    }
    el("ul")
        .attr("id", "stored-names")
        .children(
            files
                .iter()
                .map(|file| {
                    el("li").child(format!(
                        "{} ({}, {})",
                        file.name,
                        file.mime,
                        UploadLimits::size_text(file.size)
                    ))
                })
                .collect::<Vec<Element>>(),
        )
        .into()
}

fn page(shell: &AppShell) -> AppPage {
    let photo = Upload::new("photo", "Profile photo")
        .id("up-photo")
        .action("/upload/photo")
        .limits(UploadLimits::new(MB).accept(["image/png", "image/jpeg"]));
    let docs = Upload::new("docs", "Documents")
        .id("up-docs")
        .action("/upload/docs")
        .limits(docs_limits())
        .chunks("/upload/chunks")
        .chunk_from(4 * MB)
        .refresh("up-stored")
        .files(rows());
    let manual = Upload::new("sheet", "Price list (CSV)")
        .id("up-manual")
        .action("/upload/docs")
        .manual()
        .limits(UploadLimits::new(MB).accept(["text/csv"]));
    let field = el("form")
        .attr("id", "up-form")
        .attr("method", "post")
        .attr("action", "/upload/docs")
        .attr("enctype", "multipart/form-data")
        .children((
            Upload::new("papers", "Papers for the loan application")
                .id("up-field")
                .limits(docs_limits())
                .hint("Gurpreet Singh, Ludhiana branch."),
            Button::primary("Apply").submit(),
        ));
    let slow = Upload::new("slow", "A slow upload")
        .id("up-slow")
        .action("/upload/slow")
        .limits(UploadLimits::new(20 * MB));
    let preview = Upload::new("avatar", "Avatar, shown small when chosen")
        .id("up-preview")
        .action("/upload/photo")
        .manual()
        .previews()
        .limits(UploadLimits::new(4 * MB).accept(["image/png", "image/jpeg"]));
    let states = (
        Upload::new("a", "Disabled")
            .id("up-disabled")
            .action("/upload/photo")
            .disabled(true),
        Upload::new("b", "With an error")
            .id("up-error")
            .action("/upload/photo")
            .required()
            .error("Choose a photo under 1 MB."),
        Upload::new("c", "Compact")
            .id("up-compact")
            .action("/upload/photo")
            .compact()
            .limits(UploadLimits::new(MB).accept(["image/*"])),
        Upload::new("d", "Hostile names")
            .id("up-hostile")
            .file(
                UploadedFile::new("<img src=x onerror=alert(1)>.pdf")
                    .size(1536)
                    .href("/ok"),
            )
            .file(UploadedFile::new("ਗੁਰਪ੍ਰੀਤ-ਸਿੰਘ-ਆਧਾਰ.pdf").size(240 * 1024)),
    );
    let right_to_left = el("div").attr("dir", "rtl").attr("id", "up-rtl").child(
        Upload::new("e", "ملف")
            .id("up-rtl-field")
            .action("/upload/photo")
            .limits(UploadLimits::new(MB).accept(["image/png"])),
    );
    shell.page("Upload").body(
        Stack::gap(10).children((
            section("photo", "A photo", photo),
            section(
                "documents",
                "Documents, with large files in chunks",
                (
                    docs,
                    Region::new("up-stored")
                        .src("/upload/stored")
                        .children(stored_list()),
                ),
            ),
            section("manual", "Sent by hand", manual),
            section("preview", "A picture shown before it is sent", preview),
            section("field", "A field in a form", field),
            section("slow", "A slow upload", slow),
            section("states", "States", states),
            section("rtl", "Right to left", right_to_left),
        )),
    )
}

// ---- A derived form's file field -------------------------------------------------------------

/// A claim for money spent: what it was for, and the bill. The receipt is a file field: with
/// the upload module it is sent as soon as it is chosen, and the form posts its key.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct Claim {
    /// What it was for
    #[validate(length(min = 1))]
    pub title: String,
    /// Receipt
    #[validate(skip)]
    #[form(widget = file, help = "A photo or a PDF of the bill, under 1 MB.")]
    pub receipt: String,
}

fn receipt_limits() -> UploadLimits {
    UploadLimits::new(MB).accept(["image/png", "image/jpeg", "application/pdf"])
}

/// The upload the claim's receipt is kept with: the form's own file field stores through it, as
/// an app's `Uploads` registered under the field's name.
pub fn receipts() -> Uploads {
    Uploads::new(STATE.disk.clone(), "receipts", receipt_limits())
}

fn claim_page(shell: &AppShell, form: FormView<Claim>) -> AppPage {
    shell.page("A claim").body(
        Card::titled("New claim").body(
            form.post("/upload/claim").submit("Claim").upload(
                "receipt",
                Upload::new("receipt", "Receipt")
                    .id("up-claim")
                    .send_to("/upload/claim/receipt")
                    .limits(receipt_limits()),
            ),
        ),
    )
}

/// `GET /upload/claimed`: what the claim's handler was given.
fn claimed(shell: &AppShell, params: &HashMap<String, String>) -> AppPage {
    let get = |key: &str| params.get(key).cloned().unwrap_or_default();
    shell
        .page("Claimed")
        .body(el("dl").attr("data-claim", "").children((
            el("dt").child("title"),
            el("dd").attr("data-field", "title").child(get("title")),
            el("dt").child("receipt"),
            el("dd").attr("data-field", "receipt").child(get("receipt")),
        )))
}

/// The claim pages, with the app's signer for the keys of the files sent first.
pub struct ClaimPages {
    signer: Arc<UrlSigner>,
    receipts: Arc<Uploads>,
}

impl Injectable for ClaimPages {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<UrlSigner>(),
            ProviderKey::named::<Uploads>("receipt"),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                signer: r.get::<UrlSigner>().await?,
                receipts: r.get_named::<Uploads>("receipt").await?,
            })
        })
    }
}

impl Controller for ClaimPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let me = Arc::clone(&self);
        vec![
            RouteDef::on(
                Method::GET,
                "!/upload/claim",
                |shell: AppShell| async move { claim_page(&shell, FormView::new()) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/upload/claim",
                |shell: AppShell, submit: Result<Submit<Claim>, Rejected<Claim>>| async move {
                    match submit {
                        Ok(Submit(claim)) => Reply::redirect(format!(
                            "/upload/claimed?title={}&receipt={}",
                            encode(&claim.title),
                            encode(&claim.receipt)
                        ))
                        .toast(ToastTone::Success, "Claimed")
                        .into_response(),
                        Err(rejected) => rejected.respond(|form| claim_page(&shell, form)),
                    }
                },
            )
            .public()
            .body_limit(self.receipts.body_limit()),
            // The file the form's upload sends first: stored, and answered with its key.
            RouteDef::on(
                Method::POST,
                "!/upload/claim/receipt",
                move |mut form: Multipart| {
                    let me = Arc::clone(&me);
                    async move {
                        match me.receipts.receive_into(&mut form, 1).await {
                            Ok(received) => received.into_keyed_response(&me.signer),
                            Err(error) => error.into_response(),
                        }
                    }
                },
            )
            .public()
            .body_limit(self.receipts.body_limit()),
            RouteDef::on(
                Method::GET,
                "!/upload/claimed",
                |shell: AppShell, Query(params): Query<HashMap<String, String>>| async move {
                    claimed(&shell, &params)
                },
            )
            .public(),
        ]
    }
}

/// `text` for a query.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

// ---- The posts -------------------------------------------------------------------------------

/// Fails the post when the suite asked for one to fail.
fn failing() -> Option<Response> {
    let left = FAILS.load(Ordering::Relaxed);
    if left > 0 {
        FAILS.store(left - 1, Ordering::Relaxed);
        return Some(StatusCode::SERVICE_UNAVAILABLE.into_response());
    }
    None
}

async fn receive(
    uploads: &Uploads,
    headers: &HeaderMap,
    mut form: Multipart,
    back: &str,
    room: usize,
) -> Response {
    match uploads.receive_into(&mut form, room).await {
        Ok(received) => {
            keep(&received.stored);
            uploads.respond(received, headers, back)
        }
        Err(error) => error.into_response(),
    }
}

async fn photo(headers: HeaderMap, form: Multipart) -> Response {
    receive(&STATE.photo, &headers, form, "/upload#photo", 1).await
}

async fn docs(headers: HeaderMap, form: Multipart) -> Response {
    let room = STATE.docs.limits().files().saturating_sub(kept().len());
    receive(&STATE.docs, &headers, form, "/upload#documents", room).await
}

/// Takes the files at once and answers late, so a page can be looked at while a file is on its way.
async fn slow(headers: HeaderMap, form: Multipart) -> Response {
    let response = receive(&STATE.docs, &headers, form, "/upload#slow", 5).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    response
}

async fn begin(Json(start): Json<ChunkStart>) -> Response {
    if let Some(failed) = failing() {
        return failed;
    }
    match STATE.docs.begin(OWNER, start).await {
        Ok(ticket) => Json(ticket).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn chunk(id: String, index: u32, body: Bytes) -> Response {
    if let Some(failed) = failing() {
        return failed;
    }
    match STATE.docs.put_chunk(OWNER, &id, index, body).await {
        Ok(()) => Json(ok_body()).into_response(),
        Err(error) => error.into_response(),
    }
}

/// `{"ok": true}`.
fn ok_body() -> std::collections::BTreeMap<&'static str, bool> {
    std::collections::BTreeMap::from([("ok", true)])
}

async fn finish(id: String) -> Response {
    match STATE.docs.finish(OWNER, &id).await {
        Ok(stored) => {
            keep(std::slice::from_ref(&stored));
            Received {
                stored: vec![stored],
                refused: Vec::new(),
            }
            .into_json_response()
        }
        Err(error) => error.into_response(),
    }
}

async fn abort(id: String) -> Response {
    match STATE.docs.abort(OWNER, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => error.into_response(),
    }
}

fn stored_json() -> Response {
    let files: Vec<std::collections::BTreeMap<&'static str, String>> = kept()
        .into_iter()
        .map(|file| {
            std::collections::BTreeMap::from([
                ("name", file.name),
                ("mime", file.mime),
                ("size", file.size.to_string()),
            ])
        })
        .collect();
    Json(files).into_response()
}

async fn remove(id: u32) -> Response {
    let gone = {
        let mut all = STATE.kept.lock().unwrap_or_else(PoisonError::into_inner);
        all.iter()
            .position(|file| file.id == id)
            .map(|at| all.remove(at))
    };
    if let Some(file) = gone {
        let _ = STATE.disk.delete(&file.path).await;
    }
    Reply::redirect("/upload#documents")
        .toast(ToastTone::Success, "The file was removed.")
        .into_response()
}

async fn reset() -> Response {
    let all: Vec<Kept> =
        std::mem::take(&mut *STATE.kept.lock().unwrap_or_else(PoisonError::into_inner));
    for file in all {
        let _ = STATE.disk.delete(&file.path).await;
    }
    for prefix in ["photos", "documents"] {
        for path in STATE.disk.list(prefix).await.unwrap_or_default() {
            let _ = STATE.disk.delete(&path).await;
        }
    }
    FAILS.store(0, Ordering::Relaxed);
    StatusCode::NO_CONTENT.into_response()
}

/// The proof app's upload routes.
pub struct UploadPages;

impl Injectable for UploadPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for UploadPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let files = STATE.docs.body_limit();
        let parts = STATE.docs.chunk_body_limit();
        vec![
            RouteDef::on(Method::GET, "!/upload", |shell: AppShell| async move {
                page(&shell)
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/upload/photo",
                |headers: HeaderMap, form: Multipart| async move { photo(headers, form).await },
            )
            .public()
            .body_limit(STATE.photo.body_limit()),
            RouteDef::on(
                Method::POST,
                "!/upload/docs",
                |headers: HeaderMap, form: Multipart| async move { docs(headers, form).await },
            )
            .public()
            .body_limit(files),
            RouteDef::on(
                Method::POST,
                "!/upload/slow",
                |headers: HeaderMap, form: Multipart| async move { slow(headers, form).await },
            )
            .public()
            .body_limit(files),
            RouteDef::on(
                Method::POST,
                "!/upload/chunks",
                |body: Json<ChunkStart>| async move { begin(body).await },
            )
            .public(),
            RouteDef::on(
                Method::PUT,
                "!/upload/chunks/{id}/{index}",
                |Path((id, index)): Path<(String, u32)>, body: Bytes| async move {
                    chunk(id, index, body).await
                },
            )
            .public()
            .body_limit(parts),
            RouteDef::on(
                Method::POST,
                "!/upload/chunks/{id}/finish",
                |Path(id): Path<String>| async move { finish(id).await },
            )
            .public(),
            RouteDef::on(
                Method::DELETE,
                "!/upload/chunks/{id}",
                |Path(id): Path<String>| async move { abort(id).await },
            )
            .public(),
            RouteDef::on(Method::GET, "!/upload/stored", || async {
                View::new("Stored").body(stored_list())
            })
            .public(),
            RouteDef::on(Method::GET, "!/upload/stored.json", || async {
                stored_json()
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/upload/remove/{id}",
                |Path(id): Path<u32>| async move { remove(id).await },
            )
            .public(),
            RouteDef::on(Method::POST, "!/upload/reset", || async { reset().await }).public(),
            RouteDef::on(
                Method::POST,
                "!/upload/fail/{count}",
                |Path(count): Path<u32>| async move {
                    FAILS.store(count, Ordering::Relaxed);
                    StatusCode::NO_CONTENT
                },
            )
            .public(),
        ]
    }
}
