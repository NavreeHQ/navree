//! `MediaModule` in an app (memory rows): configuration, queued
//! conversions and the job, batches, error responses, the command, and
//! the FFmpeg runner an app provides.

use std::sync::Arc;

use bytes::Bytes;
use http::Method;
use image::ImageFormat;
use navree_contracts::{Job, JobContext};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_media::video::{FfmpegFake, FfmpegRunner};
use navree_media::{
    GenerateConversions, Media, MediaModule, MediaOptions, MediaSource, Regenerate, StoreKind,
};
use navree_queue::QueueFake;
use navree_storage::{DiskConfig, StorageModule, StorageOptions};
use navree_testing::{TestApp, TestAppBuilder};

use crate::models::Course;
use crate::support::{image_bytes, pdf};

fn storage() -> StorageOptions {
    let mut options = StorageOptions {
        default: "media".into(),
        ..StorageOptions::default()
    };
    options.disks.clear();
    options.disks.insert("media".into(), DiskConfig::Memory);
    options
}

/// Routes that attach uploads to course 7.
struct UploadController(Arc<Media>);

impl Injectable for UploadController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(UploadController(r.get::<Media>().await?)) })
    }
}

impl Controller for UploadController {
    fn prefix(&self) -> &'static str {
        "!/courses/7"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let upload = self.clone();
        vec![
            RouteDef::on(
                Method::POST,
                "/{collection}",
                move |navree_http::Path(collection): navree_http::Path<String>, body: Bytes| {
                    let this = upload.clone();
                    async move {
                        let item = this
                            .0
                            .add(
                                &Course { id: 7 },
                                &collection,
                                MediaSource::bytes("upload", body),
                            )
                            .save(None)
                            .await?;
                        Ok::<_, AppError>(Json(this.0.resource(&item).await?))
                    }
                },
            )
            .public(),
        ]
    }
}

struct UploadModule;

impl Module for UploadModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<UploadController>();
    }
}

fn app() -> TestAppBuilder {
    TestApp::builder()
        .module(StorageModule::for_root(storage()))
        .config("media.store", "memory")
        .module(MediaModule::from_config())
        .module(MediaModule::for_feature::<Course>())
        .module(UploadModule)
}

#[tokio::test]
async fn the_module_provides_media_from_config() {
    let app = app()
        .config("media.prefix", "uploads")
        .config("media.max_file_size", 2048)
        .build()
        .await;
    let media = app.get::<Media>().await;
    assert_eq!(media.options().store, StoreKind::Memory);
    assert_eq!(media.options().prefix, "uploads");
    let options = app.get::<MediaOptions>().await;
    assert_eq!(options.max_file_size, 2048);
    let item = media
        .add(
            &Course { id: 1 },
            "attachments",
            MediaSource::bytes("a.txt", "hello"),
        )
        .save(None)
        .await
        .unwrap();
    assert!(item.path.starts_with("uploads/"));
}

#[tokio::test]
async fn invalid_options_and_disks_fail_the_boot() {
    let error = TestApp::builder()
        .module(StorageModule::for_root(storage()))
        .config("media.store", "memory")
        .config("media.prefix", "../x")
        .module(MediaModule::from_config())
        .try_build()
        .await
        .map(|_| ())
        .expect_err("boot fails");
    assert!(error.to_string().contains("media.prefix"), "{error}");

    let error = TestApp::builder()
        .module(StorageModule::for_root(storage()))
        .module(MediaModule::for_root(MediaOptions {
            store: StoreKind::Memory,
            disk: Some("s3".into()),
            ..MediaOptions::default()
        }))
        .try_build()
        .await
        .map(|_| ())
        .expect_err("boot fails");
    assert!(
        error
            .to_string()
            .contains("`s3` is not a configured storage disk"),
        "{error}"
    );

    let error = TestApp::builder()
        .module(StorageModule::for_root(storage()))
        .module(MediaModule::from_config())
        .try_build()
        .await
        .map(|_| ())
        .expect_err("boot fails");
    assert!(
        error.to_string().contains("needs DatabaseModule"),
        "{error}"
    );
}

#[tokio::test]
async fn conversions_are_queued_and_the_job_generates_them() {
    let queue = QueueFake::new();
    let app = app()
        .config("media.queue", "media")
        .override_value(queue.dispatcher())
        .build()
        .await;
    let media = app.get::<Media>().await;
    let item = media
        .add(
            &Course { id: 7 },
            "gallery",
            MediaSource::bytes("view.png", image_bytes(400, 200, ImageFormat::Png)),
        )
        .save(None)
        .await
        .unwrap();
    assert!(item.conversions.is_empty());
    let envelopes = queue.envelopes();
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].job_type, "media.conversions");
    assert_eq!(envelopes[0].options.queue, "media");
    let job = queue.dispatched::<GenerateConversions>().remove(0);
    assert_eq!(job.media_id, item.id);
    assert_eq!(job.conversions, vec!["hero", "thumb"]);

    // A text file gets no image conversion: nothing more is queued.
    media
        .add(
            &Course { id: 7 },
            "attachments",
            MediaSource::bytes("a.txt", "x"),
        )
        .save(None)
        .await
        .unwrap();
    assert_eq!(queue.envelopes().len(), 1);

    let ctx = JobContext {
        app: app.ctx().clone(),
        job_id: "1".into(),
        attempt: 1,
    };
    job.clone().handle(ctx.clone()).await.unwrap();
    let converted = media.find_or_fail(item.id).await.unwrap();
    assert_eq!(converted.conversions["thumb"].width, Some(100));
    assert_eq!(converted.conversions["hero"].width, Some(400));

    // An item deleted before its job runs is skipped.
    media.delete(None, item.id).await.unwrap();
    job.handle(ctx).await.unwrap();
}

#[tokio::test]
async fn nonqueued_conversions_and_queue_conversions_off_run_at_once() {
    let queue = QueueFake::new();
    let app = app()
        .config("media.queue_conversions", false)
        .override_value(queue.dispatcher())
        .build()
        .await;
    let item = app
        .get::<Media>()
        .await
        .add(
            &Course { id: 7 },
            "thumbnail",
            MediaSource::bytes("c.png", image_bytes(100, 100, ImageFormat::Png)),
        )
        .save(None)
        .await
        .unwrap();
    assert!(item.conversions.contains_key("card"));
    queue.assert_nothing_dispatched();
}

#[tokio::test]
async fn regeneration_is_batched_on_the_queue() {
    let queue = QueueFake::new();
    let app = app()
        .config("media.queue", "media")
        .config("media.queue_conversions", false)
        .override_value(Arc::new(queue.queue()))
        .build()
        .await;
    let media = app.get::<Media>().await;
    for id in [1, 2] {
        media
            .add(
                &Course { id },
                "thumbnail",
                MediaSource::bytes("c.png", image_bytes(100, 100, ImageFormat::Png)),
            )
            .save(None)
            .await
            .unwrap();
    }
    let batch = media
        .queue_regenerate(&Regenerate::default())
        .await
        .unwrap()
        .expect("a batch");
    assert_eq!(batch.total_jobs, 2);
    let batches = queue.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].name(), "media.regenerate");
    let jobs = batches[0].jobs_of::<GenerateConversions>();
    assert_eq!(jobs.len(), 2);
    assert!(jobs.iter().all(|job| job.conversions == vec!["card"]));
    assert!(
        queue
            .envelopes()
            .iter()
            .chain(batches[0].jobs.iter())
            .all(|job| job.options.queue == "media")
    );
    assert!(
        media
            .queue_regenerate(&Regenerate {
                model_type: Some("Nobody".into()),
                ..Regenerate::default()
            })
            .await
            .unwrap()
            .is_none()
    );

    app.run_command("media:regenerate", &["--queue", "--only", "card"])
        .await
        .unwrap();
    assert_eq!(queue.batches().len(), 2);
    app.run_command("media:regenerate", &["--model", "Course", "--missing"])
        .await
        .unwrap();
    let error = app
        .run_command("media:regenerate", &["--bogus"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("unexpected argument"), "{error}");
}

#[tokio::test]
async fn refused_files_are_error_responses() {
    let app = app().config("media.max_file_size", 4096).build().await;
    let client = app.client();
    let response = client
        .post("/courses/7/thumbnail")
        .body(image_bytes(40, 30, ImageFormat::Png))
        .send()
        .await
        .assert_status(200);
    let body = response.json_value();
    assert_eq!(body["__type"], "Media");
    assert_eq!(body["width"], 40);
    assert_eq!(body["mime_type"], "image/png");

    client
        .post("/courses/7/thumbnail")
        .body(pdf(&["x"]))
        .send()
        .await
        .assert_error_code(400, "file_type_not_allowed");
    client
        .post("/courses/7/attachments")
        .body(vec![b'x'; 5000])
        .send()
        .await
        .assert_error_code(413, "payload_too_large");
    client
        .post("/courses/7/nowhere")
        .body("x")
        .send()
        .await
        .assert_error_code(400, "unknown_media_collection");
    client
        .post("/courses/7/attachments")
        .body(Vec::new())
        .send()
        .await
        .assert_error_code(400, "empty_file");
}

#[tokio::test]
async fn the_apps_ffmpeg_runner_is_used() {
    let ffmpeg = FfmpegFake::new();
    let app = app()
        .override_value::<dyn FfmpegRunner>(ffmpeg.runner())
        .build()
        .await;
    let media = app.get::<Media>().await;
    assert_eq!(
        media.ffmpeg().version().await.unwrap(),
        "ffmpeg version navree-fake"
    );
    ffmpeg.assert_ran(|call| call.args.contains(&"-version".to_owned()));
}

#[tokio::test]
async fn remote_files_go_through_the_apps_http_client() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/logo.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(image_bytes(
            30,
            30,
            ImageFormat::Png,
        )))
        .mount(&server)
        .await;
    let url = format!("{}/logo.png", server.uri());

    // The app's client allows the mock server's address.
    let allowed = app()
        .config("http_client.ssrf.allow_hosts", "127.0.0.1")
        .module(navree_http_client::HttpClientModule::from_config())
        .build()
        .await;
    let item = allowed
        .get::<Media>()
        .await
        .add_from_url(&Course { id: 7 }, "gallery", url.as_str())
        .save(None)
        .await
        .unwrap();
    assert_eq!(item.width, Some(30));

    // Without HttpClientModule, a client with the default policy refuses it.
    let default = app().build().await;
    let error = default
        .get::<Media>()
        .await
        .add_from_url(&Course { id: 7 }, "gallery", url.as_str())
        .save(None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "remote_file_blocked", "{error}");
}
