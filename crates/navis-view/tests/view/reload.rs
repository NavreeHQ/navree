//! `ViewOptions::reload`: picking up template changes without a restart.

use std::path::Path;

use navis_view::{ViewError, ViewOptions, Views};
use serde_json::json;

use crate::support::{options, views_dir, write};

fn reloading(dir: &Path) -> ViewOptions {
    ViewOptions {
        reload: true,
        ..options(&[dir])
    }
}

#[test]
fn reload_picks_up_changed_files() {
    let dir = views_dir(&[("mail/welcome.html", "v1")]);
    let views = Views::new(&reloading(dir.path())).expect("views");
    assert_eq!(views.render("mail/welcome", json!({})).expect("v1"), "v1");

    write(dir.path(), "mail/welcome.html", "version 2");

    assert_eq!(
        views.render("mail/welcome", json!({})).expect("v2"),
        "version 2"
    );
}

#[test]
fn reload_sees_files_created_after_boot() {
    let dir = views_dir(&[]);
    let views = Views::with_templates(&reloading(dir.path()), [("mail/welcome.html", "embedded")])
        .expect("views");
    assert!(!views.exists("mail/new"));
    assert_eq!(
        views.render("mail/welcome", json!({})).expect("embedded"),
        "embedded"
    );

    write(dir.path(), "mail/new.txt", "new");
    write(dir.path(), "mail/welcome.html", "file");

    assert_eq!(views.render("mail/new", json!({})).expect("new"), "new");
    assert_eq!(
        views.render("mail/welcome", json!({})).expect("file"),
        "file"
    );
}

#[test]
fn without_reload_changes_are_not_seen() {
    let dir = views_dir(&[("mail/welcome.html", "v1")]);
    let views = Views::new(&options(&[dir.path()])).expect("views");

    write(dir.path(), "mail/welcome.html", "version 2");
    write(dir.path(), "mail/new.html", "new");

    assert_eq!(views.render("mail/welcome", json!({})).expect("v1"), "v1");
    assert!(!views.exists("mail/new"));
}

#[test]
fn reload_still_checks_syntax_when_built() {
    let dir = views_dir(&[("mail/broken.html", "{{ name")]);

    let error = Views::new(&reloading(dir.path())).expect_err("syntax error");

    assert!(
        matches!(&error, ViewError::Template { name, .. } if name == "mail/broken.html"),
        "{error:?}"
    );
}

#[test]
fn names_cannot_escape_the_template_directories() {
    let base = views_dir(&[("views/mail/welcome.html", "ok"), ("secret.html", "secret")]);
    let views_path = base.path().join("views");
    let absolute = base.path().join("secret.html").display().to_string();
    for reload in [false, true] {
        let views = Views::new(&ViewOptions {
            dirs: vec![views_path.clone()],
            reload,
        })
        .expect("views");
        assert!(views.exists("mail/welcome"), "reload={reload}");

        for name in [
            "../secret",
            "../secret.html",
            "mail/../../secret",
            r"..\secret",
            r"mail\..\..\secret",
            "/secret",
            absolute.as_str(),
        ] {
            let result = views.render(name, json!({}));
            assert!(
                matches!(result, Err(ViewError::NotFound { .. })),
                "reload={reload} {name}: {result:?}"
            );
        }
    }
}

#[test]
fn reload_falls_back_to_embedded_when_the_file_is_removed() {
    let dir = views_dir(&[("mail/welcome.html", "file")]);
    let views = Views::with_templates(&reloading(dir.path()), [("mail/welcome.html", "embedded")])
        .expect("views");
    assert_eq!(
        views.render("mail/welcome", json!({})).expect("file"),
        "file"
    );

    std::fs::remove_file(dir.path().join("mail/welcome.html")).expect("remove");

    assert_eq!(
        views.render("mail/welcome", json!({})).expect("embedded"),
        "embedded"
    );
}
