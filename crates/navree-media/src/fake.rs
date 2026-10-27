//! [`MediaFake`]: media for tests, on memory disks and rows, recorded.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use navree_http_client::{FakeServer, HttpClient};
use navree_storage::{DiskConfig, Storage, StorageOptions};
use uuid::Uuid;

use crate::collections::HasMedia;
#[cfg(feature = "ffmpeg")]
use crate::video::FfmpegFake;
use crate::{Media, MediaItem, MediaOptions};

/// What a fake saw: added items and deleted ids.
#[derive(Debug, Default)]
pub(crate) struct Recorder {
    inner: Mutex<Recorded>,
}

#[derive(Debug, Default)]
struct Recorded {
    added: Vec<MediaItem>,
    deleted: Vec<Uuid>,
}

impl Recorder {
    fn lock(&self) -> MutexGuard<'_, Recorded> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn added(&self, item: &MediaItem) {
        self.lock().added.push(item.clone());
    }

    pub(crate) fn deleted(&self, ids: &[Uuid]) {
        self.lock().deleted.extend_from_slice(ids);
    }
}

/// Media for tests: the real service on a memory disk (`media`, the
/// default) and memory rows, with every conversion generated at once
/// (nothing is queued), remote downloads answered by a fake HTTP server,
/// FFmpeg answered by a `video::FfmpegFake` (the
/// `ffmpeg` feature), and every add and delete recorded.
///
/// ```
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// use navree_media::{HasMedia, MediaFake, MediaSource};
///
/// struct Course;
/// impl HasMedia for Course {
///     const MEDIA_TYPE: &'static str = "Course";
///     fn media_key(&self) -> String {
///         "7".into()
///     }
/// }
///
/// let fake = MediaFake::new();
/// let media = fake.media(); // install with TestApp::builder().override_value(Arc::new(media))
/// let item = media
///     .add(&Course, "handouts", MediaSource::bytes("notes.txt", "Bring a pen."))
///     .save(None)
///     .await
///     .unwrap();
/// assert_eq!(item.mime_type, "text/plain");
/// fake.assert_added("Course", "handouts");
/// assert!(fake.storage().default_disk().exists(&item.path).await.unwrap());
///
/// media.delete(None, item.id).await.unwrap();
/// fake.assert_deleted(item.id);
/// # });
/// ```
#[derive(Clone)]
pub struct MediaFake {
    media: Media,
    storage: Arc<Storage>,
    http: Arc<FakeServer>,
    recorder: Arc<Recorder>,
    #[cfg(feature = "ffmpeg")]
    ffmpeg: FfmpegFake,
}

impl std::fmt::Debug for MediaFake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaFake").finish_non_exhaustive()
    }
}

impl Default for MediaFake {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaFake {
    /// A fake with the default options.
    pub fn new() -> Self {
        Self::with_options(MediaOptions::default())
    }

    /// A fake with `options` (its disk names are ignored: every file goes
    /// to the memory disk `media`).
    ///
    /// # Panics
    ///
    /// When the options are invalid.
    pub fn with_options(mut options: MediaOptions) -> Self {
        options.disk = None;
        options.conversions_disk = None;
        let storage_options = StorageOptions {
            default: "media".into(),
            disks: [("media".into(), DiskConfig::Memory)].into_iter().collect(),
            ..StorageOptions::default()
        };
        let storage = Arc::new(
            Storage::new(&storage_options, b"navree media fake signing key")
                .expect("a memory disk"),
        );
        let (client, server) = HttpClient::fake();
        let recorder = Arc::new(Recorder::default());
        #[cfg(feature = "ffmpeg")]
        let ffmpeg = FfmpegFake::new();
        let builder = Media::builder(storage.clone())
            .options(options)
            .http_client(client)
            .recorder(recorder.clone());
        #[cfg(feature = "ffmpeg")]
        let builder = builder.ffmpeg_runner(ffmpeg.runner());
        let media = builder.build().expect("valid media options");
        Self {
            media,
            storage,
            http: Arc::new(server),
            recorder,
            #[cfg(feature = "ffmpeg")]
            ffmpeg,
        }
    }

    /// Registers `M`'s collections up front (adding a file registers its
    /// model anyway).
    ///
    /// # Panics
    ///
    /// When a collection or conversion name is invalid.
    pub fn register<M: HasMedia>(self) -> Self {
        self.media.register::<M>().expect("valid media collections");
        self
    }

    /// The [`Media`] backed by this fake. Install it with
    /// `TestApp::builder().override_value(Arc::new(fake.media()))`; the
    /// fake keeps recording through every clone.
    pub fn media(&self) -> Media {
        self.media.clone()
    }

    /// The storage the files are on (one memory disk, `media`).
    pub fn storage(&self) -> Arc<Storage> {
        self.storage.clone()
    }

    /// The fake HTTP server remote files are downloaded from: stub a URL
    /// with `fake.http().on(Method::GET, "https://cdn.example.com/*", ...)`.
    /// Unstubbed URLs fail as unreachable.
    pub fn http(&self) -> &FakeServer {
        &self.http
    }

    /// The FFmpeg fake that answers probes, frames and transcodes.
    #[cfg(feature = "ffmpeg")]
    pub fn ffmpeg(&self) -> &FfmpegFake {
        &self.ffmpeg
    }

    /// Every item added, in order.
    pub fn added(&self) -> Vec<MediaItem> {
        self.recorder.lock().added.clone()
    }

    /// Every deleted id, in order.
    pub fn deleted(&self) -> Vec<Uuid> {
        self.recorder.lock().deleted.clone()
    }

    /// Asserts a file was added to `model_type`'s `collection`.
    ///
    /// # Panics
    ///
    /// When none was.
    pub fn assert_added(&self, model_type: &str, collection: &str) {
        self.assert_added_with(|item| {
            item.model_type == model_type && item.collection == collection
        });
    }

    /// Asserts an added item matches `predicate`.
    ///
    /// # Panics
    ///
    /// When none does.
    pub fn assert_added_with(&self, predicate: impl Fn(&MediaItem) -> bool) {
        let added = self.added();
        assert!(
            added.iter().any(predicate),
            "no added media matched; added: {:?}",
            added
                .iter()
                .map(|item| format!(
                    "{}/{}: {}",
                    item.model_type, item.collection, item.file_name
                ))
                .collect::<Vec<_>>()
        );
    }

    /// Asserts `count` files were added.
    ///
    /// # Panics
    ///
    /// When another number was.
    pub fn assert_added_count(&self, count: usize) {
        let added = self.added().len();
        assert_eq!(added, count, "expected {count} added media, found {added}");
    }

    /// Asserts nothing was added.
    ///
    /// # Panics
    ///
    /// When something was.
    pub fn assert_nothing_added(&self) {
        self.assert_added_count(0);
    }

    /// Asserts the item `id` was deleted.
    ///
    /// # Panics
    ///
    /// When it was not.
    pub fn assert_deleted(&self, id: Uuid) {
        assert!(
            self.deleted().contains(&id),
            "media {id} was not deleted; deleted: {:?}",
            self.deleted()
        );
    }
}
