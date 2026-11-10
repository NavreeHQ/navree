//! Models the tests attach files to, and services to attach them with.

use std::sync::Arc;

use navree_media::{
    Conversion, Fit, HasMedia, Media, MediaCollections, MediaOptions, OutputFormat,
};
use navree_storage::{DiskConfig, Storage, StorageOptions};

/// A course: a single thumbnail (images, a WebP card), a gallery (a
/// responsive hero and a thumb), attachments (PDFs, images and text, at
/// most 64 KiB, with a preview), and handouts that keep the latest two.
pub struct Course {
    pub id: i64,
}

impl HasMedia for Course {
    const MEDIA_TYPE: &'static str = "Course";
    const MEDIA_TABLE: &'static str = "courses";

    fn media_key(&self) -> String {
        self.id.to_string()
    }

    fn media_collections(collections: &mut MediaCollections) {
        collections
            .collection("thumbnail")
            .single_file()
            .accepts(["image/*"])
            .conversion(
                Conversion::new("card")
                    .fit(Fit::Cover, 64, 36)
                    .format(OutputFormat::WebP),
            );
        collections
            .collection("gallery")
            .accepts(["image/*", "video/*"])
            .conversion(Conversion::new("hero").width(800).responsive())
            .conversion(Conversion::new("thumb").fit(Fit::Contain, 100, 100));
        collections
            .collection("attachments")
            .accepts(["application/pdf", "image/*", "text/*"])
            .max_size(64 * 1024)
            .conversion(Conversion::new("preview").width(50));
        collections.collection("handouts").only_keep_latest(2);
    }
}

/// A note: declares no collections, so any name works.
pub struct Note {
    pub id: String,
}

impl HasMedia for Note {
    const MEDIA_TYPE: &'static str = "Note";

    fn media_key(&self) -> String {
        self.id.clone()
    }
}

/// A storage with two memory disks, `media` (the default) and `archive`,
/// and a local disk `public` served from `https://cdn.example.com/files`.
pub fn storage(public_root: &std::path::Path) -> Arc<Storage> {
    let mut options = StorageOptions {
        default: "media".into(),
        ..StorageOptions::default()
    };
    options.disks.clear();
    options.disks.insert("media".into(), DiskConfig::Memory);
    options.disks.insert("archive".into(), DiskConfig::Memory);
    options.disks.insert(
        "public".into(),
        DiskConfig::Local {
            root: public_root.to_path_buf(),
            public_url: Some("https://cdn.example.com/files".into()),
        },
    );
    Arc::new(Storage::new(&options, b"test signing key").expect("storage"))
}

/// A media service outside an app (no queue: conversions run at once).
pub struct Service {
    pub media: Media,
    pub storage: Arc<Storage>,
    _dir: tempfile::TempDir,
}

pub fn service() -> Service {
    service_with(MediaOptions::default())
}

pub fn service_with(options: MediaOptions) -> Service {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("temp dir");
    let storage = storage(dir.path());
    let media = Media::builder(storage.clone())
        .options(options)
        .register::<Course>()
        .build()
        .expect("media");
    Service {
        media,
        storage,
        _dir: dir,
    }
}
