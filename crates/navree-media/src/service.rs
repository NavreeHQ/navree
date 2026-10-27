//! [`Media`]: attach, list, convert, present and delete files.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use navree_contracts::{Dispatcher, JobEnvelope};
use navree_db::Uow;
use navree_db::sea_orm::DatabaseTransaction;
use navree_http_client::{HttpClient, HttpClientOptions};
use navree_kernel::WeakAppContext;
use navree_storage::{Disk, Storage, StorageError};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::archive::{Archive, ExtractReport, ZipBuilder};
use crate::collections::{HasMedia, MediaCollections};
use crate::conversion::Conversion;
use crate::fake::Recorder;
use crate::generate;
use crate::jobs::GenerateConversions;
use crate::source::{Loader, MediaSource, sanitize_file_name};
use crate::store::{MediaQuery, MediaStore, MemoryStore};
use crate::text::{self, ExtractedText};
#[cfg(feature = "ffmpeg")]
use crate::video::{Ffmpeg, FfmpegRunner};
use crate::{MediaError, MediaItem, MediaOptions, MediaResource, images, mime};

/// A model type's collections, as registered.
#[derive(Clone, Debug)]
pub(crate) struct Registered {
    pub(crate) collections: Arc<MediaCollections>,
    pub(crate) table: &'static str,
}

/// Files attached to models: add them from uploads, URLs, disks or local
/// files; list, present, convert and delete them. Provided by
/// [`MediaModule`](crate::MediaModule) (resolve `Arc<Media>`); cheap to
/// clone.
///
/// ```no_run
/// use navree_db::Uow;
/// use navree_media::{HasMedia, Media, MediaSource};
/// # struct Course { id: uuid::Uuid }
/// # impl HasMedia for Course {
/// #     const MEDIA_TYPE: &'static str = "Course";
/// #     fn media_key(&self) -> String { self.id.to_string() }
/// # }
///
/// # async fn example(media: &Media, uow: &Uow, course: &Course, bytes: Vec<u8>) -> Result<(), navree_media::MediaError> {
/// let item = media
///     .add(course, "thumbnail", MediaSource::bytes("cover.png", bytes))
///     .property("alt", "The course cover")
///     .save(uow)
///     .await?;
/// let resource = media.resource(&item).await?; // JSON with URLs
/// let thumbnails = media.get(course, "thumbnail").await?;
/// # let _ = (resource, thumbnails);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Media {
    inner: Arc<Inner>,
}

struct Inner {
    options: MediaOptions,
    storage: Arc<Storage>,
    store: Arc<dyn MediaStore>,
    registry: RwLock<HashMap<String, Registered>>,
    app: Option<WeakAppContext>,
    http: OnceLock<HttpClient>,
    no_dispatcher: bool,
    #[cfg(feature = "ffmpeg")]
    ffmpeg: Ffmpeg,
    recorder: Option<Arc<Recorder>>,
}

impl std::fmt::Debug for Media {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Media")
            .field("options", &self.inner.options)
            .finish_non_exhaustive()
    }
}

/// Builds a [`Media`] outside an app (tools, tests); in an app,
/// [`MediaModule`](crate::MediaModule) builds it.
pub struct MediaBuilder {
    options: MediaOptions,
    storage: Arc<Storage>,
    store: Option<Arc<dyn MediaStore>>,
    http: Option<HttpClient>,
    app: Option<WeakAppContext>,
    no_dispatcher: bool,
    #[cfg(feature = "ffmpeg")]
    ffmpeg_runner: Option<Arc<dyn FfmpegRunner>>,
    recorder: Option<Arc<Recorder>>,
    registrations: Vec<(String, Registered)>,
}

impl std::fmt::Debug for MediaBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaBuilder")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl MediaBuilder {
    /// The options (default [`MediaOptions::default`]).
    pub fn options(mut self, options: MediaOptions) -> Self {
        self.options = options;
        self
    }

    /// Where rows are kept (default a [`MemoryStore`]).
    pub fn store(mut self, store: Arc<dyn MediaStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// The client remote files are downloaded with (default one with the
    /// default SSRF policy).
    pub fn http_client(mut self, client: HttpClient) -> Self {
        self.http = Some(client);
        self
    }

    /// The FFmpeg runner (default the real tools, configured by
    /// `options.ffmpeg`).
    #[cfg(feature = "ffmpeg")]
    pub fn ffmpeg_runner(mut self, runner: Arc<dyn FfmpegRunner>) -> Self {
        self.ffmpeg_runner = Some(runner);
        self
    }

    /// Registers `M`'s collections.
    pub fn register<M: HasMedia>(mut self) -> Self {
        self.registrations.push(registration::<M>());
        self
    }

    pub(crate) fn app(mut self, app: WeakAppContext) -> Self {
        self.app = Some(app);
        self
    }

    pub(crate) fn recorder(mut self, recorder: Arc<Recorder>) -> Self {
        self.recorder = Some(recorder);
        self.no_dispatcher = true;
        self
    }

    pub(crate) fn registered(mut self, model_type: String, registered: Registered) -> Self {
        self.registrations.push((model_type, registered));
        self
    }

    /// The service; fails on invalid options or collections.
    pub fn build(self) -> Result<Media, MediaError> {
        self.options.check().map_err(invalid_options)?;
        let mut registry = HashMap::new();
        for (model_type, registered) in self.registrations {
            registered
                .collections
                .check(&model_type)
                .map_err(invalid_options)?;
            registry.insert(model_type, registered);
        }
        let http = OnceLock::new();
        if let Some(client) = self.http {
            let _ = http.set(client);
        }
        #[cfg(feature = "ffmpeg")]
        let ffmpeg = match self.ffmpeg_runner {
            Some(runner) => Ffmpeg::with_runner(self.options.ffmpeg.clone(), runner),
            None => Ffmpeg::new(self.options.ffmpeg.clone()),
        };
        Ok(Media {
            inner: Arc::new(Inner {
                store: self.store.unwrap_or_else(|| Arc::new(MemoryStore::new())),
                options: self.options,
                storage: self.storage,
                registry: RwLock::new(registry),
                app: self.app,
                http,
                no_dispatcher: self.no_dispatcher,
                #[cfg(feature = "ffmpeg")]
                ffmpeg,
                recorder: self.recorder,
            }),
        })
    }
}

fn invalid_options(message: String) -> MediaError {
    MediaError::Storage(StorageError::InvalidOptions(message))
}

pub(crate) fn registration<M: HasMedia>() -> (String, Registered) {
    (
        M::MEDIA_TYPE.to_owned(),
        Registered {
            collections: Arc::new(MediaCollections::of::<M>()),
            table: M::MEDIA_TABLE,
        },
    )
}

/// Which items [`Media::regenerate`] converts again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Regenerate {
    /// Only this model type.
    pub model_type: Option<String>,
    /// Only this collection.
    pub collection: Option<String>,
    /// Only these items.
    pub ids: Vec<Uuid>,
    /// Only these conversions (all by default).
    pub conversions: Vec<String>,
    /// Only conversions not generated yet.
    pub missing_only: bool,
}

/// What [`Media::regenerate`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct RegenerateReport {
    /// Items converted.
    pub media: u64,
    /// Conversions generated.
    pub conversions: u64,
    /// Items that failed, with the error.
    pub failed: Vec<(Uuid, String)>,
}

impl Media {
    /// A builder for a service on `storage`'s disks.
    pub fn builder(storage: Arc<Storage>) -> MediaBuilder {
        MediaBuilder {
            options: MediaOptions::default(),
            storage,
            store: None,
            http: None,
            app: None,
            no_dispatcher: false,
            #[cfg(feature = "ffmpeg")]
            ffmpeg_runner: None,
            recorder: None,
            registrations: Vec::new(),
        }
    }

    /// The options.
    pub fn options(&self) -> &MediaOptions {
        &self.inner.options
    }

    /// The storage the files live in.
    pub fn storage(&self) -> &Storage {
        &self.inner.storage
    }

    /// The row store.
    pub fn store(&self) -> &Arc<dyn MediaStore> {
        &self.inner.store
    }

    /// The FFmpeg sidecar (the `ffmpeg` feature).
    #[cfg(feature = "ffmpeg")]
    pub fn ffmpeg(&self) -> &Ffmpeg {
        &self.inner.ffmpeg
    }

    pub(crate) fn recorder(&self) -> Option<&Arc<Recorder>> {
        self.inner.recorder.as_ref()
    }

    /// Registers `M`'s collections (done for you by `add` and
    /// [`MediaModule::for_feature`](crate::MediaModule::for_feature)).
    pub fn register<M: HasMedia>(&self) -> Result<Arc<MediaCollections>, MediaError> {
        if let Some(registered) = self.registered(M::MEDIA_TYPE) {
            return Ok(registered.collections);
        }
        let (model_type, registered) = registration::<M>();
        registered
            .collections
            .check(&model_type)
            .map_err(invalid_options)?;
        let collections = registered.collections.clone();
        self.inner
            .registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(model_type, registered);
        Ok(collections)
    }

    pub(crate) fn registered(&self, model_type: &str) -> Option<Registered> {
        self.inner
            .registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(model_type)
            .cloned()
    }

    /// Starts attaching a file to `model` in `collection`; finish with
    /// [`PendingMedia::save`].
    pub fn add<M: HasMedia>(&self, model: &M, collection: &str, source: MediaSource) -> PendingMedia {
        PendingMedia {
            media: self.clone(),
            collections: self.register::<M>(),
            model_type: M::MEDIA_TYPE.to_owned(),
            model_id: model.media_key(),
            collection: collection.to_owned(),
            source,
            name: None,
            file_name: None,
            properties: Map::new(),
        }
    }

    /// Starts attaching a remote file (see [`MediaSource::url`]).
    pub fn add_from_url<M: HasMedia>(
        &self,
        model: &M,
        collection: &str,
        url: impl Into<String>,
    ) -> PendingMedia {
        self.add(model, collection, MediaSource::url(url))
    }

    /// `model`'s files in `collection`, in order.
    pub async fn get<M: HasMedia>(
        &self,
        model: &M,
        collection: &str,
    ) -> Result<Vec<MediaItem>, MediaError> {
        self.list(&MediaQuery::owner(M::MEDIA_TYPE, model.media_key()).collection(collection))
            .await
    }

    /// `model`'s first file in `collection`.
    pub async fn first<M: HasMedia>(
        &self,
        model: &M,
        collection: &str,
    ) -> Result<Option<MediaItem>, MediaError> {
        Ok(self.get(model, collection).await?.into_iter().next())
    }

    /// Every file of `model`, by collection and order.
    pub async fn all<M: HasMedia>(&self, model: &M) -> Result<Vec<MediaItem>, MediaError> {
        let mut items = self
            .list(&MediaQuery::owner(M::MEDIA_TYPE, model.media_key()))
            .await?;
        items.sort_by(|a, b| {
            a.collection
                .cmp(&b.collection)
                .then(a.order.cmp(&b.order))
                .then(a.id.cmp(&b.id))
        });
        Ok(items)
    }

    /// The rows `query` selects.
    pub async fn list(&self, query: &MediaQuery) -> Result<Vec<MediaItem>, MediaError> {
        self.inner.store.list(None, query).await
    }

    /// The item `id`.
    pub async fn find(&self, id: Uuid) -> Result<Option<MediaItem>, MediaError> {
        self.inner.store.find(id).await
    }

    /// The item `id`, or [`MediaError::NotFound`] (404).
    pub async fn find_or_fail(&self, id: Uuid) -> Result<MediaItem, MediaError> {
        self.find(id).await?.ok_or(MediaError::NotFound { id })
    }

    /// Saves changes to an item's `name`, `custom_properties` and `order`
    /// (the files and conversions are the module's).
    pub async fn save<'u>(
        &self,
        uow: impl Into<Option<&'u Uow>>,
        item: &MediaItem,
    ) -> Result<MediaItem, MediaError> {
        let mut current = self.find_or_fail(item.id).await?;
        current.name = item.name.clone();
        current.custom_properties = item.custom_properties.clone();
        current.order = item.order;
        current.updated_at = Utc::now();
        self.inner.store.update(uow.into(), &current).await?;
        Ok(current)
    }

    /// Puts the items `ids` (of one collection) in this order, from 1.
    pub async fn reorder<'u>(
        &self,
        uow: impl Into<Option<&'u Uow>>,
        ids: &[Uuid],
    ) -> Result<(), MediaError> {
        let uow = uow.into();
        for (position, id) in ids.iter().enumerate() {
            let mut item = self.find_or_fail(*id).await?;
            item.order = i32::try_from(position + 1).unwrap_or(i32::MAX);
            item.updated_at = Utc::now();
            self.inner.store.update(uow, &item).await?;
        }
        Ok(())
    }

    /// Deletes the item `id`: its row now (in `uow`), its files once
    /// `uow` commits (at once without a unit of work). `false` when it did
    /// not exist.
    pub async fn delete<'u>(
        &self,
        uow: impl Into<Option<&'u Uow>>,
        id: Uuid,
    ) -> Result<bool, MediaError> {
        let Some(item) = self.find(id).await? else {
            return Ok(false);
        };
        Ok(self.remove(uow.into(), vec![item]).await? > 0)
    }

    /// Deletes every file of `model` in `collection`.
    pub async fn clear<'u, M: HasMedia>(
        &self,
        uow: impl Into<Option<&'u Uow>>,
        model: &M,
        collection: &str,
    ) -> Result<u64, MediaError> {
        let items = self.get(model, collection).await?;
        self.remove(uow.into(), items).await
    }

    /// Deletes every file of `model` (when the model itself is deleted).
    pub async fn delete_all<'u, M: HasMedia>(
        &self,
        uow: impl Into<Option<&'u Uow>>,
        model: &M,
    ) -> Result<u64, MediaError> {
        let items = self.all(model).await?;
        self.remove(uow.into(), items).await
    }

    async fn remove(&self, uow: Option<&Uow>, items: Vec<MediaItem>) -> Result<u64, MediaError> {
        if items.is_empty() {
            return Ok(0);
        }
        let ids: Vec<Uuid> = items.iter().map(|item| item.id).collect();
        let deleted = self.inner.store.delete(uow, &ids).await?;
        if let Some(recorder) = self.recorder() {
            recorder.deleted(&ids);
        }
        self.delete_files_later(uow, items).await;
        Ok(deleted)
    }

    /// Deletes the media of these owners (model type, key) inside `tx`, a
    /// change observer's transaction, and their files after `uow` commits.
    pub(crate) async fn delete_owners_in(
        &self,
        tx: &DatabaseTransaction,
        uow: &Uow,
        owners: &[(&str, &str)],
    ) -> Result<(), MediaError> {
        let mut removed = Vec::new();
        for (model_type, model_id) in owners {
            removed.extend(
                self.inner
                    .store
                    .delete_owner(tx, model_type, model_id)
                    .await?,
            );
        }
        if removed.is_empty() {
            return Ok(());
        }
        if let Some(recorder) = self.recorder() {
            let ids: Vec<Uuid> = removed.iter().map(|item| item.id).collect();
            recorder.deleted(&ids);
        }
        let media = self.clone();
        uow.after_commit(move || Box::pin(async move { media.delete_files(&removed).await }));
        Ok(())
    }

    /// Deletes the items' files once `uow` (or the current unit of work)
    /// commits, or at once when there is none.
    pub(crate) async fn delete_files_later(&self, uow: Option<&Uow>, items: Vec<MediaItem>) {
        let current = match uow {
            Some(uow) => Some(uow.clone()),
            None => match Uow::current() {
                Some(current) if !current.is_finished().await => Some(current),
                _ => None,
            },
        };
        match current {
            Some(uow) => {
                let media = self.clone();
                uow.after_commit(move || Box::pin(async move { media.delete_files(&items).await }));
            }
            None => self.delete_files(&items).await,
        }
    }

    /// Deletes every file under the items' directories, logging failures.
    pub(crate) async fn delete_files(&self, items: &[MediaItem]) {
        for item in items {
            let mut disks = vec![item.disk.clone()];
            if item.conversions_disk != item.disk {
                disks.push(item.conversions_disk.clone());
            }
            for disk in disks {
                if let Err(error) = self.delete_directory(&disk, item.directory()).await {
                    tracing::warn!(target: "navree::media", media = %item.id, %disk, %error, "cannot delete media files");
                }
            }
        }
    }

    async fn delete_directory(&self, disk: &str, directory: &str) -> Result<(), MediaError> {
        if directory.is_empty() {
            return Ok(());
        }
        let disk = self.inner.storage.disk(disk)?;
        for path in disk.list(directory).await? {
            disk.delete(&path).await?;
        }
        Ok(())
    }

    pub(crate) fn disk(&self, name: &str) -> Result<Disk, MediaError> {
        Ok(self.inner.storage.disk(name)?)
    }

    fn file_of<'a>(&self, item: &'a MediaItem, conversion: &str) -> Result<&'a str, MediaError> {
        item.conversions
            .get(conversion)
            .map(|generated| generated.path.as_str())
            .ok_or_else(|| MediaError::MissingConversion {
                id: item.id,
                conversion: conversion.to_owned(),
            })
    }

    fn public_url(&self, disk: &str, path: &str) -> Result<String, MediaError> {
        self.disk(disk)?.url(path).map_err(|error| match error {
            StorageError::NoPublicUrl { disk } => MediaError::NoPublicUrl { disk },
            other => MediaError::Storage(other),
        })
    }

    /// The original's public URL ([`MediaError::NoPublicUrl`] on a disk
    /// without `public_url`).
    pub fn url(&self, item: &MediaItem) -> Result<String, MediaError> {
        self.public_url(&item.disk, &item.path)
    }

    /// A conversion's public URL.
    pub fn conversion_url(&self, item: &MediaItem, conversion: &str) -> Result<String, MediaError> {
        self.public_url(&item.conversions_disk, self.file_of(item, conversion)?)
    }

    /// An expiring link to the original (provider-signed on cloud disks,
    /// served by the storage route on local and memory disks).
    pub async fn temporary_url(
        &self,
        item: &MediaItem,
        expires_at: DateTime<Utc>,
    ) -> Result<String, MediaError> {
        Ok(self
            .disk(&item.disk)?
            .temporary_url(&item.path, expires_at)
            .await?)
    }

    /// An expiring link to a conversion.
    pub async fn temporary_conversion_url(
        &self,
        item: &MediaItem,
        conversion: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<String, MediaError> {
        let path = self.file_of(item, conversion)?;
        Ok(self
            .disk(&item.conversions_disk)?
            .temporary_url(path, expires_at)
            .await?)
    }

    /// The `srcset` of a responsive conversion (`url 320w, url 640w, ...`,
    /// the conversion itself last), with public URLs.
    pub fn srcset(&self, item: &MediaItem, conversion: &str) -> Result<String, MediaError> {
        let generated = item
            .conversions
            .get(conversion)
            .ok_or_else(|| MediaError::MissingConversion {
                id: item.id,
                conversion: conversion.to_owned(),
            })?;
        let mut parts = Vec::new();
        for variant in &generated.variants {
            let url = self.public_url(&item.conversions_disk, &variant.path)?;
            parts.push(format!("{url} {}w", variant.width));
        }
        if let Some(width) = generated.width {
            let url = self.public_url(&item.conversions_disk, &generated.path)?;
            parts.push(format!("{url} {width}w"));
        }
        Ok(parts.join(", "))
    }

    /// A URL for `path`: public when the disk has one, temporary
    /// (`media.url_ttl_secs`) otherwise.
    async fn any_url(&self, disk: &str, path: &str) -> Result<String, MediaError> {
        match self.public_url(disk, path) {
            Ok(url) => Ok(url),
            Err(MediaError::NoPublicUrl { .. }) => {
                let ttl = i64::try_from(self.inner.options.url_ttl_secs).unwrap_or(i64::MAX);
                let expires = Utc::now() + chrono::Duration::seconds(ttl);
                Ok(self.disk(disk)?.temporary_url(path, expires).await?)
            }
            Err(error) => Err(error),
        }
    }

    /// The item as an API returns it: URLs for the original and every
    /// generated conversion, `srcset`s for responsive ones; public URLs
    /// on disks that have one, temporary ones (`media.url_ttl_secs`)
    /// otherwise.
    pub async fn resource(&self, item: &MediaItem) -> Result<MediaResource, MediaError> {
        let mut conversions = BTreeMap::new();
        let mut srcsets = BTreeMap::new();
        for (name, generated) in &item.conversions {
            let url = self.any_url(&item.conversions_disk, &generated.path).await?;
            if !generated.variants.is_empty() {
                let mut parts = Vec::new();
                for variant in &generated.variants {
                    let variant_url = self.any_url(&item.conversions_disk, &variant.path).await?;
                    parts.push(format!("{variant_url} {}w", variant.width));
                }
                if let Some(width) = generated.width {
                    parts.push(format!("{url} {width}w"));
                }
                srcsets.insert(name.clone(), parts.join(", "));
            }
            conversions.insert(name.clone(), url);
        }
        Ok(MediaResource {
            kind: "Media".to_owned(),
            id: item.id,
            collection: item.collection.clone(),
            name: item.name.clone(),
            file_name: item.file_name.clone(),
            mime_type: item.mime_type.clone(),
            size: item.size,
            width: item.width,
            height: item.height,
            url: self.any_url(&item.disk, &item.path).await?,
            conversions,
            srcsets,
            custom_properties: item.custom_properties.clone(),
            order: item.order,
            created_at: item.created_at,
        })
    }

    /// The original's contents.
    pub async fn read(&self, item: &MediaItem) -> Result<Bytes, MediaError> {
        Ok(self.disk(&item.disk)?.get(&item.path).await?)
    }

    /// A conversion's contents.
    pub async fn read_conversion(
        &self,
        item: &MediaItem,
        conversion: &str,
    ) -> Result<Bytes, MediaError> {
        let path = self.file_of(item, conversion)?;
        Ok(self.disk(&item.conversions_disk)?.get(path).await?)
    }

    /// The text of a PDF, DOCX, HTML or text file (see [`text::extract`]).
    pub async fn extract_text(&self, item: &MediaItem) -> Result<ExtractedText, MediaError> {
        let bytes = self.read(item).await?;
        let mime_type = item.mime_type.clone();
        blocking(move || text::extract(&bytes, &mime_type)).await
    }

    /// The item's zip archive, checked against `media.archives`.
    pub async fn open_archive(&self, item: &MediaItem) -> Result<Archive, MediaError> {
        let bytes = self.read(item).await?;
        let limits = self.inner.options.archives.clone();
        blocking(move || Archive::open(bytes, &limits).map_err(MediaError::from)).await
    }

    /// Unpacks the item's zip archive under `prefix` on the disk `disk`
    /// (a SCORM package served from its own directory).
    pub async fn unpack(
        &self,
        item: &MediaItem,
        disk: &str,
        prefix: &str,
    ) -> Result<ExtractReport, MediaError> {
        let disk = self.disk(disk)?;
        self.open_archive(item).await?.extract_to(&disk, prefix).await
    }

    /// One zip archive of the items' originals (duplicate names get ` (2)`,
    /// ` (3)`, ...).
    pub async fn zip(&self, items: &[MediaItem]) -> Result<Bytes, MediaError> {
        let mut zip = ZipBuilder::new();
        let mut names = HashSet::new();
        for item in items {
            let bytes = self.read(item).await?;
            let mut name = item.file_name.clone();
            let mut n = 1;
            while !names.insert(name.clone()) {
                n += 1;
                name = match item.file_name.rsplit_once('.') {
                    Some((stem, ext)) => format!("{stem} ({n}).{ext}"),
                    None => format!("{} ({n})", item.file_name),
                };
            }
            zip.file(&name, &bytes)?;
        }
        zip.finish()
    }

    /// Generates the item's conversions now (all of them, or `only` these)
    /// and saves them; what the queued job runs.
    pub async fn convert(&self, id: Uuid, only: Option<&[String]>) -> Result<MediaItem, MediaError> {
        let item = self.find_or_fail(id).await?;
        let conversions = self.conversions_of(&item, only, false)?;
        if conversions.is_empty() {
            return Ok(item);
        }
        let generated = self.generate(item, &conversions, None).await?;
        self.save_generated(&generated, &conversions).await
    }

    /// Saves the generated `conversions` of `generated` onto the stored
    /// row, which may have changed while they were generated (a new name,
    /// other properties): only the conversions are written.
    async fn save_generated(
        &self,
        generated: &MediaItem,
        conversions: &[Conversion],
    ) -> Result<MediaItem, MediaError> {
        let mut current = self.find_or_fail(generated.id).await?;
        for conversion in conversions {
            if let Some(done) = generated.conversions.get(&conversion.name) {
                current
                    .conversions
                    .insert(conversion.name.clone(), done.clone());
            }
        }
        current.updated_at = generated.updated_at;
        self.inner.store.update(None, &current).await?;
        Ok(current)
    }

    /// The conversions the item's collection declares (`only` these;
    /// with `missing_only`, those not generated yet).
    pub(crate) fn conversions_of(
        &self,
        item: &MediaItem,
        only: Option<&[String]>,
        missing_only: bool,
    ) -> Result<Vec<Conversion>, MediaError> {
        let registered =
            self.registered(&item.model_type)
                .ok_or_else(|| MediaError::UnknownCollection {
                    model_type: item.model_type.clone(),
                    collection: item.collection.clone(),
                })?;
        let Some(collection) = registered.collections.resolve(&item.collection) else {
            return Err(MediaError::UnknownCollection {
                model_type: item.model_type.clone(),
                collection: item.collection.clone(),
            });
        };
        Ok(collection
            .conversions
            .iter()
            .filter(|c| only.is_none_or(|only| only.iter().any(|name| *name == c.name)))
            .filter(|c| !missing_only || !item.conversions.contains_key(&c.name))
            .filter(|c| generate::applies(c, &item.mime_type))
            .cloned()
            .collect())
    }

    /// Converts the items [`Regenerate`] selects, here and now, page by
    /// page; a failing item is reported and the rest go on.
    pub async fn regenerate(&self, filter: &Regenerate) -> Result<RegenerateReport, MediaError> {
        let mut report = RegenerateReport::default();
        let only = (!filter.conversions.is_empty()).then_some(filter.conversions.as_slice());
        let mut after = None;
        loop {
            let page = self.page(filter, after).await?;
            let Some(last) = page.last() else {
                break;
            };
            after = Some(last.id);
            for item in page {
                let conversions = match self.conversions_of(&item, only, filter.missing_only) {
                    Ok(conversions) => conversions,
                    Err(error) => {
                        report.failed.push((item.id, error.to_string()));
                        continue;
                    }
                };
                if conversions.is_empty() {
                    continue;
                }
                let id = item.id;
                let before = item.conversions.clone();
                let generated = match self.generate(item, &conversions, None).await {
                    Ok(generated) => generated,
                    Err(error) => {
                        report.failed.push((id, error.to_string()));
                        continue;
                    }
                };
                let count = conversions
                    .iter()
                    .filter(|c| generated.conversions.get(&c.name) != before.get(&c.name))
                    .count() as u64;
                if count == 0 {
                    continue;
                }
                match self.save_generated(&generated, &conversions).await {
                    Ok(_) => {
                        report.media += 1;
                        report.conversions += count;
                    }
                    Err(MediaError::NotFound { .. }) => {}
                    Err(error) => report.failed.push((id, error.to_string())),
                }
            }
        }
        Ok(report)
    }

    /// Queues the work of [`regenerate`](Media::regenerate) as one batch
    /// of `media.conversions` jobs on `media.queue` (needs
    /// `QueueModule`); `None` when nothing needs converting.
    pub async fn queue_regenerate(
        &self,
        filter: &Regenerate,
    ) -> Result<Option<navree_queue::JobBatch>, MediaError> {
        let queue = match self.inner.app.as_ref().and_then(WeakAppContext::upgrade) {
            Some(app) => app.try_get::<navree_queue::Queue>().await,
            None => None,
        }
        .ok_or_else(|| MediaError::Queue {
            message: "queued regeneration needs QueueModule".to_owned(),
        })?;
        let only = (!filter.conversions.is_empty()).then_some(filter.conversions.as_slice());
        let mut jobs = Vec::new();
        let mut after = None;
        loop {
            let page = self.page(filter, after).await?;
            let Some(last) = page.last() else {
                break;
            };
            after = Some(last.id);
            for item in page {
                let Ok(conversions) = self.conversions_of(&item, only, filter.missing_only) else {
                    continue;
                };
                if !conversions.is_empty() {
                    jobs.push(GenerateConversions {
                        media_id: item.id,
                        conversions: conversions.into_iter().map(|c| c.name).collect(),
                    });
                }
            }
        }
        if jobs.is_empty() {
            return Ok(None);
        }
        let batch = navree_queue::Batch::new("media.regenerate")
            .on_queue(self.inner.options.queue.clone())
            .allow_failures()
            .add(jobs);
        queue
            .dispatch_batch(batch)
            .await
            .map(Some)
            .map_err(|error| MediaError::Queue {
                message: error.to_string(),
            })
    }

    async fn page(&self, filter: &Regenerate, after: Option<Uuid>) -> Result<Vec<MediaItem>, MediaError> {
        self.list(&MediaQuery {
            model_type: filter.model_type.clone(),
            model_id: None,
            collection: filter.collection.clone(),
            ids: (!filter.ids.is_empty()).then(|| filter.ids.clone()),
            after,
            limit: Some(100),
        })
        .await
    }

    /// The client remote files are downloaded with: the app's
    /// `HttpClient`, or one with the default SSRF policy.
    async fn http(&self) -> Result<HttpClient, MediaError> {
        if let Some(client) = self.inner.http.get() {
            return Ok(client.clone());
        }
        let app_client = match self.inner.app.as_ref().and_then(WeakAppContext::upgrade) {
            Some(app) => app.try_get::<HttpClient>().await.map(|c| (*c).clone()),
            None => None,
        };
        let client = match app_client {
            Some(client) => client,
            None => HttpClient::new(HttpClientOptions::default()).map_err(|error| {
                MediaError::Download {
                    url: String::new(),
                    reason: error.to_string(),
                }
            })?,
        };
        Ok(self.inner.http.get_or_init(|| client).clone())
    }

    /// The app's job dispatcher, if it has a queue.
    async fn dispatcher(&self) -> Option<Arc<dyn Dispatcher>> {
        if self.inner.no_dispatcher {
            return None;
        }
        let app = self.inner.app.as_ref()?.upgrade()?;
        app.try_get::<dyn Dispatcher>().await
    }
}

/// Runs CPU-bound work on Tokio's blocking pool.
pub(crate) async fn blocking<T, F>(work: F) -> Result<T, MediaError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, MediaError> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| MediaError::image(format!("the conversion stopped: {error}")))?
}

/// A file being attached; finish with [`save`](PendingMedia::save).
#[must_use = "nothing is stored until `save` is awaited"]
pub struct PendingMedia {
    media: Media,
    collections: Result<Arc<MediaCollections>, MediaError>,
    model_type: String,
    model_id: String,
    collection: String,
    source: MediaSource,
    name: Option<String>,
    file_name: Option<String>,
    properties: Map<String, Value>,
}

impl std::fmt::Debug for PendingMedia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingMedia")
            .field("model_type", &self.model_type)
            .field("model_id", &self.model_id)
            .field("collection", &self.collection)
            .finish_non_exhaustive()
    }
}

impl PendingMedia {
    /// The display name (default: the file name without its extension).
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The stored file name (default: the uploaded or downloaded name),
    /// sanitized.
    pub fn file_name(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }

    /// A custom property (`alt`, `caption`, ...).
    pub fn property(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.properties.insert(key.into(), value.into());
        self
    }

    /// Stores the file and its row, then generates the conversions: the
    /// non-queued ones now, the others in one `media.conversions` job
    /// (all of them now when the app has no queue).
    ///
    /// With a unit of work the row (and the job) are part of it; the files
    /// are written at once. Pass `None` to write the row on its own (the
    /// current unit of work is joined when there is one).
    pub async fn save<'u>(self, uow: impl Into<Option<&'u Uow>>) -> Result<MediaItem, MediaError> {
        match uow.into() {
            Some(uow) => uow.scope(self.run(Some(uow))).await,
            None => self.run(None).await,
        }
    }

    async fn run(self, uow: Option<&Uow>) -> Result<MediaItem, MediaError> {
        let media = self.media;
        let options = &media.inner.options;
        let collection = self
            .collections?
            .resolve(&self.collection)
            .ok_or_else(|| MediaError::UnknownCollection {
                model_type: self.model_type.clone(),
                collection: self.collection.clone(),
            })?;
        let max = collection
            .max_size
            .map_or(options.max_file_size, |max| max.min(options.max_file_size));
        let http = if self.source.remote_url().is_some() {
            media.http().await?
        } else {
            HttpClient::fake().0
        };
        let loaded = Loader {
            storage: &media.inner.storage,
            http: &http,
            max,
            timeout: Duration::from_secs(options.download_timeout_secs),
        }
        .load(self.source)
        .await?;
        let mut bytes = loaded.bytes;
        let original_name = self.file_name.unwrap_or(loaded.file_name);
        let mime_type = mime::sniff(&bytes, Some(&original_name));
        if !collection.accepts_type(&mime_type) {
            return Err(MediaError::TypeNotAllowed {
                collection: collection.name.clone(),
                mime_type,
            });
        }

        let (mut width, mut height) = (None, None);
        #[cfg_attr(not(feature = "ffmpeg"), allow(unused_mut))]
        let mut metadata = Map::new();
        if generate::is_decodable_image(&mime_type) {
            let image_options = options.images.clone();
            let strip = options.strip_metadata;
            let (stripped, info) = {
                let bytes = bytes.clone();
                let mime_type = mime_type.clone();
                blocking(move || {
                    let stripped = if strip {
                        images::strip_metadata(&bytes, &mime_type, &image_options)?
                    } else {
                        None
                    };
                    let info = images::probe(stripped.as_deref().unwrap_or(&bytes))?;
                    Ok((stripped, info))
                })
                .await?
            };
            if let Some(stripped) = stripped {
                bytes = Bytes::from(stripped);
            }
            width = Some(info.width);
            height = Some(info.height);
        }
        #[cfg(feature = "ffmpeg")]
        if mime_type.starts_with("video/") || mime_type.starts_with("audio/") {
            if let Some((probed, probe_metadata)) = media.probe(&bytes, &mime_type).await {
                (width, height) = probed.map_or((None, None), |(w, h)| (Some(w), Some(h)));
                metadata = probe_metadata;
            }
        }

        let id = Uuid::now_v7();
        let extension = mime::extension(&mime_type);
        let file_name = sanitize_file_name(&original_name, extension);
        let name = self.name.unwrap_or_else(|| {
            file_name
                .rsplit_once('.')
                .map_or(file_name.as_str(), |(stem, _)| stem)
                .to_owned()
        });
        let default_disk = media.inner.storage.default_disk().name().to_owned();
        let disk_name = collection
            .disk
            .clone()
            .or_else(|| options.disk.clone())
            .unwrap_or(default_disk);
        let conversions_disk = collection
            .conversions_disk
            .clone()
            .or_else(|| collection.disk.clone().and(None))
            .or_else(|| options.conversions_disk.clone())
            .unwrap_or_else(|| disk_name.clone());
        let disk = media.disk(&disk_name)?;
        media.disk(&conversions_disk)?;
        let path = format!("{}/{id}/{file_name}", options.prefix());
        disk.put(&path, bytes.clone()).await?;

        let existing = media
            .inner
            .store
            .list(
                uow,
                &MediaQuery::owner(self.model_type.clone(), self.model_id.clone())
                    .collection(collection.name.clone()),
            )
            .await?;
        let order = existing.iter().map(|item| item.order).max().unwrap_or(0) + 1;
        let now = Utc::now();
        let mut item = MediaItem {
            id,
            model_type: self.model_type,
            model_id: self.model_id,
            collection: collection.name.clone(),
            name,
            file_name,
            mime_type,
            disk: disk_name,
            conversions_disk,
            path,
            size: bytes.len() as u64,
            width,
            height,
            conversions: BTreeMap::new(),
            metadata,
            custom_properties: self.properties,
            order,
            created_at: now,
            updated_at: now,
        };
        if let Err(error) = media.inner.store.insert(uow, &item).await {
            media.delete_files(std::slice::from_ref(&item)).await;
            return Err(error);
        }

        if let Some(keep) = collection.keep_latest
            && existing.len() + 1 > keep
        {
            let surplus = existing.len() + 1 - keep;
            let old: Vec<MediaItem> = existing.into_iter().take(surplus).collect();
            media.remove(uow, old).await?;
        }

        let applicable: Vec<Conversion> = collection
            .conversions
            .iter()
            .filter(|c| generate::applies(c, &item.mime_type))
            .cloned()
            .collect();
        let dispatcher = if applicable.is_empty() {
            None
        } else {
            media.dispatcher().await
        };
        let (now, later): (Vec<Conversion>, Vec<Conversion>) = applicable
            .into_iter()
            .partition(|c| dispatcher.is_none() || !c.runs_queued(options.queue_conversions));
        if !now.is_empty() {
            item = media.generate(item, &now, Some(bytes)).await?;
            media.inner.store.update(uow, &item).await?;
        }
        if let (Some(dispatcher), false) = (dispatcher, later.is_empty()) {
            let job = GenerateConversions {
                media_id: item.id,
                conversions: later.into_iter().map(|c| c.name).collect(),
            };
            let mut envelope = JobEnvelope::of(&job).map_err(|error| MediaError::Queue {
                message: error.to_string(),
            })?;
            envelope.options.queue = options.queue.clone();
            dispatcher
                .dispatch_envelope(envelope)
                .await
                .map_err(|error| MediaError::Queue {
                    message: error.to_string(),
                })?;
        }
        if let Some(recorder) = media.recorder() {
            recorder.added(&item);
        }
        Ok(item)
    }
}
