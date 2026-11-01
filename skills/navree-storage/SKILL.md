---
name: navree-storage
description: "Stores and serves files in a Navree app with `StorageModule`: named disks (local, memory, S3 and S3-compatible stores such as MinIO, Cloudflare R2 and DigitalOcean Spaces, Google Cloud Storage, Azure), reading, writing and streaming files, multipart uploads checked by content with `store_upload`, direct browser uploads with `presigned_put` and `accept_upload`, public and signed temporary URLs, and copying files between disks with `storage:sync`. Use when accepting file uploads, uploading straight to a bucket, serving or downloading stored files, generating expiring download links, moving files between disks, or configuring where files are kept."
---

# File storage

`StorageModule` gives the app named **disks**: `local` (a
directory), `memory` (tests), `s3` (AWS, MinIO, R2, Spaces; facade feature
`storage-s3`, in `full`), `gcs` and `azure` (opt-in features `storage-gcs`,
`storage-azure`). Inject `Arc<Disk>` (the default disk) or `Arc<Storage>`
(any disk by name). Paths are relative, `/`-separated and normalized; `..`,
control characters and over-long paths fail with `StorageError::InvalidPath`,
so no path leaves the disk's root.

## Steps

1. Add `StorageModule::from_config()` to the root (a new app doesn't have it).
2. Configure disks (or keep the default `local` disk at `storage/app`).
3. Set `APP_KEY`: the app's own links (local and memory disks) are signed
   with a key derived from it.
4. Inject `Arc<Disk>` / `Arc<Storage>` in a service or controller.
5. Test with a `memory` disk.

## Configuration

```toml
[storage]
default = "local"
base_url = "https://api.example.com"      # optional: absolute links for local/memory disks

[storage.disks.local]
driver = "local"
root = "storage/app"

[storage.disks.avatars]
driver = "local"
root = "storage/app/avatars"
public_url = "https://cdn.example.com/avatars"   # for Disk::url

[storage.disks.documents]
driver = "s3"
bucket = "acme-documents"
region = "eu-west-1"            # else AWS_REGION; `auto` for R2
# endpoint = "https://<account>.r2.cloudflarestorage.com"   # MinIO, R2, Spaces
# access_key = "..."            # else AWS_ACCESS_KEY_ID, IAM role, IMDS
# prefix = "production"         # every key under production/
# path_style = false            # default: true with an endpoint

[storage.disks.scratch]
driver = "memory"
```

- Defining `storage.disks` replaces the default list: name every disk you use.
- Secrets from the environment: `APP_STORAGE__DISKS__DOCUMENTS__SECRET_KEY`.
  AWS/Google/Azure fallbacks (`AWS_*`, `GOOGLE_*`, `AZURE_*`) are read from
  the process environment only, not `.env`.
- `gcs`: `bucket`, `service_account_path` or `service_account_key`.
  `azure`: `account`, `container`, `access_key`.
- A cloud driver whose feature is off fails the boot naming the feature.
  Cloud disks make no request at boot; the readiness check probes them.
- Local MinIO: `navree dock:add minio`, `navree dock up`, then
  `driver = "s3"` + `bucket`; Dock passes `AWS_ENDPOINT_URL` and keys to
  `navree dev` (with `cargo run`: `eval "$(navree dock env --export)"`).

## Read and write

```rust
use chrono::{Duration, Utc};
use navree::prelude::*;
use navree::storage::{Disk, StorageError};

#[injectable]
pub struct ReportFiles {
    disk: Arc<Disk>,
}

impl ReportFiles {
    /// Stores the report and returns a download link valid for 30 minutes.
    pub async fn publish(&self, quarter: &str, csv: String) -> Result<String, StorageError> {
        let path = format!("reports/{quarter}.csv");
        self.disk.put(&path, csv).await?;                     // atomic replace
        self.disk.temporary_url(&path, Utc::now() + Duration::minutes(30)).await
    }
}
```

`get(path)` (bytes), `get_stream(path)`, `put_stream(path, stream)`,
`exists`, `size`, `metadata` / `files(dir)` (`FileMeta { path, size,
last_modified }`), `delete` (succeeds when missing), `list(dir)` (recursive,
sorted), `url(path)` (needs `public_url`). No copy or move helper: stream
`get_stream` into another disk's `put_stream`.

On a `local` disk: a write goes through a private staging file (`name#1`),
is flushed to the device and renamed over the file (which keeps its permission
bits), so a file name whose last `#` is followed by digits only is refused and
never listed; symlinks are followed by reads and listings, and a write at the
name of a link replaces the link; on Windows a `:` in a name is stored as
`%3A`, and names ending in a dot or a space, device names (`NUL`, `COM1`...),
names with `~` and a digit and names with `< > " | ? *` are refused; deleting a
file removes the directories it leaves empty; a directory or a pipe reads as a
missing file.

## Uploads through the app (multipart)

```rust
use navree::http::extract::Multipart;
use navree::prelude::*;
use navree::storage::{Storage, StoredFile, UploadRules, store_upload};

#[injectable]
pub struct AvatarController {
    storage: Arc<Storage>,
}

#[controller("/avatars", version = "1")]
#[authenticated]
impl AvatarController {
    #[post("/")]
    async fn store(&self, mut form: Multipart) -> Result<Json<StoredFile>> { // Multipart: last argument
        let field = form
            .next_field()
            .await?
            .ok_or_else(|| AppError::bad_request("avatar should not be empty"))?;
        let disk = self.storage.default_disk();
        let rules = UploadRules::new(1024 * 1024, ["image/png", "image/jpeg", "image/webp"]);
        Ok(Json(store_upload(field, &disk, "avatars", rules).await?))
    }
}
```

- The type is sniffed from the content, not taken from the client; an SVG
  or HTML file is never an image. Wildcards (`image/*`) work.
- Every body is limited by `http.body_limit_bytes` (2 MiB) first. For large
  files use direct uploads instead.

## Direct uploads (browser to bucket)

Three requests: the browser declares type and size, the app signs a link;
the browser `PUT`s the file to it; the browser reports it done, the app
checks it.

```rust
use chrono::Duration;
use navree::http::CurrentPrincipal;
use navree::prelude::*;
use navree::storage::{PresignedUpload, Storage, StoredFile, UploadRules, upload_path};

fn rules() -> UploadRules {
    UploadRules::new(20 * 1024 * 1024, ["application/pdf", "image/*"])
}

// #[post("/")] #[authenticated]
async fn start(&self, CurrentPrincipal(me): CurrentPrincipal, Json(file): Json<NewUpload>)
    -> Result<Json<PresignedUpload>> {
    let path = upload_path(&format!("incoming/{}", me.user_id), &file.content_type);
    let rules = rules();
    let upload = self.storage.disk("documents")?
        .presigned_put(&path, Duration::minutes(15), &rules)
        .content_type(file.content_type)   // required when the rules list types
        .size(file.size)
        .await?;                           // 400/413 when the declared values break the rules
    Ok(Json(upload))                       // { disk, path, method, url, headers, expires_at }
}

// #[post("/complete")] #[authenticated]
async fn complete(&self, CurrentPrincipal(me): CurrentPrincipal, Json(done): Json<FinishedUpload>)
    -> Result<Json<StoredFile>> {
    if !done.path.starts_with(&format!("incoming/{}/", me.user_id)) {
        return Err(AppError::forbidden());   // only the caller's own uploads
    }
    Ok(Json(self.storage.disk("documents")?.accept_upload(&done.path, &rules()).await?))
}
```

- The browser sends `fetch(upload.url, { method: upload.method, headers:
  upload.headers, body: file })`.
- `accept_upload` checks size and sniffed type and **deletes** a bad file
  (`StorageError::Rejected`: 400, or 413 when too large). Always call it.
- Each rejection carries a `code` the front end translates:
  `file_type_not_allowed`, `missing_content_type`, `invalid_content_type`,
  `empty_file`, `payload_too_large` (and `upload_size_mismatch` from the
  local upload route). Handling a rejection yourself, keep it with
  `AppError::validation(..).with_code(rejection.code())`.
- What the link enforces: S3 the `Content-Type` (no size); GCS type and
  `x-goog-content-length-range`; Azure only expiry (send `x-ms-blob-type`);
  local/memory disks the app's `PUT /storage/{disk}/{*path}` route checks
  type, declared size and `max_bytes` while streaming.
- The bucket's CORS rules must allow `PUT` from the front end's origin with
  the listed headers. Navree doesn't change bucket settings.
- S3 and GCS links last at most 7 days (`StorageError::Signing` beyond).

## Serve files

```rust
use navree::http::body::Body;
use navree::http::response::{IntoResponse, Response};

#[get("/{*path}")]
async fn show(&self, Path(path): Path<String>) -> Result<Response> {
    let stream = self.storage.default_disk().get_stream(&path).await?;
    Ok(Body::from_stream(stream).into_response()) // set Content-Type yourself
}
```

Better for downloads: a **temporary URL** (`temporary_url(path,
expires_at).await`). Cloud disks: a provider-signed link straight to the
bucket. Local/memory disks: `StorageModule` serves
`GET /storage/{disk}/{*path}?expires=..&signature=..` (public; the signature
is the credential): 403 for a bad or expired link, 404 when the file is
gone, HTML/SVG/JS sent as attachments. Keep expiry short and don't log the
links. A `StorageError` through `?` in a handler is a 404 (`not_found`), 400
(`invalid_file_path`), 400/413 (rejected upload, a code per rule) or 500.

## Copy between disks

```sh
cargo run -- storage:sync local documents --dry-run
cargo run -- storage:sync local documents --prefix incoming --delete
```

Copies files missing on the target or of another size (compared by path
and size), four at a time; `--delete` removes target files the source
lacks. In code: `storage.sync("local", "documents", &SyncOptions { .. })`
returns a `SyncReport` (`copied`, `deleted`, `unchanged`, `bytes`,
`failed`).

## Test

```rust
use navree::prelude::*;
use navree::storage::{DiskConfig, Storage, StorageOptions};
use navree::testing::TestApp;

#[tokio::test]
async fn publishing_a_report_stores_it() {
    let app = TestApp::builder()
        .config("app.key", navree::crypto::generate_key())
        .module(StorageModule::for_root(StorageOptions {
            default: "memory".into(),
            disks: [("memory".to_owned(), DiskConfig::Memory)].into(),
            ..StorageOptions::default()
        }))
        .module(FilesModule)
        .build()
        .await;
    let link = app.get::<ReportFiles>().await.publish("2026-q3", "a,b\n".into()).await.unwrap();
    assert!(link.starts_with("/storage/memory/reports/2026-q3.csv?expires="));
    assert_eq!(app.get::<Storage>().await.default_disk().get("reports/2026-q3.csv").await.unwrap(), "a,b\n");
    app.client().get(&link).send().await.assert_status(200);
    app.shutdown().await.unwrap();
}
```

A memory disk signs direct uploads to the app's own route, so a test runs
the whole flow: `start`, then `app.client().put(&upload.url)` with
`upload.headers` and the body, then `complete`.

## Common mistakes

- No `APP_KEY`: local/memory links stop working at every restart and differ
  per instance.
- Trusting the client's file name or `Content-Type`; skipping
  `accept_upload` after a direct upload.
- Letting `complete` accept any path: tie paths to the user who asked.
- Reading huge files with `get` instead of `get_stream`.
- `UploadRules::max_bytes` above the body limit that applies (multipart).
- Forgetting the bucket's CORS rule for direct browser uploads.
- `gcs`/`azure` disks without turning on `storage-gcs`/`storage-azure`.

## Docs

- https://navree.com/docs/digging-deeper/storage/
