//! `docs/plugins/testing.mdx` and `docs/plugins/releasing.mdx`: running the testing page's
//! `TestApp`, `DiskDriver` and `plugin:publish` recipes, and checking that the releasing page
//! matches what the repository's example plugins ship.

use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use navree::datetime::Timestamp;
use navree::http::body::Bytes;
use navree::kernel::BoxFuture;
use navree::prelude::*;
use navree::storage::{
    Disk, DiskDriver, DiskDriverError, DiskUpload, FileMeta, OpenedFile, StorageError,
    StorageModule,
};
use navree::testing::TestApp;
use navree::view::ViewModule;

const PUBLISH_CHILD_ENV: &str = "NAVREE_PLUGINS_DOCS_TESTING_PUBLISH_ARGS";
const FIXTURE_LIST_HTML: &str = "<p>Notices from the Ludhiana office</p>\n";

type Files = Arc<Mutex<BTreeMap<String, (Bytes, Timestamp)>>>;

#[derive(Clone, Default)]
struct Sanduk {
    files: Files,
}

impl Sanduk {
    fn file(&self, path: &str) -> Result<(Bytes, Timestamp), DiskDriverError> {
        let files = self.files.lock().unwrap();
        files.get(path).cloned().ok_or(DiskDriverError::NotFound)
    }
}

impl DiskDriver for Sanduk {
    fn describe(&self) -> String {
        "sanduk disk in memory".to_owned()
    }

    fn put<'a>(
        &'a self,
        path: &'a str,
        contents: Bytes,
    ) -> BoxFuture<'a, Result<(), DiskDriverError>> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), (contents, Timestamp::now()));
        Box::pin(async { Ok(()) })
    }

    fn begin_upload<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, Result<Box<dyn DiskUpload>, DiskDriverError>> {
        let upload = SandukUpload {
            files: self.files.clone(),
            path: path.to_owned(),
            buffer: Vec::new(),
        };
        Box::pin(async move { Ok(Box::new(upload) as Box<dyn DiskUpload>) })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, Result<Bytes, DiskDriverError>> {
        let read = self.file(path).map(|(contents, _)| match range {
            Some(range) => {
                let end = usize::try_from(range.end)
                    .map_or(contents.len(), |end| end.min(contents.len()));
                let start = usize::try_from(range.start).map_or(end, |start| start.min(end));
                contents.slice(start..end)
            }
            None => contents,
        });
        Box::pin(async move { read })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<OpenedFile, DiskDriverError>> {
        let opened = self.file(path).map(|(contents, _)| {
            let size = contents.len() as u64;
            OpenedFile::new(size, Box::pin(tokio_stream::once(Ok(contents))))
        });
        Box::pin(async move { opened })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<FileMeta, DiskDriverError>> {
        let meta = self.file(path).map(|(contents, last_modified)| FileMeta {
            path: path.to_owned(),
            size: contents.len() as u64,
            last_modified,
        });
        Box::pin(async move { meta })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<(), DiskDriverError>> {
        let removed = self.files.lock().unwrap().remove(path);
        Box::pin(async move { removed.map(drop).ok_or(DiskDriverError::NotFound) })
    }

    fn list<'a>(
        &'a self,
        prefix: &'a str,
    ) -> BoxFuture<'a, Result<Vec<FileMeta>, DiskDriverError>> {
        let folder = format!("{prefix}/");
        let files: Vec<FileMeta> = self
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| prefix.is_empty() || path.starts_with(&folder))
            .map(|(path, (contents, last_modified))| FileMeta {
                path: path.clone(),
                size: contents.len() as u64,
                last_modified: *last_modified,
            })
            .collect();
        Box::pin(async move { Ok(files) })
    }
}

struct SandukUpload {
    files: Files,
    path: String,
    buffer: Vec<u8>,
}

impl DiskUpload for SandukUpload {
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, Result<(), DiskDriverError>> {
        self.buffer.extend_from_slice(&chunk);
        Box::pin(async { Ok(()) })
    }

    fn finish(self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
        let SandukUpload {
            files,
            path,
            buffer,
        } = *self;
        files
            .lock()
            .unwrap()
            .insert(path, (Bytes::from(buffer), Timestamp::now()));
        Box::pin(async { Ok(()) })
    }

    fn abort(self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
        Box::pin(async { Ok(()) })
    }
}

/// Inline plugin standing in for its crate (`module_path!()`, as an integration test binary
/// compiles as one crate).
mod sanduk_plugin {
    use navree::plugin::PluginInfo;
    use navree::plugin::prelude::*;
    use navree::prelude::*;

    use super::Sanduk;

    const SANDUK: PluginInfo =
        PluginInfo::new("navree-plugin-sanduk", module_path!(), "sanduk", "0.1.0")
            .title("Sanduk storage");

    pub struct SandukModule;

    impl Module for SandukModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(SANDUK)
                .disk_driver("sanduk", |_| async { Ok(Sanduk::default()) })
                .plugin_templates(navree::embed_templates!(
                    "tests/fixtures/plugins_docs_resources/views"
                ));
        }
    }
}

fn run_testing_publish(dir: &Path, args: &[&str]) -> String {
    let encoded = serde_json::to_string(&args).expect("encode publish args");
    let output = Command::new(std::env::current_exe().expect("plugins_docs test binary"))
        .arg("--exact")
        .arg("testing_page::the_testing_page_recipe_runs")
        .arg("--nocapture")
        .current_dir(dir)
        .env(PUBLISH_CHILD_ENV, encoded)
        .output()
        .expect("spawn child publish process");
    assert!(
        output.status.success(),
        "command {args:?} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}

#[derive(Debug)]
struct FencedBlock {
    line: usize,
    lang: String,
    code: String,
}

fn fenced_blocks(page: &str) -> Vec<FencedBlock> {
    let mut out = Vec::new();
    let mut open: Option<(usize, usize, usize, String)> = None;
    let mut code = String::new();
    for (idx, line) in page.split('\n').enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let ticks = trimmed.chars().take_while(|&ch| ch == '`').count();
        match &open {
            None if ticks >= 3 => {
                open = Some((idx + 1, ticks, indent, trimmed[ticks..].trim().to_owned()));
                code.clear();
            }
            None => {}
            Some((start, opened, at, info)) => {
                if ticks >= *opened && trimmed.trim_end().len() == ticks {
                    let lang = info
                        .split([',', ' '])
                        .find(|word| !word.is_empty())
                        .unwrap_or_default()
                        .to_owned();
                    out.push(FencedBlock {
                        line: *start,
                        lang,
                        code: std::mem::take(&mut code),
                    });
                    open = None;
                } else {
                    let cut = line.len().min(*at);
                    let strip = line[..cut].trim().is_empty();
                    code.push_str(if strip { &line[cut..] } else { line });
                    code.push('\n');
                }
            }
        }
    }
    assert!(open.is_none(), "unclosed fence in page");
    out
}

fn assert_slice_of_file(repo_root: &Path, rel_path: &str, body_lines: &[&str], block_line: usize) {
    let full_path = repo_root.join(rel_path);
    let source = fs::read_to_string(&full_path)
        .unwrap_or_else(|err| {
            panic!("block on line {block_line} quotes missing file `{rel_path}`: {err}")
        })
        .replace("\r\n", "\n");
    let file_lines: Vec<&str> = source.lines().map(str::trim_end).collect();
    let needle: Vec<&str> = body_lines.iter().map(|line| line.trim_end()).collect();
    assert!(
        !needle.is_empty(),
        "empty slice block on line {block_line} for `{rel_path}`"
    );
    let found = file_lines
        .windows(needle.len())
        .any(|window| window == needle.as_slice());
    assert!(
        found,
        "block on line {block_line} is not a consecutive line slice of `{rel_path}`"
    );
}

#[tokio::test]
async fn the_testing_page_recipe_runs() {
    if let Ok(encoded) = std::env::var(PUBLISH_CHILD_ENV) {
        let args: Vec<String> = serde_json::from_str(&encoded).expect("decode publish args");
        Navree::app()
            .module(ConfigModule::for_root(ConfigSources::new()))
            .module(sanduk_plugin::SandukModule)
            .run_with_args(args)
            .await
            .expect("plugin:publish runs cleanly");
        return;
    }

    // Boot the inline plugin in TestApp and verify it is listed in `plugins`.
    let app = TestApp::builder()
        .config("storage.default", "invoices")
        .config("storage.disks.invoices.driver", "sanduk")
        .module(StorageModule::from_config())
        .module(ViewModule::from_config())
        .module(sanduk_plugin::SandukModule)
        .build()
        .await;

    let report = app.ctx().plugins();
    let plugin = report
        .plugins
        .iter()
        .find(|plugin| plugin.info.name == "navree-plugin-sanduk")
        .expect("sanduk plugin listed in plugins");
    assert_eq!(plugin.info.namespace, "sanduk");
    assert_eq!(plugin.info.title, Some("Sanduk storage"));
    assert!(!plugin.first_party);

    // Run the six Disk calls and check the missing-file error.
    let disk = app.get::<Disk>().await;
    disk.put("hello.txt", "hello").await.expect("put hello.txt");
    assert_eq!(disk.get("hello.txt").await.expect("get hello.txt"), "hello");
    assert!(
        disk.exists("hello.txt")
            .await
            .expect("exists before delete")
    );
    assert_eq!(disk.size("hello.txt").await.expect("size of hello.txt"), 5);
    assert_eq!(
        disk.list("").await.expect("list root"),
        vec!["hello.txt".to_owned()]
    );
    disk.delete("hello.txt").await.expect("delete hello.txt");
    assert!(!disk.exists("hello.txt").await.expect("exists after delete"));
    assert!(matches!(
        disk.get("hello.txt").await,
        Err(StorageError::NotFound { .. })
    ));

    app.shutdown().await.expect("clean shutdown");

    // Publish the embedded template into a temporary folder, edit it, and check the second run keeps it.
    let temp = tempfile::tempdir().expect("temp publish dir");
    let root = temp.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"ludhiana-warehouse\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("write Cargo.toml");

    let first = run_testing_publish(root, &["plugin:publish", "sanduk", "--tag", "views"]);
    assert!(
        first.contains("resources/views/sanduk/list.html: written"),
        "{first}"
    );
    let published = root.join("resources/views/sanduk/list.html");
    assert_eq!(
        fs::read_to_string(&published).expect("read published list.html"),
        FIXTURE_LIST_HTML
    );

    let custom = "<p>Updated notices for Ludhiana</p>\n";
    fs::write(&published, custom).expect("edit published list.html");
    let second = run_testing_publish(root, &["plugin:publish", "sanduk", "--tag", "views"]);
    assert!(
        second
            .contains("resources/views/sanduk/list.html: exists, kept (use --force to replace it)"),
        "{second}"
    );
    assert_eq!(
        fs::read_to_string(&published).expect("read kept list.html"),
        custom
    );

    // Any block on `docs/plugins/testing.mdx` that starts with `// plugins/...` is a slice of that file.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let testing_mdx = fs::read_to_string(repo_root.join("docs/plugins/testing.mdx"))
        .expect("read docs/plugins/testing.mdx")
        .replace("\r\n", "\n");
    for block in fenced_blocks(&testing_mdx) {
        if block.lang == "rust" {
            let lines: Vec<&str> = block.code.lines().collect();
            if let Some(rel_path) = lines
                .first()
                .and_then(|first| first.strip_prefix("// "))
                .filter(|path| path.starts_with("plugins/"))
            {
                assert_slice_of_file(&repo_root, rel_path.trim(), &lines[1..], block.line);
            }
        }
    }
}

#[test]
fn the_releasing_page_matches_what_a_plugin_ships() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let releasing_mdx = fs::read_to_string(repo_root.join("docs/plugins/releasing.mdx"))
        .expect("read docs/plugins/releasing.mdx")
        .replace("\r\n", "\n");

    assert!(
        releasing_mdx.contains("keywords = [\"navree-plugin\", \"navree\"]"),
        "releasing.mdx must carry the keyword array"
    );
    assert!(
        releasing_mdx.contains("It is not an official Navree package."),
        "releasing.mdx must carry the community plugin README sentence"
    );

    let blocks = fenced_blocks(&releasing_mdx);
    let mut checked_cargo_slice = false;
    for block in &blocks {
        if block.lang == "toml" {
            let lines: Vec<&str> = block.code.lines().collect();
            if lines.first().copied() == Some("# plugins/notices/Cargo.toml") {
                assert_slice_of_file(
                    &repo_root,
                    "plugins/notices/Cargo.toml",
                    &lines[1..],
                    block.line,
                );
                checked_cargo_slice = true;
            }
        }
    }
    assert!(
        checked_cargo_slice,
        "releasing.mdx must quote `# plugins/notices/Cargo.toml` as a slice"
    );

    for plugin in ["notices", "dakiya", "godam"] {
        let plugin_dir = repo_root.join("plugins").join(plugin);
        let readme = fs::read_to_string(plugin_dir.join("README.md"))
            .unwrap_or_else(|err| panic!("read plugins/{plugin}/README.md: {err}"));
        assert!(
            readme.contains("It is not an official Navree package."),
            "plugins/{plugin}/README.md missing non-official sentence"
        );

        let manifest_str = fs::read_to_string(plugin_dir.join("Cargo.toml"))
            .unwrap_or_else(|err| panic!("read plugins/{plugin}/Cargo.toml: {err}"));
        let manifest: toml::Value = toml::from_str(&manifest_str)
            .unwrap_or_else(|err| panic!("parse plugins/{plugin}/Cargo.toml: {err}"));
        let description = manifest["package"]["description"]
            .as_str()
            .unwrap_or_else(|| panic!("plugins/{plugin}/Cargo.toml missing package.description"));
        assert!(
            description.ends_with("Not an official Navree package."),
            "plugins/{plugin}/Cargo.toml description must end with `Not an official Navree package.`, got: {description:?}"
        );
    }
}
