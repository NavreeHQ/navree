---
name: navree-storage
description: "Stores and serves files in a Navree app with `StorageModule`: named disks, reading, writing and streaming files, multipart uploads checked by content with `store_upload`, public URLs and signed temporary URLs. Use when accepting file uploads, serving or downloading stored files, generating expiring download links, or configuring where files are kept."
---

# File storage

`StorageModule` gives the app named **disks** on `object_store`: `local` (a
directory) and `memory` (tests). Inject `Arc<Disk>` (the default disk) or
`Arc<Storage>` (any disk by name). Paths are relative, `/`-separated and
normalized; `..`, control characters and over-long paths fail with
`StorageError::InvalidPath`, so no path leaves the disk's root.

## Steps

1. Add `StorageModule::from_config()` to the root (a new app doesn't have it).
2. Configure disks (or keep the default `local` disk at `storage/app`).
3. Set `APP_KEY`: temporary URLs are signed with a key derived from it.
4. Inject `Arc<Disk>` / `Arc<Storage>` in a service or controller.
5. Test with a `memory` disk.

## Configuration

```toml
[storage]
default = "local"
base_url = "https://api.example.com"      # optional: absolute temporary URLs

[storage.disks.local]
driver = "local"
root = "storage/app"

[storage.disks.avatars]
driver = "local"
root = "storage/app/avatars"
public_url = "https://cdn.example.com/avatars"   # for Disk::url

[storage.disks.scratch]
driver = "memory"
```

Defining `storage.disks` replaces the default list: name every disk you use.

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
        self.disk.temporary_url(&path, Utc::now() + Duration::minutes(30))
    }
}
```

`get(path)` (bytes), `get_stream(path)`, `put_stream(path, stream)`,
`exists`, `size`, `delete` (succeeds when missing), `list(dir)` (recursive,
sorted), `url(path)` (needs `public_url`). There is no copy or move helper.
`storage.disk("avatars")?` picks a disk; `storage.default_disk()` the default.

## Uploads

```rust
use navree::axum::extract::Multipart;
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
- The file gets a random 40-character name with a matching extension;
  `StoredFile` has `disk`, `path`, `size`, `mime`, `original_name` (untrusted:
  display it, never use it as a path).
- A disallowed type is a 400 field error, an oversized file a 413.
- Every body is limited by `http.body_limit_bytes` (2 MiB) first: raise it
  for the app, or use a hand-written route with `RouteDef::body_limit(bytes)`.

## Serve files

```rust
use navree::axum::body::Body;
use navree::axum::response::{IntoResponse, Response};

#[get("/{*path}")]
async fn show(&self, Path(path): Path<String>) -> Result<Response> {
    let stream = self.storage.default_disk().get_stream(&path).await?;
    Ok(Body::from_stream(stream).into_response()) // set Content-Type yourself
}
```

Better for downloads: a **temporary URL**. `StorageModule` serves them at
`GET /storage/{disk}/{*path}?expires=..&signature=..` (public; the
signature is the credential): 403 for a bad or expired link, 404 when the
file is gone, HTML/SVG/JS sent as attachments. Keep expiry short and don't
log the links. A `StorageError` through `?` in a handler is a 404
(not found), 400 (invalid path) or 500.

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

## Common mistakes

- No `APP_KEY`: links stop working at every restart and differ per instance.
- Trusting the client's file name or `Content-Type`.
- Reading huge files with `get` instead of `get_stream`.
- `UploadRules::max_bytes` above the body limit that applies (the request
  is refused before your rules run).
- Expecting S3: only `local` and `memory` drivers exist today.

## Docs

- https://navree.com/docs/digging-deeper/storage/
