//! [`HasMedia`], [`MediaCollections`] and [`MediaCollection`]: what a
//! model accepts and which conversions its files get.

use crate::conversion::Conversion;
use crate::mime;

/// A model files are attached to.
///
/// ```
/// use navree_media::{Conversion, Fit, HasMedia, MediaCollections, OutputFormat};
/// # use uuid::Uuid;
/// # struct Course { id: Uuid }
///
/// impl HasMedia for Course {
///     const MEDIA_TYPE: &'static str = "Course";
///     const MEDIA_TABLE: &'static str = "courses";
///
///     fn media_key(&self) -> String {
///         self.id.to_string()
///     }
///
///     fn media_collections(collections: &mut MediaCollections) {
///         collections
///             .collection("thumbnail")
///             .single_file()
///             .accepts(["image/*"])
///             .conversion(Conversion::new("card").fit(Fit::Cover, 640, 360).format(OutputFormat::WebP));
///         collections
///             .collection("attachments")
///             .accepts(["application/pdf", "image/*"])
///             .max_size(10 * 1024 * 1024);
///     }
/// }
/// ```
pub trait HasMedia {
    /// The model's name in the `media` table's `model_type` column
    /// (`Course`). Keep it stable: renaming it orphans existing rows.
    const MEDIA_TYPE: &'static str;

    /// The model's table. When it is set and the model is registered with
    /// [`MediaModule::for_feature`](crate::MediaModule::for_feature),
    /// deleting a row through the unit of work (a navree-orm record's
    /// `delete`, `Model::destroy`, `Repo::delete`: the deletes that record
    /// change events) deletes its media in the same commit (rows) and
    /// right after it (files). Empty by default: delete media yourself
    /// with [`Media::delete_all`](crate::Media::delete_all), as you do for
    /// bulk deletes, which record no change events, and for models with a
    /// `deleted_at` column, whose soft deletes keep their media.
    const MEDIA_TABLE: &'static str = "";

    /// The record's key in the `model_id` column. For automatic deletion it
    /// must match the primary key as `Repo` reports it (`to_string()` of a
    /// UUID or an integer).
    fn media_key(&self) -> String;

    /// Declares the model's collections. A model that declares none
    /// accepts any collection name with the module's defaults.
    fn media_collections(collections: &mut MediaCollections) {
        let _ = collections;
    }
}

/// The collections of one model type.
#[derive(Clone, Debug, Default)]
pub struct MediaCollections {
    collections: Vec<MediaCollection>,
}

impl MediaCollections {
    /// The collections `M` declares.
    pub fn of<M: HasMedia>() -> Self {
        let mut collections = Self::default();
        M::media_collections(&mut collections);
        collections
    }

    /// The collection `name`, added with the defaults the first time.
    pub fn collection(&mut self, name: &str) -> &mut MediaCollection {
        let position = match self.collections.iter().position(|c| c.name == name) {
            Some(position) => position,
            None => {
                self.collections.push(MediaCollection::new(name));
                self.collections.len() - 1
            }
        };
        &mut self.collections[position]
    }

    /// The collection `name`, if declared.
    pub fn get(&self, name: &str) -> Option<&MediaCollection> {
        self.collections.iter().find(|c| c.name == name)
    }

    /// Every declared collection.
    pub fn iter(&self) -> impl Iterator<Item = &MediaCollection> {
        self.collections.iter()
    }

    /// Whether no collection is declared.
    pub fn is_empty(&self) -> bool {
        self.collections.is_empty()
    }

    /// The collection files named `name` go to: the declared one, or a
    /// default one when nothing is declared.
    pub(crate) fn resolve(&self, name: &str) -> Option<MediaCollection> {
        match self.get(name) {
            Some(collection) => Some(collection.clone()),
            None if self.collections.is_empty() => Some(MediaCollection::new(name)),
            None => None,
        }
    }

    /// Checks every collection and conversion name.
    pub(crate) fn check(&self, model_type: &str) -> Result<(), String> {
        for collection in &self.collections {
            let valid = !collection.name.is_empty()
                && collection.name.len() <= 100
                && collection
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            if !valid {
                return Err(format!(
                    "{model_type}: collection name `{}` must be 1 to 100 letters, digits, `-` or `_`",
                    collection.name
                ));
            }
            let mut seen = Vec::new();
            for conversion in &collection.conversions {
                conversion
                    .check()
                    .map_err(|e| format!("{model_type}: {e}"))?;
                if seen.contains(&conversion.name) {
                    return Err(format!(
                        "{model_type}: collection `{}` declares the conversion `{}` twice",
                        collection.name, conversion.name
                    ));
                }
                seen.push(conversion.name.clone());
            }
        }
        Ok(())
    }
}

/// A named group of files on a model (`thumbnail`, `attachments`): what
/// it accepts, where its files go and which conversions they get.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaCollection {
    pub(crate) name: String,
    pub(crate) keep_latest: Option<usize>,
    pub(crate) accepts: Vec<String>,
    pub(crate) max_size: Option<u64>,
    pub(crate) disk: Option<String>,
    pub(crate) conversions_disk: Option<String>,
    pub(crate) conversions: Vec<Conversion>,
}

impl MediaCollection {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            keep_latest: None,
            accepts: Vec::new(),
            max_size: None,
            disk: None,
            conversions_disk: None,
            conversions: Vec::new(),
        }
    }

    /// The name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Keeps one file: adding a new one deletes the previous one.
    pub fn single_file(&mut self) -> &mut Self {
        self.keep_latest = Some(1);
        self
    }

    /// Keeps the `count` newest files.
    pub fn only_keep_latest(&mut self, count: usize) -> &mut Self {
        self.keep_latest = Some(count.max(1));
        self
    }

    /// Accepts only these types, sniffed from the content: exact types
    /// (`application/pdf`) or families (`image/*`). Everything by default.
    pub fn accepts<I, S>(&mut self, types: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.accepts = types.into_iter().map(Into::into).collect();
        self
    }

    /// The largest file in bytes (never more than `media.max_file_size`).
    pub fn max_size(&mut self, bytes: u64) -> &mut Self {
        self.max_size = Some(bytes);
        self
    }

    /// The storage disk this collection's originals go to.
    pub fn disk(&mut self, disk: impl Into<String>) -> &mut Self {
        self.disk = Some(disk.into());
        self
    }

    /// The storage disk this collection's conversions go to.
    pub fn conversions_disk(&mut self, disk: impl Into<String>) -> &mut Self {
        self.conversions_disk = Some(disk.into());
        self
    }

    /// Adds a conversion generated for every file of the collection.
    pub fn conversion(&mut self, conversion: Conversion) -> &mut Self {
        self.conversions.push(conversion);
        self
    }

    /// The conversions.
    pub fn conversions(&self) -> &[Conversion] {
        &self.conversions
    }

    /// The conversion named `name`.
    pub fn conversion_named(&self, name: &str) -> Option<&Conversion> {
        self.conversions.iter().find(|c| c.name == name)
    }

    /// Whether files of `mime_type` are accepted.
    pub fn accepts_type(&self, mime_type: &str) -> bool {
        self.accepts.is_empty()
            || self
                .accepts
                .iter()
                .any(|pattern| mime::matches(pattern, mime_type))
    }
}
