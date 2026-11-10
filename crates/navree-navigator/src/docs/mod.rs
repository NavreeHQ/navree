//! `search_docs` and `read_doc`: the docs of the Navree version the app
//! uses.
//!
//! The docs come as a bundle (`docs.json`: every page with its title,
//! description and `##` sections as Markdown). Navigator takes it from, in
//! order:
//!
//! 1. the docs built into Navigator, when they are of the app's version;
//! 2. the cache, `~/.navree/docs/<version>/docs.json` (`NAVREE_HOME`
//!    replaces `~/.navree`);
//! 3. `https://navree.com/v/<version>/docs.json`, downloaded once into the
//!    cache (never with `--docs-offline`);
//! 4. the docs built into Navigator, with a note naming both versions.

pub mod mdx;
mod search;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use search::{SearchHit, SearchIndex, tokenize};

/// The site the docs live on.
pub const SITE: &str = "https://navree.com";

/// The Navree version whose docs are built into Navigator.
pub const EMBEDDED_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A docs bundle: every page of one Navree version.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocsBundle {
    /// The Navree version the docs describe.
    #[serde(default)]
    pub version: String,
    /// The pages.
    pub pages: Vec<DocPage>,
}

/// One docs page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocPage {
    /// Its path under `/docs/` (`ai/navigator`).
    pub slug: String,
    /// The title.
    pub title: String,
    /// The one-sentence description.
    #[serde(default)]
    pub description: String,
    /// Whether it documents something not built yet.
    #[serde(default)]
    pub planned: bool,
    /// The `##` sections, in order.
    #[serde(default)]
    pub sections: Vec<DocSection>,
}

/// One `##` section of a page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocSection {
    /// The heading (empty for text before the first heading).
    #[serde(default, alias = "title")]
    pub heading: String,
    /// The heading's anchor (`#the-tools`).
    #[serde(default, alias = "id")]
    pub anchor: String,
    /// The section's Markdown.
    #[serde(default, alias = "content", alias = "body")]
    pub markdown: String,
}

impl DocPage {
    /// The page as one Markdown document, or only its section whose heading
    /// or anchor is `section`.
    pub fn markdown(&self, section: Option<&str>) -> Option<String> {
        let mut out = format!("# {}\n\n", self.title);
        if !self.description.is_empty() {
            out.push_str(&format!("{}\n\n", self.description));
        }
        if self.planned {
            out.push_str(
                "> **Planned:** this page documents something that isn't built yet in this version.\n\n",
            );
        }
        out.push_str(&format!("Source: {SITE}/docs/{}/\n\n", self.slug));
        let wanted = section.map(|s| s.trim().trim_start_matches('#').to_lowercase());
        let mut found = wanted.is_none();
        for s in &self.sections {
            if let Some(wanted) = &wanted
                && s.anchor.to_lowercase() != *wanted
                && s.heading.to_lowercase() != *wanted
            {
                continue;
            }
            found = true;
            if !s.heading.is_empty() {
                out.push_str(&format!("## {}\n\n", s.heading));
            }
            out.push_str(&s.markdown);
            out.push_str("\n\n");
        }
        found.then(|| out.trim_end().to_owned() + "\n")
    }
}

impl DocsBundle {
    /// The docs built into Navigator (the site's MDX pages at build time).
    pub fn embedded() -> DocsBundle {
        DocsBundle {
            version: EMBEDDED_VERSION.to_owned(),
            pages: crate::embedded::DOC_PAGES
                .iter()
                .map(|(slug, source)| mdx::page_from_mdx(slug, source))
                .collect(),
        }
    }

    /// The page at `reference`: a slug (`ai/navigator`), a path
    /// (`/docs/ai/navigator/`, `ai/navigator.md`) or a URL.
    pub fn page(&self, reference: &str) -> Option<&DocPage> {
        let slug = normalize_slug(reference);
        self.pages.iter().find(|p| p.slug == slug)
    }

    /// Slugs that share a word with `reference`, for "did you mean".
    pub fn similar(&self, reference: &str) -> Vec<String> {
        let words = tokenize(&normalize_slug(reference));
        self.pages
            .iter()
            .filter(|p| {
                let slug_words = tokenize(&p.slug);
                words.iter().any(|w| slug_words.contains(w))
            })
            .map(|p| p.slug.clone())
            .take(8)
            .collect()
    }
}

/// `https://navree.com/docs/ai/navigator/#x`, `/docs/ai/navigator.md` and
/// `ai/navigator` all name `ai/navigator`.
pub fn normalize_slug(reference: &str) -> String {
    let mut s = reference.trim();
    if let Some(rest) = s.split_once("://").map(|(_, rest)| rest) {
        s = rest.split_once('/').map_or("", |(_, path)| path);
    }
    let s = s.split(['#', '?']).next().unwrap_or_default();
    let s = s.trim_matches('/');
    let s = s.strip_prefix("docs/").unwrap_or(s);
    let s = s.strip_suffix(".md").unwrap_or(s);
    s.trim_matches('/').to_owned()
}

/// Where the loaded docs came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DocsSource {
    /// Built into Navigator.
    Embedded,
    /// The cache directory.
    Cache,
    /// Downloaded just now (and cached).
    Download,
}

/// The docs a session searches: the bundle, its index, where it came from.
#[derive(Debug)]
pub struct DocsLibrary {
    /// The bundle.
    pub bundle: DocsBundle,
    /// Its index.
    pub index: SearchIndex,
    /// Where it came from.
    pub source: DocsSource,
    /// A note for the agent when these aren't the docs of the app's version.
    pub note: Option<String>,
}

/// Fetches a URL's body: `Ok(None)` for a 404.
pub trait DocsFetcher: Send + Sync {
    /// GETs `url`.
    fn fetch(
        &self,
        url: &str,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Option<String>, String>> + Send + '_>>;
}

/// The real fetcher: reqwest on rustls with the ring provider, 20 s timeout.
#[derive(Clone, Debug)]
pub struct HttpFetcher {
    client: reqwest::Client,
}

impl HttpFetcher {
    /// A new fetcher.
    pub fn new() -> Result<HttpFetcher, String> {
        // reqwest is built without a default crypto provider: use ring. An
        // error only means a process-wide provider is already installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("navree-navigator/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| format!("cannot build the HTTP client: {e}"))?;
        Ok(HttpFetcher { client })
    }
}

impl DocsFetcher for HttpFetcher {
    fn fetch(
        &self,
        url: &str,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Option<String>, String>> + Send + '_>> {
        let url = url.to_owned();
        Box::pin(async move {
            let response = self
                .client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("cannot download {url}: {e}"))?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if !response.status().is_success() {
                return Err(format!("{url} answered {}", response.status()));
            }
            response
                .text()
                .await
                .map(Some)
                .map_err(|e| format!("cannot read {url}: {e}"))
        })
    }
}

/// Where docs are looked for.
#[derive(Clone)]
pub struct DocsSettings {
    /// `~/.navree` (or `NAVREE_HOME`): the cache is `<home>/docs/<version>/`.
    pub home: Option<PathBuf>,
    /// The site the bundles are downloaded from (`https://navree.com`).
    pub base_url: String,
    /// Never download (`--docs-offline`).
    pub offline: bool,
    /// The downloader.
    pub fetcher: Arc<dyn DocsFetcher>,
}

impl std::fmt::Debug for DocsSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocsSettings")
            .field("home", &self.home)
            .field("base_url", &self.base_url)
            .field("offline", &self.offline)
            .finish_non_exhaustive()
    }
}

/// `NAVREE_HOME`, else `~/.navree`.
pub fn navree_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("NAVREE_HOME").filter(|h| !h.is_empty()) {
        return Some(PathBuf::from(home));
    }
    std::env::home_dir().map(|home| home.join(".navree"))
}

/// The cache file of `version`'s bundle.
pub fn cache_file(home: &Path, version: &str) -> PathBuf {
    home.join("docs").join(version).join("docs.json")
}

impl DocsLibrary {
    /// Loads the docs of `version` (the app's Navree version, if known).
    pub async fn load(version: Option<&str>, settings: &DocsSettings) -> DocsLibrary {
        // A version names a cache directory and a URL path: plain versions only.
        let version = version.filter(|v| {
            !v.is_empty()
                && !v.starts_with('.')
                && v.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        });
        let Some(version) = version else {
            return Self::embedded(Some(format!(
                "The app's Cargo.lock doesn't pin navree, so these are the docs of Navree {EMBEDDED_VERSION}, which Navigator ships with."
            )));
        };
        if version == EMBEDDED_VERSION {
            return Self::embedded(None);
        }
        let cache = settings
            .home
            .as_deref()
            .map(|home| cache_file(home, version));
        if let Some(cache) = &cache
            && let Ok(text) = std::fs::read_to_string(cache)
            && let Ok(bundle) = serde_json::from_str::<DocsBundle>(&text)
        {
            return Self::from_bundle(bundle, DocsSource::Cache, None);
        }
        let reason = if settings.offline {
            "they aren't cached and --docs-offline is on".to_owned()
        } else {
            let url = format!(
                "{}/v/{version}/docs.json",
                settings.base_url.trim_end_matches('/')
            );
            match settings.fetcher.fetch(&url).await {
                Ok(Some(text)) => match serde_json::from_str::<DocsBundle>(&text) {
                    Ok(bundle) => {
                        if let Some(cache) = &cache
                            && let Some(dir) = cache.parent()
                        {
                            let _ = std::fs::create_dir_all(dir)
                                .and_then(|()| std::fs::write(cache, &text));
                        }
                        return Self::from_bundle(bundle, DocsSource::Download, None);
                    }
                    Err(error) => format!("{url} isn't a docs bundle ({error})"),
                },
                Ok(None) => format!("{url} doesn't exist"),
                Err(error) => error,
            }
        };
        Self::embedded(Some(format!(
            "The docs of Navree {version} (the app's version) aren't available: {reason}. These are the docs of \
             Navree {EMBEDDED_VERSION}, which Navigator ships with; check APIs against the code when versions differ."
        )))
    }

    fn embedded(note: Option<String>) -> DocsLibrary {
        Self::from_bundle(DocsBundle::embedded(), DocsSource::Embedded, note)
    }

    fn from_bundle(bundle: DocsBundle, source: DocsSource, note: Option<String>) -> DocsLibrary {
        let index = SearchIndex::new(&bundle);
        DocsLibrary {
            bundle,
            index,
            source,
            note,
        }
    }

    /// The best `limit` sections for `query`.
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        self.index.search(&self.bundle, query, limit)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Answers every URL with `body` and records the URLs asked for.
    struct StubFetcher {
        body: Result<Option<String>, String>,
        asked: Mutex<Vec<String>>,
    }

    impl DocsFetcher for StubFetcher {
        fn fetch(
            &self,
            url: &str,
        ) -> std::pin::Pin<Box<dyn Future<Output = Result<Option<String>, String>> + Send + '_>>
        {
            self.asked.lock().unwrap().push(url.to_owned());
            let body = self.body.clone();
            Box::pin(async move { body })
        }
    }

    fn settings(
        home: &Path,
        offline: bool,
        body: Result<Option<String>, String>,
    ) -> (DocsSettings, Arc<StubFetcher>) {
        let fetcher = Arc::new(StubFetcher {
            body,
            asked: Mutex::new(Vec::new()),
        });
        (
            DocsSettings {
                home: Some(home.to_path_buf()),
                base_url: "https://docs.test/".to_owned(),
                offline,
                fetcher: fetcher.clone(),
            },
            fetcher,
        )
    }

    fn tiny_bundle(version: &str) -> String {
        serde_json::to_string(&DocsBundle {
            version: version.to_owned(),
            pages: vec![DocPage {
                slug: "basics/old".to_owned(),
                title: "Old API".to_owned(),
                sections: vec![DocSection {
                    heading: "Usage".to_owned(),
                    anchor: "usage".to_owned(),
                    markdown: "Call `old_thing()`.".to_owned(),
                }],
                ..DocPage::default()
            }],
        })
        .unwrap()
    }

    #[test]
    fn the_embedded_docs_hold_the_site_pages() {
        let bundle = DocsBundle::embedded();
        assert_eq!(bundle.version, EMBEDDED_VERSION);
        let navigator = bundle.page("ai/navigator").expect("the Navigator page");
        assert_eq!(navigator.title, "Navigator");
        assert!(bundle.pages.len() > 50, "{}", bundle.pages.len());
        assert!(bundle.page("/docs/ai/navigator/").is_some());
        assert!(
            bundle
                .page("https://navree.com/docs/ai/navigator/#options")
                .is_some()
        );
        assert!(bundle.page("ai/navigator.md").is_some());
        assert!(
            bundle
                .similar("navigator")
                .contains(&"ai/navigator".to_owned())
        );
    }

    #[test]
    fn slugs_are_normalized() {
        assert_eq!(
            normalize_slug("https://navree.com/docs/ai/navigator/#x"),
            "ai/navigator"
        );
        assert_eq!(
            normalize_slug("/docs/basics/validation.md"),
            "basics/validation"
        );
        assert_eq!(normalize_slug("basics/validation/"), "basics/validation");
    }

    #[test]
    fn a_page_reads_as_markdown_whole_or_by_section() {
        let bundle: DocsBundle = serde_json::from_str(&tiny_bundle("0.0.1")).unwrap();
        let page = bundle.page("basics/old").unwrap();
        let whole = page.markdown(None).unwrap();
        assert!(whole.starts_with("# Old API\n"), "{whole}");
        assert!(whole.contains("## Usage\n\nCall `old_thing()`."), "{whole}");
        assert!(page.markdown(Some("#usage")).is_some());
        assert!(page.markdown(Some("Usage")).is_some());
        assert!(page.markdown(Some("missing")).is_none());
    }

    #[tokio::test]
    async fn the_apps_version_is_downloaded_once_then_cached() {
        let home = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let (settings, fetcher) = settings(home.path(), false, Ok(Some(tiny_bundle("0.0.1"))));
        let library = DocsLibrary::load(Some("0.0.1"), &settings).await;
        assert_eq!(library.source, DocsSource::Download);
        assert_eq!(library.note, None);
        assert_eq!(
            *fetcher.asked.lock().unwrap(),
            ["https://docs.test/v/0.0.1/docs.json"]
        );
        assert!(cache_file(home.path(), "0.0.1").is_file());

        let library = DocsLibrary::load(Some("0.0.1"), &settings).await;
        assert_eq!(library.source, DocsSource::Cache);
        assert_eq!(fetcher.asked.lock().unwrap().len(), 1, "no second download");
        assert_eq!(library.search("old thing", 3)[0].slug, "basics/old");
    }

    #[tokio::test]
    async fn the_embedded_docs_serve_their_own_version_without_network() {
        let home = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let (settings, fetcher) = settings(home.path(), false, Err("no network".into()));
        let library = DocsLibrary::load(Some(EMBEDDED_VERSION), &settings).await;
        assert_eq!(library.source, DocsSource::Embedded);
        assert_eq!(library.note, None);
        assert!(fetcher.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn offline_or_missing_versions_fall_back_with_a_note() {
        let home = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let (settings, fetcher) = settings(home.path(), true, Ok(Some(tiny_bundle("0.0.1"))));
        let library = DocsLibrary::load(Some("0.0.1"), &settings).await;
        assert_eq!(library.source, DocsSource::Embedded);
        let note = library.note.unwrap();
        assert!(note.contains("--docs-offline"), "{note}");
        assert!(
            fetcher.asked.lock().unwrap().is_empty(),
            "offline never downloads"
        );

        let (settings, _) = settings_404(home.path());
        let library = DocsLibrary::load(Some("0.0.2"), &settings).await;
        assert_eq!(library.source, DocsSource::Embedded);
        assert!(library.note.unwrap().contains("doesn't exist"));

        let library = DocsLibrary::load(None, &settings).await;
        assert!(library.note.unwrap().contains("doesn't pin navree"));

        // A version that isn't a plain version never names a path or URL.
        let (settings, fetcher) = settings_404(home.path());
        let library = DocsLibrary::load(Some("../../escape"), &settings).await;
        assert_eq!(library.source, DocsSource::Embedded);
        assert!(fetcher.asked.lock().unwrap().is_empty());
    }

    fn settings_404(home: &Path) -> (DocsSettings, Arc<StubFetcher>) {
        settings(home, false, Ok(None))
    }

    #[tokio::test]
    async fn the_http_fetcher_downloads_and_reports_404() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v/0.0.1/docs.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(tiny_bundle("0.0.1")))
            .mount(&server)
            .await;
        let fetcher = HttpFetcher::new().unwrap();
        let body = fetcher
            .fetch(&format!("{}/v/0.0.1/docs.json", server.uri()))
            .await
            .unwrap()
            .unwrap();
        assert!(body.contains("basics/old"));
        let missing = fetcher
            .fetch(&format!("{}/v/9.9.9/docs.json", server.uri()))
            .await
            .unwrap();
        assert_eq!(missing, None);

        let home = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let settings = DocsSettings {
            home: Some(home.path().to_path_buf()),
            base_url: server.uri(),
            offline: false,
            fetcher: Arc::new(fetcher),
        };
        let library = DocsLibrary::load(Some("0.0.1"), &settings).await;
        assert_eq!(library.source, DocsSource::Download);
    }
}
