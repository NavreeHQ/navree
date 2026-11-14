//! `plugin:publish` on a real disk. Every test runs in a folder of its own as
//! the current directory, one at a time (the current directory is the process's).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use navree_kernel::{
    CommandSpec, Module, ModuleBuilder, Navree, PublishTag, Publishable, plugin_info,
};

/// Held by each test while it runs in its folder.
static LOCK: Mutex<()> = Mutex::new(());

/// The folder `in_app_folder` made. On drop, even when the test fails, the
/// current directory goes back to the package's folder and the folder is
/// removed.
struct AppFolder(PathBuf);

impl Drop for AppFolder {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"));
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs `f` with a new empty folder as the current directory, then removes it.
fn in_app_folder(name: &str, f: impl FnOnce(&Path)) {
    // A test that failed while holding the lock poisoned it; the next one takes it over.
    let _lock = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let folder = AppFolder(
        std::env::temp_dir().join(format!("navree-publish-{name}-{}", std::process::id())),
    );
    let _ = fs::remove_dir_all(&folder.0);
    fs::create_dir_all(&folder.0).expect("an app folder");
    std::env::set_current_dir(&folder.0).expect("the app folder as the current directory");
    f(&folder.0);
}

/// `run_with_args(args)` for an app of `module`; the error as text.
fn publish(module: impl Module, args: &[&str]) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let args = args.iter().map(|arg| (*arg).to_owned()).collect();
    runtime
        .block_on(Navree::app().module(module).run_with_args(args))
        .map_err(|error| error.to_string())
}

/// Every file and folder below `root`, `/`-separated, sorted; links are listed, not followed.
fn files_under(root: &Path) -> Vec<String> {
    fn walk(folder: &Path, prefix: &str, out: &mut Vec<String>) {
        for entry in fs::read_dir(folder).expect("a folder") {
            let entry = entry.expect("a folder entry");
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            let metadata = fs::symlink_metadata(entry.path()).expect("its metadata");
            if metadata.is_dir() {
                walk(&entry.path(), &format!("{name}/"), out);
            }
            out.push(name);
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out.sort();
    out
}

/// Views `list.html`, `admin/edit.html`; locales `en/announcements.ftl`, `pa-IN/announcements.ftl`;
/// files `banner.css`; namespace `announcements` (plugin name `navree-kernel`).
struct Announcements;

impl Module for Announcements {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(plugin_info!(namespace = "announcements"))
            .contribute(
                Publishable::new(PublishTag::Views)
                    .file("list.html", b"<ul></ul>")
                    .file("admin/edit.html", b"<form></form>"),
            )
            .contribute(
                Publishable::new(PublishTag::Locales)
                    .file("en/announcements.ftl", b"announcements-title = Notices\n")
                    .file(
                        "pa-IN/announcements.ftl",
                        "announcements-title = ਸੂਚਨਾਵਾਂ\n".as_bytes(),
                    ),
            )
            .contribute(Publishable::new(PublishTag::Files).file("banner.css", b"body{}"));
    }
}

/// One file under `files`, at the path given, beside a safe `banner.css`.
struct Shipping(String);

impl Module for Shipping {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(plugin_info!(namespace = "announcements"))
            .contribute(
                Publishable::new(PublishTag::Files)
                    .file("banner.css", b"body{}")
                    .file(self.0.clone(), b"x"),
            );
    }
}

/// Makes `link` a junction to the folder `target` with `mklink /J`, which
/// needs no administrator rights, and checks it was made.
#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    // `mklink` reads a `/` as the start of an option: pass `\` only.
    let backslashed = |path: &Path| path.to_string_lossy().replace('/', "\\");
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(backslashed(link))
        .arg(backslashed(target))
        .output()
        .expect("cmd runs");
    assert!(
        output.status.success(),
        "mklink /J failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn plugin_publish_writes_only_inside_the_three_folders() {
    in_app_folder("three-folders", |root| {
        publish(Announcements, &["plugin:publish", "announcements"]).expect("published");
        assert_eq!(
            files_under(root),
            [
                "locales",
                "locales/en",
                "locales/en/announcements",
                "locales/en/announcements/announcements.ftl",
                "locales/pa-IN",
                "locales/pa-IN/announcements",
                "locales/pa-IN/announcements/announcements.ftl",
                "resources",
                "resources/announcements",
                "resources/announcements/banner.css",
                "resources/views",
                "resources/views/announcements",
                "resources/views/announcements/admin",
                "resources/views/announcements/admin/edit.html",
                "resources/views/announcements/list.html",
            ]
        );
        assert_eq!(
            fs::read(root.join("resources/views/announcements/list.html")).unwrap(),
            b"<ul></ul>"
        );
        publish(
            Announcements,
            &[
                "plugin:publish",
                "navree-kernel",
                "--tag",
                "files",
                "--force",
            ],
        )
        .expect("the crate name works as well as the namespace");
    });
}

#[test]
fn plugin_publish_keeps_an_existing_file_without_force() {
    in_app_folder("keep", |root| {
        let list = root.join("resources/views/announcements/list.html");
        fs::create_dir_all(list.parent().unwrap()).unwrap();
        fs::write(&list, "Simran Kaur's own list").unwrap();
        publish(
            Announcements,
            &["plugin:publish", "announcements", "--tag", "views"],
        )
        .expect("kept");
        assert_eq!(fs::read_to_string(&list).unwrap(), "Simran Kaur's own list");
        assert!(
            root.join("resources/views/announcements/admin/edit.html")
                .is_file()
        );
        publish(
            Announcements,
            &[
                "plugin:publish",
                "announcements",
                "--tag",
                "views",
                "--force",
            ],
        )
        .expect("replaced");
        assert_eq!(fs::read_to_string(&list).unwrap(), "<ul></ul>");
    });
}

#[test]
fn plugin_publish_dry_run_writes_nothing() {
    in_app_folder("dry-run", |root| {
        publish(
            Announcements,
            &["plugin:publish", "announcements", "--dry-run"],
        )
        .expect("dry run");
        assert!(files_under(root).is_empty());
        let list = root.join("resources/views/announcements/list.html");
        fs::create_dir_all(list.parent().unwrap()).unwrap();
        fs::write(&list, "Gurpreet Singh's list").unwrap();
        let before = files_under(root);
        publish(
            Announcements,
            &["plugin:publish", "announcements", "--dry-run", "--force"],
        )
        .expect("dry run");
        assert_eq!(files_under(root), before);
        assert_eq!(fs::read_to_string(&list).unwrap(), "Gurpreet Singh's list");
    });
}

#[test]
fn plugin_publish_replaces_by_rename_and_never_writes_through_a_hard_link() {
    in_app_folder("hard-link", |root| {
        let outside = root.join("outside.html");
        fs::write(&outside, "Ludhiana office notes").unwrap();
        let list = root.join("resources/views/announcements/list.html");
        fs::create_dir_all(list.parent().unwrap()).unwrap();
        fs::hard_link(&outside, &list).unwrap();
        publish(
            Announcements,
            &[
                "plugin:publish",
                "announcements",
                "--tag",
                "views",
                "--force",
            ],
        )
        .expect("replaced");
        assert_eq!(fs::read_to_string(&list).unwrap(), "<ul></ul>");
        assert_eq!(
            fs::read_to_string(&outside).unwrap(),
            "Ludhiana office notes"
        );
        assert!(
            !files_under(root)
                .iter()
                .any(|path| path.contains(".navree-publish-")),
            "no temporary file is left"
        );
    });
}

#[cfg(unix)]
#[test]
fn plugin_publish_refuses_a_destination_that_is_a_symbolic_link() {
    in_app_folder("symlink", |root| {
        let outside = root.join("outside.html");
        fs::write(&outside, "Ludhiana office notes").unwrap();
        let folder = root.join("resources/views/announcements");
        fs::create_dir_all(&folder).unwrap();
        std::os::unix::fs::symlink(&outside, folder.join("list.html")).unwrap();
        let error = publish(
            Announcements,
            &["plugin:publish", "announcements", "--force"],
        )
        .expect_err("a link");
        assert_eq!(
            error,
            "plugin:publish: resources/views/announcements/list.html is a symbolic link or inside one; nothing was written"
        );
        assert_eq!(
            fs::read_to_string(&outside).unwrap(),
            "Ludhiana office notes"
        );
        assert!(!root.join("locales").exists() && !root.join("resources/announcements").exists());

        fs::remove_dir_all(&folder).unwrap();
        fs::create_dir(root.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(root.join("elsewhere"), &folder).unwrap();
        let error = publish(Announcements, &["plugin:publish", "announcements"])
            .expect_err("a linked folder");
        assert_eq!(
            error,
            "plugin:publish: resources/views/announcements is a symbolic link or inside one; nothing was written"
        );
        assert!(
            fs::read_dir(root.join("elsewhere"))
                .unwrap()
                .next()
                .is_none()
        );
    });
}

#[cfg(windows)]
#[test]
fn plugin_publish_refuses_a_destination_folder_that_is_a_junction() {
    in_app_folder("junction", |root| {
        fs::create_dir(root.join("elsewhere")).unwrap();
        fs::create_dir_all(root.join("resources/views")).unwrap();
        junction(
            &root.join("resources/views/announcements"),
            &root.join("elsewhere"),
        );
        let error =
            publish(Announcements, &["plugin:publish", "announcements"]).expect_err("a junction");
        assert_eq!(
            error,
            "plugin:publish: resources/views/announcements is a symbolic link or inside one; nothing was written"
        );
        assert!(
            fs::read_dir(root.join("elsewhere"))
                .unwrap()
                .next()
                .is_none()
        );
    });
}

/// The link check covers the app's folder and below it, not the folders above it.
#[cfg(windows)]
#[test]
fn plugin_publish_works_in_an_app_inside_a_junction() {
    in_app_folder("inside-junction", |root| {
        fs::create_dir(root.join("real")).unwrap();
        junction(&root.join("app"), &root.join("real"));
        std::env::set_current_dir(root.join("app")).unwrap();
        publish(
            Announcements,
            &["plugin:publish", "announcements", "--tag", "files"],
        )
        .expect("published");
        assert!(
            root.join("real/resources/announcements/banner.css")
                .is_file()
        );
    });
}

#[test]
fn plugin_publish_refuses_a_destination_folder_that_is_a_file() {
    in_app_folder("folder-is-a-file", |root| {
        fs::create_dir(root.join("resources")).unwrap();
        fs::write(root.join("resources/views"), "a file").unwrap();
        let error =
            publish(Announcements, &["plugin:publish", "announcements"]).expect_err("a file");
        assert_eq!(
            error,
            "plugin:publish: resources/views is a file where a folder is needed; nothing was written"
        );
        assert_eq!(files_under(root), ["resources", "resources/views"]);
    });
}

#[test]
fn plugin_publish_names_the_plugins_and_tags_it_knows() {
    in_app_folder("names", |root| {
        let usage = "usage: plugin:publish <plugin> [--tag views|locales|config|files] [--force] [--dry-run]";
        for (args, expected) in [
            (
                vec!["plugin:publish", "weather"],
                "plugin:publish: no plugin is named `weather` (plugins: announcements)".to_owned(),
            ),
            (
                vec!["plugin:publish", "announcements", "--tag", "themes"],
                "plugin:publish: unknown tag `themes` (tags: views, locales, config, files)"
                    .to_owned(),
            ),
            (
                vec!["plugin:publish"],
                format!("plugin:publish: name the plugin to publish; {usage}"),
            ),
            (
                vec!["plugin:publish", "announcements", "--yes"],
                format!("plugin:publish: unexpected argument `--yes`; {usage}"),
            ),
            (
                vec!["plugin:publish", "announcements", "--tag"],
                "plugin:publish: --tag needs a tag (tags: views, locales, config, files)"
                    .to_owned(),
            ),
        ] {
            assert_eq!(
                publish(Announcements, &args).expect_err("refused"),
                expected
            );
        }
        struct NoPlugin;
        impl Module for NoPlugin {
            fn register(&self, _: &mut ModuleBuilder) {}
        }
        assert_eq!(
            publish(NoPlugin, &["plugin:publish", "announcements"]).expect_err("no plugin"),
            "plugin:publish: no plugin is named `announcements` (plugins: none)"
        );
        assert!(files_under(root).is_empty());
    });
}

#[test]
fn a_tag_with_nothing_to_publish_writes_nothing_and_succeeds() {
    in_app_folder("nothing", |root| {
        publish(
            Shipping("print.css".into()),
            &["plugin:publish", "announcements", "--tag", "views"],
        )
        .expect("nothing to do");
        assert!(files_under(root).is_empty());
    });
}

#[test]
fn plugin_publish_yields_to_a_module_command() {
    in_app_folder("yields", |root| {
        // The app's own `plugin:publish`, which records its arguments.
        struct OwnPublish(Arc<Mutex<Vec<String>>>);
        impl Module for OwnPublish {
            fn register(&self, m: &mut ModuleBuilder) {
                let seen = self.0.clone();
                m.command(CommandSpec::new(
                    "plugin:publish",
                    "The app's own publishing",
                    move |_, args| {
                        seen.lock().unwrap().extend(args);
                        async { Ok(()) }
                    },
                ));
            }
        }
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        publish(
            OwnPublish(seen.clone()),
            &["plugin:publish", "announcements", "--force"],
        )
        .expect("the app's own command");
        assert_eq!(*seen.lock().unwrap(), ["announcements", "--force"]);
        assert!(files_under(root).is_empty(), "nothing is written");
    });
}

/// One test per refused path shape: the boot fails naming the path, and nothing is written.
macro_rules! refused {
    ($($test:ident: [$($path:expr),+ $(,)?];)+) => { $(
        #[test]
        fn $test() {
            for path in [$(String::from($path)),+] {
                in_app_folder(stringify!($test), |root| {
                    let error = publish(Shipping(path.clone()), &["plugin:publish", "announcements"])
                        .expect_err(&path);
                    assert!(
                        error.contains(&format!("the files file `{path}` is not a safe relative path")),
                        "{error}"
                    );
                    assert!(files_under(root).is_empty(), "{path:?}");
                });
            }
        }
    )+ };
}

refused! {
    plugin_publish_refuses_a_parent_segment: ["../outside.css", "a/../../outside.css"];
    plugin_publish_refuses_an_absolute_path: ["/etc/navree.css", "/"];
    plugin_publish_refuses_a_backslash: ["a\\..\\..\\outside.css", "a\\b.css"];
    plugin_publish_refuses_a_drive_letter_or_a_stream: ["c:/outside.css", "C:outside.css", "banner.css:hidden"];
    plugin_publish_refuses_a_trailing_dot_or_space: ["list.html.", "list.html "];
    plugin_publish_refuses_a_segment_of_dots_and_spaces: [".../x.css", ". /x.css"];
    plugin_publish_refuses_a_device_name: [
        "con.html", "nul", "aux.txt", "com1.css", "lpt9", "CON", "con .txt", "conin$", "lpt².html",
    ];
    plugin_publish_refuses_a_reserved_character: ["a|b", "a*b", "a?b", "a\"b", "a<b", "a>b"];
    plugin_publish_refuses_a_control_character: ["a\u{1}b.css", "a\nb.css"];
    plugin_publish_refuses_an_empty_or_dot_segment: ["a//b.css", "./b.css", "a/"];
    plugin_publish_refuses_a_path_that_is_too_long: ["a".repeat(256), "a".repeat(129)];
}

#[test]
fn plugin_publish_refuses_two_names_that_differ_only_in_case() {
    in_app_folder("case", |root| {
        let error = publish(
            Shipping("Banner.css".into()),
            &["plugin:publish", "announcements"],
        )
        .expect_err("one file on a disk that ignores case");
        assert!(
            error.contains("the files file `Banner.css` is not a safe relative path"),
            "{error}"
        );
        assert!(files_under(root).is_empty());
    });
}
