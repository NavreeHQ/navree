---
name: navree-media
description: "Files attached to models with navree-media: the opt-in `media` facade feature (and `media-ffmpeg`), `MediaModule` and the `media` config section, `impl HasMedia` (`MEDIA_TYPE`, `MEDIA_TABLE`, `media_key`, `media_collections`), collections (`single_file`, `only_keep_latest`, `accepts`, `max_size`, `disk`, `conversions_disk`), `MediaModule::for_feature::<M>()` / `m.media_model::<M>()`, `Media::add(&model, collection, MediaSource)` with `bytes`/`url`/`disk`/`file` sources and `PendingMedia::save(&uow)`, the `media` table (`MediaRecord`, a navree-orm model) and `MediaItem`, `resource()` with URLs and `srcset`s, `get`/`first`/`all`/`list(MediaQuery)`, `save`/`reorder`/`delete`/`clear`/`delete_all` and deleting with the model, `Conversion` (fit, format, quality, greyscale, blur, responsive, nonqueued, frame_at, transcode), the `media.conversions` job (`GenerateConversions`), `media:regenerate` and `queue_regenerate`, metadata stripping, `images::{convert, probe, strip_metadata, rasterize_svg}`, remote files through the SSRF guard, `archive::{Archive, ArchiveLimits, ZipBuilder}` and `unpack`, `text::extract` for PDF/DOCX/HTML, video and audio through FFmpeg (`Transcode`, `Ffmpeg`, `FfmpegFake`), `MediaError` codes, and `MediaFake`. Use when adding uploads, thumbnails, avatars, logos, galleries or attachments to a model, re-hosting a remote image, generating WebP or responsive images, unpacking a zip or SCORM package, extracting text from documents for search or RAG, making video posters or transcodes, testing uploads, or debugging file_type_not_allowed, payload_too_large, remote_file_blocked, invalid_image, unsafe_archive or missing conversions."
---

# Media

`navree::media` (crate `navree-media`, facade feature `media`, **opt-in**:
not part of `full`; `media-ffmpeg` adds video and audio) attaches files to
models and converts, presents and deletes them. Everything below is in the
code: when in doubt read `crates/navree-media/src` and its tests in
`crates/navree-media/tests/media/` (`service.rs`, `module.rs`,
`database.rs`, `remote.rs`, `fake.rs`). The docs page is
`apps/website/content/docs/packages/media.mdx`.

## Steps: add media to a model

1. Turn the feature on:

```toml
[dependencies]
navree = { git = "https://github.com/NavreeHQ/navree", features = ["media"] } # or "media-ffmpeg"
```

   This brings `navree::media` and puts `MediaModule`, `Media`, `HasMedia`
   and `ModuleBuilderMediaExt` in the prelude.

2. Add the module to the app root, after `StorageModule` and
   `DatabaseModule` (both required; `media.store = "memory"` drops the
   database). `QueueModule` makes conversions run on a worker; without it
   they run while the file is added:

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(StorageModule::from_config())
    .module(QueueModule::from_config())
    .module(MediaModule::from_config()) // section `media`; or MediaModule::for_root(MediaOptions { .. })
```

   The `media` migration set creates the `media` table.

3. Implement `HasMedia` on the model and declare its collections:

```rust
use navree::media::{Conversion, Fit, HasMedia, MediaCollections, OutputFormat};

impl HasMedia for Course {
    const MEDIA_TYPE: &'static str = "Course";   // stored in model_type: keep it stable
    const MEDIA_TABLE: &'static str = "courses"; // deleting a course deletes its media

    fn media_key(&self) -> String {
        self.id.to_string() // must match the key change events report (to_string of the id)
    }

    fn media_collections(collections: &mut MediaCollections) {
        collections
            .collection("thumbnail")
            .single_file()
            .accepts(["image/*"])
            .conversion(Conversion::new("card").fit(Fit::Cover, 640, 360).format(OutputFormat::WebP));
        collections
            .collection("attachments")
            .accepts(["application/pdf", "image/*", "text/*"])
            .max_size(10 * 1024 * 1024);
    }
}
```

4. Register it in the feature module:
   `#[module(imports = [MediaModule::for_feature::<Course>()])]` (or
   `m.media_model::<Course>()` in a hand-written `register`). Needed for
   deleting with the model and for `media:regenerate` to know the
   conversions before any file is added in the process.

5. Add files from a handler with `Arc<Media>` and the request's `Uow`:

```rust
let field = form.next_field().await?.ok_or_else(|| AppError::bad_request("thumbnail should not be empty"))?;
let file_name = field.file_name().unwrap_or("thumbnail").to_owned();
let bytes = field.bytes().await?;
let item = self
    .media
    .add(course.model(), "thumbnail", MediaSource::bytes(file_name, bytes)) // course: Record<Course>
    .property("alt", course.title.clone())
    .save(&uow)
    .await?;
Ok(Json(self.media.resource(&item).await?))
```

   Request bodies are capped by `http.body_limit_bytes` (2 MiB) before the
   handler runs: raise it, or use storage's direct uploads and
   `MediaSource::disk(disk, path)`.

## What `save` does (in this order; any failure keeps nothing)

1. Reads the source; empty → `empty_file`, over the collection's
   `max_size` (capped by `media.max_file_size`, 25 MiB) → `payload_too_large`.
2. Sniffs the type from the content (`mime::sniff`), checks `accepts` →
   `file_type_not_allowed`. Undeclared collection on a model that declares
   some → `unknown_media_collection`; a model that declares none accepts any.
3. JPEG/PNG/GIF/WebP/BMP/SVG: reads width/height (EXIF orientation
   applied), strips EXIF/XMP/IPTC/text (`media.strip_metadata`), refuses
   undecodable files (`invalid_image`). Other image types are stored as is.
   Video/audio with `media-ffmpeg`: ffprobe fills width/height and
   `metadata.duration`/`video_codec`/`audio_codec` (a failing probe only logs).
4. Writes the original to `{prefix}/{id}/{sanitized file name}` on the
   collection's disk (`collection.disk` → `media.disk` → storage default).
5. Generates the conversions that run now (non-queued ones, or all without
   a queue), writes the row (in the `Uow`), deletes what `single_file` /
   `only_keep_latest` no longer keeps (files after commit), dispatches one
   `media.conversions` job for the queued ones on `media.queue`.

Files are written at once even inside a `Uow`; a rollback leaves the files
without a row. `save(None)` joins the current unit of work if there is one.

## Conversions

`Conversion::new("name")` (lower-case letters, digits, `-`, `_`) with
`width`, `height`, `fit(Fit::Contain|Cover|Fill, w, h)`, `upscale`,
`format(OutputFormat::Jpeg|Png|WebP)` (WebP is lossless; default keeps
JPEG/PNG/WebP, else PNG), `quality` (JPEG, default 82), `greyscale`,
`blur(sigma)`, `responsive()` (copies at each `media.images.responsive_widths`
below its width), `nonqueued()` / `queued()`, and with `media-ffmpeg`
`frame_at(secs)` (video posters) and `transcode(Transcode::mp4()|webm()|mp3()|m4a())`.

- Image conversions apply to JPEG, PNG, GIF, WebP, BMP and SVG (and video
  frames with FFmpeg); transcodes to video/audio. Others are skipped, not errors.
- Paths: `{prefix}/{id}/conversions/{stem}-{name}.{ext}`, responsive copies
  `{prefix}/{id}/responsive/{stem}-{name}-{width}.{ext}`, on the conversions disk.
- `item.conversions[name]` is a `GeneratedConversion` (path, mime_type,
  size, width, height, variants, generated_at); `item.has_conversion(name)`.
- Queued: the job `media.conversions` (`GenerateConversions { media_id,
  conversions }`) writes only the conversions back; deleted items are skipped.
- Regenerate: `navree media:regenerate [--model T] [--collection c] [--id
  uuid]... [--only a,b] [--missing] [--queue]`; in code `media.regenerate(&Regenerate { .. })`
  (now, returns `RegenerateReport`) or `media.queue_regenerate(&filter)` (one
  `media.regenerate` batch with `allow_failures`, needs `QueueModule`).

## Reading, presenting, deleting

- `media.get(&model, "c")`, `first`, `all`, `find`, `find_or_fail` (404
  `media_not_found`), `list(&MediaQuery { model_type, model_id, collection,
  ids, after, limit })`. Query the table directly with
  `MediaRecord::query().where_(MediaRecord::COLLECTION.eq("x"))`.
- `media.resource(&item)` → `MediaResource` JSON (`__type: "Media"`, url,
  conversions, srcsets, custom_properties, ...): public URLs on disks with
  `public_url`, temporary ones (`media.url_ttl_secs`) otherwise. Also `url`,
  `conversion_url`, `srcset` (public only, `NoPublicUrl` otherwise),
  `temporary_url`, `temporary_conversion_url`, `read`, `read_conversion`.
- `media.save(&uow, &item)` writes only name/custom_properties/order;
  `reorder(&uow, &ids)`.
- `delete(&uow, id)`, `clear(&uow, &model, c)`, `delete_all(&uow, &model)`:
  rows in the unit of work, files after commit.
- Deleting the model deletes its media when it is registered with
  `for_feature`, names `MEDIA_TABLE`, the delete records change events
  (record `delete`, `Model::destroy`; NOT query-builder bulk
  deletes) and the row has no `deleted_at` column (soft-deleting models keep
  their media: call `delete_all` on force delete). `media.delete_with_model = false` turns it off.

## Remote files, archives, text, video

- `MediaSource::url(url)` / `media.add_from_url(..)`: through the app's
  `HttpClient` SSRF guard (private/loopback/metadata addresses and redirects
  to them → `remote_file_blocked`), http(s) only, capped by size and
  `media.download_timeout_secs` (`remote_file_unavailable`). Tests on a local
  mock server need `SsrfPolicy { allow_hosts: vec!["127.0.0.1".into()], .. }`
  on the client passed to `Media::builder(..).http_client(..)`.
- `archive::Archive::open(bytes, &ArchiveLimits)`: refuses zip slip,
  symlinks, encrypted entries, duplicates and bombs (`unsafe_archive`,
  `archive_too_large`); `entries`, `files`, `contains`, `read`,
  `extract_to(&disk, prefix)`. `media.open_archive(&item)`,
  `media.unpack(&item, disk, prefix)` (SCORM), `media.zip(&items)`,
  `archive::ZipBuilder`.
- `text::extract(bytes, mime)` / `media.extract_text(&item)` →
  `ExtractedText { text, pages, title, mime_type }` for PDF (per page),
  DOCX, HTML/XHTML, `text/*`; `unsupported_media_type` (415) otherwise. No OCR.
- `media-ffmpeg`: `media.ffmpeg()` / `video::Ffmpeg::new(FfmpegOptions)` with
  `probe`, `frame`, `transcode`, `version`; `media.ffmpeg.{ffmpeg, ffprobe,
  timeout_secs, threads}`. Keep transcodes queued.

## Testing

```rust
let fake = MediaFake::new().register::<Course>(); // memory disk `media`, memory rows, conversions at once
let app = TestApp::builder()
    .temp_database()
    .module(DatabaseModule::from_config())
    .module(CoursesModule)
    .override_value(Arc::new(fake.media()))
    .build()
    .await;
// ... exercise the route ...
fake.assert_added("Course", "thumbnail");
fake.assert_added_with(|item| item.has_conversion("card"));
fake.assert_deleted(id);
```

- `fake.http().on(Method::GET, "https://cdn.example.com/*", FakeResponse::new(200).body(png))`
  stubs remote files; `fake.ffmpeg()` is an `FfmpegFake` (`probe_json`,
  `fail`, `assert_ran`). To fake only FFmpeg in a real app:
  `.override_value::<dyn FfmpegRunner>(FfmpegFake::new().runner())`.
- With the real module: `TestApp` + `StorageModule::for_root` with a
  `DiskConfig::Memory` disk + `config("media.store", "memory")` (or
  `temp_database()`, a SQLite file: the test build needs the `sqlite` feature
  in `[dev-dependencies]`). A `QueueFake` dispatcher records `GenerateConversions`; run it
  with `job.handle(JobContext { app: app.ctx().clone(), job_id, attempt: 1 })`.
- Outside an app: `Media::builder(Arc<Storage>).options(..).register::<M>().build()?`
  (memory rows, conversions at once, real HTTP client with the default SSRF policy).

## Gotchas

- `media.add(&record, ..)` with a `Record<Course>` does not compile: pass
  `record.model()` (the trait is on `Course`).
- `MEDIA_TYPE` is stored on every row; renaming it orphans rows.
- Register every model that has conversions with `for_feature`: adding a
  file registers its model only in that process, so a separate queue worker
  (or `media:regenerate`) that never added one fails the job with
  `unknown_media_collection`.
- `srcset`/`url` need a disk with `public_url`; `resource` falls back to
  temporary URLs, which on local/memory disks are served by storage's route.
- A conversion that runs at once and fails fails the whole add (500 for
  FFmpeg errors). Queued failures retry 3 times.
- Image decoding is CPU-bound; `images::*` called directly in a handler
  belongs in `spawn_blocking` (`Media` does this itself).
