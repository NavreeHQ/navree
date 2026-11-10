//! `Views::render`: name resolution, file and embedded templates, escaping
//! and errors.

use navree_view::{ViewError, Views};
use serde_json::json;

use crate::support::{options, views_dir};

#[test]
fn render_source_renders_a_template_kept_as_text() {
    let views = Views::from_templates([(
        "layouts/mail.html",
        "<main>{% block body %}{% endblock %}</main>",
    )])
    .expect("views");

    // The name picks the escaping, like a file's extension.
    let html = views
        .render_source(
            "override.html",
            "{% extends \"layouts/mail.html\" %}{% block body %}Hi {{ name }}{% endblock %}",
            json!({ "name": "<Anu>" }),
        )
        .expect("render");
    assert_eq!(html, "<main>Hi &lt;Anu&gt;</main>");

    let text = views
        .render_source("subject.txt", "Hi {{ name }}", json!({ "name": "<Anu>" }))
        .expect("render");
    assert_eq!(text, "Hi <Anu>");

    // The filters and the strict undefined rule of every template apply.
    let error = views
        .render_source("subject.txt", "{{ missing }}", json!({}))
        .expect_err("undefined");
    assert!(
        matches!(&error, ViewError::Template { name, .. } if name == "subject.txt"),
        "{error:?}"
    );
    let error = views
        .render_source("broken.html", "{% if %}", json!({}))
        .expect_err("syntax");
    assert!(matches!(error, ViewError::Template { .. }), "{error:?}");
}

#[test]
fn renders_file_template() {
    let dir = views_dir(&[("mail/welcome.html", "Hello {{ name }}!")]);
    let views = Views::new(&options(&[dir.path()])).expect("views");

    let html = views
        .render("mail/welcome", json!({ "name": "Anu" }))
        .expect("render");

    assert_eq!(html, "Hello Anu!");
}

#[test]
fn embedded_template_used_when_no_file() {
    let dir = views_dir(&[]);
    let views = Views::with_templates(
        &options(&[dir.path()]),
        [("mail/welcome.html", "Embedded {{ name }}")],
    )
    .expect("views");

    let html = views
        .render("mail/welcome", json!({ "name": "Anu" }))
        .expect("render");

    assert_eq!(html, "Embedded Anu");
}

#[test]
fn file_overrides_embedded() {
    let dir = views_dir(&[("mail/welcome.html", "File {{ name }}")]);
    let views = Views::with_templates(
        &options(&[dir.path()]),
        [("mail/welcome.html", "Embedded {{ name }}")],
    )
    .expect("views");

    let html = views
        .render("mail/welcome", json!({ "name": "Anu" }))
        .expect("render");

    assert_eq!(html, "File Anu");
}

#[test]
fn earlier_directory_wins() {
    let app = views_dir(&[("mail/welcome.html", "app")]);
    let vendor = views_dir(&[("mail/welcome.html", "vendor")]);
    let views = Views::new(&options(&[app.path(), vendor.path()])).expect("views");

    assert_eq!(
        views.render("mail/welcome", json!({})).expect("render"),
        "app"
    );
}

#[test]
fn html_is_escaped() {
    let views = Views::from_templates([
        ("mail/welcome.html", "<p>{{ name }}</p>"),
        ("mail/welcome.txt", "{{ name }}"),
    ])
    .expect("views");
    let ctx = json!({ "name": "<script>alert(\"x\")</script> & co" });

    let html = views
        .render("mail/welcome.html", &ctx)
        .expect("render html");
    let text = views.render("mail/welcome.txt", &ctx).expect("render text");

    assert_eq!(
        html,
        "<p>&lt;script&gt;alert(&quot;x&quot;)&lt;&#x2f;script&gt; &amp; co</p>"
    );
    assert_eq!(text, "<script>alert(\"x\")</script> & co");
}

#[test]
fn name_without_extension_tries_html_then_txt_then_j2() {
    let views = Views::from_templates([
        ("a.html", "a.html"),
        ("a.txt", "a.txt"),
        ("b.txt", "b.txt"),
        ("b.j2", "b.j2"),
        ("c.j2", "c.j2"),
    ])
    .expect("views");

    assert_eq!(views.render("a", json!({})).expect("a"), "a.html");
    assert_eq!(views.render("b", json!({})).expect("b"), "b.txt");
    assert_eq!(views.render("c", json!({})).expect("c"), "c.j2");
    assert_eq!(views.render("a.txt", json!({})).expect("a.txt"), "a.txt");
}

#[test]
fn missing_template_error_names_template() {
    let dir = views_dir(&[]);
    let views = Views::new(&options(&[dir.path()])).expect("views");

    let error = views
        .render("mail/missing", json!({}))
        .expect_err("missing template");

    assert!(
        matches!(&error, ViewError::NotFound { name, .. } if name == "mail/missing"),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("mail/missing"), "{message}");
    assert!(message.contains("mail/missing.html"), "{message}");
}

#[test]
fn non_template_and_hidden_files_are_ignored() {
    let dir = views_dir(&[
        ("mail/welcome.html", "hi"),
        (".hidden/secret.html", "secret"),
        ("mail/.draft.html", "draft"),
    ]);
    std::fs::write(dir.path().join("mail/logo.png"), [0xff, 0xfe, 0x00, 0x9f]).expect("png");
    let views = Views::new(&options(&[dir.path()])).expect("views");

    assert!(views.exists("mail/welcome"));
    assert!(!views.exists(".hidden/secret"));
    assert!(!views.exists("mail/.draft"));
    assert!(!views.exists("mail/logo.png"));
}

#[test]
fn missing_directory_is_skipped() {
    let dir = views_dir(&[]);
    let missing = dir.path().join("does-not-exist");
    let views = Views::with_templates(&options(&[&missing]), [("a.txt", "a")]).expect("views");

    assert_eq!(views.render("a", json!({})).expect("render"), "a");
}

#[test]
fn syntax_error_names_template() {
    let dir = views_dir(&[("mail/broken.html", "{% if %}")]);

    let error = Views::new(&options(&[dir.path()])).expect_err("syntax error");

    assert!(
        matches!(&error, ViewError::Template { name, .. } if name == "mail/broken.html"),
        "{error:?}"
    );
    assert!(error.to_string().contains("mail/broken.html"), "{error}");
}

#[test]
fn printing_an_undefined_variable_fails() {
    let views = Views::from_templates([
        ("typo.txt", "Hello {{ nmae }}"),
        (
            "optional.txt",
            "{% if title %}{{ title }} {% endif %}{{ name }}",
        ),
    ])
    .expect("views");

    let error = views
        .render("typo", json!({ "name": "Anu" }))
        .expect_err("undefined variable");
    assert!(
        matches!(&error, ViewError::Template { name, .. } if name == "typo.txt"),
        "{error:?}"
    );
    assert_eq!(
        views
            .render("optional", json!({ "name": "Anu" }))
            .expect("render"),
        "Anu"
    );
}

#[test]
fn file_templates_extend_and_include_embedded_ones() {
    let dir = views_dir(&[(
        "mail/welcome.html",
        r#"{% extends "layouts/mail.html" %}{% block body %}Hi {{ name }}{% endblock %}"#,
    )]);
    let views = Views::with_templates(
        &options(&[dir.path()]),
        [
            (
                "layouts/mail.html",
                r#"<main>{% block body %}{% endblock %}</main>{% include "partials/footer.html" %}"#,
            ),
            ("partials/footer.html", "<footer>{{ name }}</footer>"),
        ],
    )
    .expect("views");

    let html = views
        .render("mail/welcome", json!({ "name": "A&B" }))
        .expect("render");

    assert_eq!(html, "<main>Hi A&amp;B</main><footer>A&amp;B</footer>");
}

#[test]
fn render_errors_do_not_include_context_values() {
    let views = Views::from_templates([("reset.txt", "{{ token + 1 }}")]).expect("views");

    let error = views
        .render("reset", json!({ "token": "s3cr3t-reset-token" }))
        .expect_err("type error");

    assert!(!error.to_string().contains("s3cr3t"), "{error}");
    assert!(!format!("{error:?}").contains("s3cr3t"), "{error:?}");
}

#[test]
fn later_embedded_template_replaces_earlier() {
    let views = Views::from_templates([("a.txt", "first"), ("a.txt", "second")]).expect("views");

    assert_eq!(views.render("a", json!({})).expect("render"), "second");
}

/// A context whose serialization fails.
struct Unserializable;

impl serde::Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("cannot serialize"))
    }
}

#[test]
fn context_serialization_error_is_a_render_error() {
    let views = Views::from_templates([("a.txt", "{{ name }}")]).expect("views");

    let error = views.render("a", Unserializable).expect_err("bad context");

    assert!(
        matches!(&error, ViewError::Template { name, .. } if name == "a.txt"),
        "{error:?}"
    );
}

#[test]
fn directory_symlink_loops_are_not_followed_forever() {
    let dir = views_dir(&[("mail/welcome.html", "hi")]);
    let link = dir.path().join("mail").join("again");
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(dir.path(), &link);
    // Directory symlinks on Windows need developer mode or elevation; a
    // junction (`mklink /J`) does not, and loops the same way.
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(dir.path(), &link).or_else(|_| {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(dir.path())
            .stdout(std::process::Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("mklink /J failed: {status}")))
        }
    });
    if let Err(error) = linked {
        eprintln!("skipped: cannot create a directory link: {error}");
        return;
    }
    assert!(
        link.join("mail").join("welcome.html").is_file(),
        "link works"
    );

    for reload in [false, true] {
        let views = Views::new(&navree_view::ViewOptions {
            reload,
            ..options(&[dir.path()])
        })
        .expect("views");

        assert_eq!(
            views.render("mail/welcome", json!({})).expect("render"),
            "hi"
        );
        if !reload {
            // A directory reached again through the link is not walked again.
            assert!(!views.exists("mail/again/mail/welcome"));
        }
    }
}

#[test]
fn every_name_has_a_mode_and_unknown_names_escape_html() {
    let hostile = "<img src=x onerror=alert(1)>";
    let ctx = json!({ "x": hostile });
    let views = Views::from_templates([
        (
            "layouts/mail.html",
            "<main>{{ x }}{% block body %}{% endblock %}</main>",
        ),
        ("note.txt", "NOTE {{ x }}"),
        ("page.j2", "<p>{{ x }}</p>"),
    ])
    .expect("views");

    // Whatever is not known to be text or JSON escapes HTML.
    for name in [
        "a.html",
        "a.HTML",
        "a.HTML.j2",
        "a.htm",
        "a.xml",
        "a.xhtml",
        "a.svg",
        "a.j2",
        "a.jinja",
        "a.jinja2",
        "a",
        "a.html.",
        "a.mjml",
        "a.vue",
        "a.unknown",
        "Override.HTML",
    ] {
        assert_eq!(
            views.render_source(name, "{{ x }}", &ctx).expect("renders"),
            "&lt;img src=x onerror=alert(1)&gt;",
            "{name}"
        );
    }
    // Text prints as it is.
    for name in ["a.txt", "a.TXT", "a.md", "a.txt.j2", "a.md.jinja"] {
        assert_eq!(
            views.render_source(name, "{{ x }}", &ctx).expect("renders"),
            hostile,
            "{name}"
        );
    }
    // Data prints as JSON.
    for name in [
        "a.json", "a.JSON", "a.json5", "a.js", "a.yaml", "a.yml", "a.js.j2",
    ] {
        assert_eq!(
            views.render_source(name, "{{ x }}", &ctx).expect("renders"),
            format!("\"{hostile}\""),
            "{name}"
        );
    }
    // A name found by trying extensions escapes by the name it found.
    assert!(
        !views
            .render("page", &ctx)
            .expect("renders")
            .contains("<img")
    );
    // Layouts and includes keep their own mode, and can't print less strictly.
    for (name, source) in [
        (
            "subject.txt",
            r#"{% extends "layouts/mail.html" %}{% block body %}{{ x }}{% endblock %}"#,
        ),
        ("override", r#"{% extends "layouts/mail.html" %}"#),
        ("override.html", r#"<p>{% include "note.txt" %}</p>"#),
    ] {
        let out = views.render_source(name, source, &ctx).expect("renders");
        assert!(!out.contains("<img"), "{name}: {out}");
    }
}

#[cfg(windows)]
#[test]
fn another_name_for_a_file_finds_nothing() {
    let dir = views_dir(&[
        ("page.html", "PAGE"),
        ("averylongtemplatename.html", "LONG"),
        (".hiddendirectory/secret.html", "HIDDEN"),
    ]);
    let mut opts = options(&[dir.path()]);
    opts.reload = true;
    let views = Views::new(&opts).expect("views");
    assert_eq!(views.render("page.html", json!({})).expect("page"), "PAGE");
    for name in [
        "HIDDEN~1/secret.html",
        "AVERYL~1.html",
        "AVERYL~1.htm",
        "NUL.html",
        "CON.html",
    ] {
        assert!(
            matches!(
                views.render(name, json!({})),
                Err(ViewError::NotFound { .. })
            ),
            "{name}"
        );
    }
    let error = views
        .render_source("t.txt", "{% include 'HIDDEN~1/secret.html' %}", json!({}))
        .expect_err("not found");
    assert!(!error.to_string().contains("HIDDEN {{"), "{error}");
}
